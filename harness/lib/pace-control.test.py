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

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pace_control
import pace_sampler


class Pace(unittest.TestCase):
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
                     "reset_h": 4, "age_s": 0, "as_of": "sample", "resets_at": "reset", "projected_pct": 368}
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
