#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import random
import subprocess
import sys
import tempfile
import time
import unittest
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pace_control
import pace_sampler


class Pace(unittest.TestCase):
    def test_deadline_target_admits_spenddown_and_retains_reset_defaults(self):
        now = float(int(time.time()))
        by = datetime.fromtimestamp(now + 33 * 3600, timezone.utc).isoformat()
        window = {"window": "week", "used_pct": 18, "burn_pct_h": 2.02, "reset_h": 157,
                  "resets_at": "reset", "age_s": 0, "as_of": datetime.fromtimestamp(now, timezone.utc).isoformat(),
                  "projected_pct": 335.14}
        previous = {"at": now - 60, "workers": 2, "allowed_workers": 1, "held": True, "windows": []}
        with tempfile.TemporaryDirectory() as scratch:
            policy = Path(scratch) / "orchestration.toml"
            policy.write_text(f'[pace.codex]\nspend_to_pct=90\nby="{by}"\n')
            old_env = dict(os.environ)
            try:
                os.environ["AGENTS_ORCHESTRATION"] = str(policy)
                target = pace_control.target_for("codex", now)
                control = pace_control.advance(previous, [window], 2, now, initial=5, target=target)
                self.assertFalse(control["held"])
                self.assertEqual(control["allowed_workers"], 6)
                self.assertAlmostEqual(control["windows"][0]["projected_pct"], 84.66)
                self.assertIsNone(pace_control.refusal("codex", control, now))
                spec = importlib.util.spec_from_file_location("deadline_capacity", HERE.parent / "hooks/lib/spawn_capacity.py")
                capacity = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(capacity)
                os.environ.update(SPAWN_CAPACITY_HOME=scratch, XDG_STATE_HOME=str(Path(scratch) / ".local/state"))
                state = Path(scratch) / ".local/state/agents"
                state.mkdir(parents=True)
                (state / "usage-gate.json").write_text(json.dumps({"codex": {
                    "epoch": now, "stale_min": 30, "pace_control": True, "refusal": None,
                    "controls": {"codex": control}, "projections": {"codex": [window]}}}))
                event = {"tool_name": "mcp__native__spawn_agent", "session_id": "fixture", "cwd": str(Path.home()),
                         "tool_input": {"message": "[routine:watchdog]\nItem: north#62", "task_name": "deadline_fixture"}}
                self.assertIsNone(capacity.evaluate(event)[0])
                native_week = window | {"resets_at": datetime.fromtimestamp(now + 157 * 3600, timezone.utc).isoformat(),
                                        "burn_pct_h": 4.83}
                under_line = pace_control.advance(previous, [native_week], 5, now, initial=5, target=target)
                self.assertFalse(under_line["held"])
                self.assertIsNone(pace_control.refusal("codex", under_line, now))
                above_line = pace_control.advance(previous, [native_week | {"used_pct": 24}], 5, now, target=target)
                self.assertTrue(above_line["held"])
                over = pace_control.advance(control, [window | {"used_pct": 99}], 2, now + 60, target=target)
                self.assertTrue(over["held"])
                self.assertIn(target["by"], pace_control.refusal("codex", over, now + 60))
                claude = pace_control.advance(previous, [window], 2, now)
                self.assertTrue(claude["held"])
                self.assertIn("to reset", pace_control.refusal("claude:c", claude, now))
                self.assertIsNone(pace_control.target_for("codex", now + 34 * 3600))
                expired_at = now + 34 * 3600
                expired = control | {"at": expired_at, "windows": [control["windows"][0] | {
                    "as_of": datetime.fromtimestamp(expired_at, timezone.utc).isoformat()}]}
                self.assertIn("to reset", pace_control.refusal("codex", expired, expired_at))
                policy.write_text('[pace.codex]\nspend_to_pct=90\nby="not-a-time"\n')
                self.assertIn("invalid", pace_control.refusal("codex", control, now))
            finally:
                os.environ.clear()
                os.environ.update(old_env)

    def test_codex_delivery_passes_the_public_send_parser(self):
        with tempfile.TemporaryDirectory() as scratch:
            previous = dict(os.environ)
            try:
                os.environ.update(NORTH_CODEX_POOLED_HOME=scratch, NORTH_CODEX_CONVERSATION_HOME=scratch)
                code, _, error = pace_sampler.send_codex("00000000-0000-7000-8000-000000000001", "HOLD-SPAWNS fixture")
                self.assertEqual(code, 1)
                self.assertIn("no shared Codex app-server", error)
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_profile_metadata_timeouts_share_one_parallel_budget(self):
        import hashlib
        import socket
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            policy = root / "orchestration.toml"
            host = socket.gethostname()
            rows = []
            for label in ("a", "b", "c"):
                directory = root / label
                directory.mkdir()
                rows.extend([f'[accounts.claude.profiles.{label}."{host}"]', f'path="{directory}"'])
            policy.write_text("\n".join(rows))
            vendor = root / "claude"
            vendor.write_text("#!/bin/sh\nsleep 30\n")
            vendor.chmod(0o755)
            previous = dict(os.environ)
            try:
                os.environ.update(AGENTS_ORCHESTRATION=str(policy), PATH=str(root) + os.pathsep + os.environ["PATH"])
                start = time.monotonic()
                counts = pace_sampler.worker_counts(["claude:a", "claude:b", "claude:c"], timeout=0.3)
                self.assertEqual(counts, {"claude:a": None, "claude:b": None, "claude:c": None})
                self.assertLess(time.monotonic() - start, 0.8)
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_source_timestamp_limits_admission_age(self):
        randomizer = random.Random(120)
        now = 10000
        for _ in range(250):
            age = randomizer.uniform(-10, 180)
            window = {"window": "week", "age_s": 0, "as_of": datetime.fromtimestamp(now - age, timezone.utc).isoformat(),
                      "used_pct": 20, "burn_pct_h": 0, "reset_h": 100, "projected_pct": 20}
            control = {"at": now - 30, "held": False, "workers": 0, "allowed_workers": 2, "windows": [window]}
            reason = pace_control.refusal("fixture", control, now)
            self.assertEqual(reason is None, 0 <= age <= 120)
            self.assertEqual(pace_control.fresh(control, now), 0 <= age <= 120)

    def test_concurrent_admissions_consume_the_sampled_allowance(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "usage-gate.json"
            control = {"at": time.time(), "held": False, "workers": 0, "allowed_workers": 2, "windows": []}
            path.write_text(json.dumps({"codex": {"controls": {"codex": control}}}))
            command = [sys.executable, "-c", "import sys;sys.path.insert(0,sys.argv[1]);import pace_sampler;from pathlib import Path;print(pace_sampler.reserve(Path(sys.argv[2]),'codex','codex') or 'ALLOW')", str(HERE), str(path)]
            processes = [subprocess.Popen(command, stdout=subprocess.PIPE, text=True) for _ in range(8)]
            results = [p.communicate(timeout=5)[0].strip() for p in processes]
            self.assertLessEqual(results.count("ALLOW"), 2)
            self.assertEqual(json.loads(path.read_text())["codex"]["controls"]["codex"]["workers"], results.count("ALLOW"))
            while pace_sampler.reserve(path, "codex", "codex") is None:
                results.append("ALLOW")
            self.assertEqual(results.count("ALLOW"), 2)

    def test_measured_ramp_never_raises_on_missing_or_over_budget(self):
        randomizer = random.Random(61)
        for _ in range(250):
            used = randomizer.uniform(0, 50)
            delta = randomizer.uniform(0.001, 2)
            count = randomizer.randint(1, 8)
            hours = randomizer.uniform(0.1, 200)
            def window(value, stamp):
                return [{"window": "week", "used_pct": value, "burn_pct_h": 0,
                         "reset_h": hours, "resets_at": "reset", "age_s": 0,
                         "as_of": stamp, "projected_pct": value}]
            old = pace_control.advance({}, window(used, "first"), count, 60)
            new = pace_control.advance(old, window(used + delta, "second"), count, 120)
            self.assertAlmostEqual(new["worker_minutes"], count)
            self.assertAlmostEqual(new["burn_pct_worker_min"]["week"], delta / count)
            if used + delta + delta * 60 * hours > 100:
                self.assertTrue(new["held"])
                self.assertEqual(new["allowed_workers"], 1)
            elif used + delta + delta / count * 60 * 3 * hours <= 90:
                self.assertEqual(new["allowed_workers"], 3)
            else:
                self.assertEqual(new["allowed_workers"], 2)
            repeated = pace_control.advance(new, window(used + delta, "second"), count, 130)
            self.assertEqual(repeated["allowed_workers"], new["allowed_workers"])
            missing = pace_control.advance(new, window(used + delta, "third"), None, 180)
            self.assertTrue(missing["held"])
            self.assertLessEqual(missing["allowed_workers"], new["allowed_workers"])
        self.assertEqual(pace_control.interval(60, 10), 60)
        self.assertEqual(pace_control.interval(60, 25), 60)
        self.assertEqual(pace_control.interval(60, 40), 80)
        self.assertEqual(pace_control.interval(60, 2, True, 180), 180)
        self.assertIn("not fresh", pace_control.refusal("fixture", new | {"at": 0}, 121))

    def test_provider_spawn_journey_holds_projection_and_allowance(self):
        path = HERE.parent / "hooks/lib/spawn_capacity.py"
        spec = importlib.util.spec_from_file_location("capacity", path)
        capacity = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(capacity)
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            previous = dict(os.environ)
            try:
                os.environ.update(SPAWN_CAPACITY_HOME=scratch, XDG_STATE_HOME=str(root / ".local/state"),
                                  AGENTS_ORCHESTRATION=str(root / "policy.toml"))
                (root / "policy.toml").write_text("")
                state = root / ".local/state/agents"
                state.mkdir(parents=True)
                now = time.time()
                w = {"window": "5h", "used_pct": 52, "burn_pct_h": 79,
                     "reset_h": 4, "age_s": 0, "as_of": datetime.fromtimestamp(now, timezone.utc).isoformat(), "resets_at": "reset", "projected_pct": 368}
                control = pace_control.advance({}, [w], 1, now)
                for provider, tool in (("claude", "Agent"), ("codex", "mcp__native__spawn_agent")):
                    verdict = {"epoch": now, "stale_min": 30, "refusal": None, "pace_control": True,
                               "controls": {provider: control}}
                    (state / "usage-gate.json").write_text(json.dumps({provider: verdict}))
                    event = {"tool_name": tool, "session_id": "thread", "cwd": str(Path.home()),
                             "tool_input": {"cmd": "cat AGENTS.md", "workdir": str(root / "lane"),
                                            "message": "CASE=URGENT FACT='required urgent fix'\n[routine:watchdog]\nItem: north#1"}}
                    reason, warning = capacity.evaluate(event)
                    self.assertIn("368.0% > 100%", reason)
                    self.assertIn(provider, reason)
                    self.assertIsNone(warning)
                    verdict["controls"][provider] = control | {"held": False, "windows": [w | {"projected_pct": 52, "burn_pct_h": 0}], "workers": 2, "allowed_workers": 2}
                    (state / "usage-gate.json").write_text(json.dumps({provider: verdict}))
                    self.assertIn("workers 2/2", capacity.evaluate(event)[0])
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_provider_read_cleans_up_timed_out_process_group(self):
        with tempfile.TemporaryDirectory() as scratch:
            script = Path(scratch) / "adapter.py"
            script.write_text("import json\nprint(json.dumps({'provider':'fixture','kind':'window'}))\n")
            rows, measure = pace_sampler.read_adapter(script)
            self.assertEqual(rows, [{"provider": "fixture", "kind": "window"}])
            self.assertEqual(measure["code"], 0)
            start = time.monotonic()
            code, _, reason = pace_sampler.run([sys.executable, "-c", "import subprocess,time; subprocess.Popen(['sleep','30']); time.sleep(30)"], timeout=0.1)
            self.assertEqual(code, 124)
            self.assertEqual(reason, "timeout")
            self.assertLess(time.monotonic() - start, 5)


if __name__ == "__main__":
    unittest.main()
