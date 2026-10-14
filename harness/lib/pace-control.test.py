#!/usr/bin/env python3
import importlib.util
import base64
import hashlib
import json
import os
from pathlib import Path
import random
import socket
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time
import threading
import unittest
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pace_control
import pace_sampler


class Pace(unittest.TestCase):
    def test_native_read_survives_delayed_remote_snapshot_to_spawn(self):
        import usage_codex
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / ".local/state/agents"
            state.mkdir(parents=True)
            snapshot = root / ".local/state/threads/snapshot.db"
            snapshot.parent.mkdir()
            remote = root / ".config/threads/remote"
            remote.parent.mkdir(parents=True)
            remote.write_text("unreachable-fixture")
            transport = root / "ssh"
            transport.write_text("#!/bin/sh\ncat >/dev/null\nexit 255\n")
            transport.chmod(0o755)
            now = time.time()
            stamp = lambda at: datetime.fromtimestamp(at, timezone.utc).isoformat()
            reset = now + 140.53665573361113 * 3600
            rows = [dict(ts=stamp(now - ago), provider="codex", account=hashlib.sha256(b"fixture").hexdigest()[:8],
                         kind="window", name="week", used_pct=used, window_min=10080,
                         resets_at=usage_codex.utc(reset), source=usage_codex.SOURCE)
                    for ago, used in ((3600, 34), (174, 35))]
            with sqlite3.connect(snapshot) as db:
                pace_sampler.usage.record(db, rows)
            policy = root / "policy.toml"
            policy.write_text(f'[pace.codex]\nspend_to_pct=95\nby="{stamp(now + 38.97941374586688 * 3600)}"\n')
            pooled = root / "pool"
            endpoint = pooled / "app-server-control/app-server-control.sock"
            endpoint.parent.mkdir(parents=True)
            listener = socket.socket(socket.AF_UNIX)
            listener.bind(str(endpoint))
            listener.listen()
            listener.settimeout(0.1)
            stopped, failed = threading.Event(), threading.Event()
            errors = []

            def serve():
                while not stopped.is_set():
                    try:
                        peer, _ = listener.accept()
                    except socket.timeout:
                        continue
                    except OSError:
                        break
                    try:
                        with peer:
                            header = bytearray()
                            while not header.endswith(b"\r\n\r\n"):
                                header.extend(usage_codex.read_exact(peer, 1))
                            key = next(line.split(b":", 1)[1].strip() for line in header.split(b"\r\n")
                                       if line.lower().startswith(b"sec-websocket-key:"))
                            accept = base64.b64encode(hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
                            peer.sendall(b"HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
                            while True:
                                head = usage_codex.read_exact(peer, 2)
                                length = head[1] & 127
                                if length in (126, 127):
                                    length = int.from_bytes(usage_codex.read_exact(peer, 2 if length == 126 else 8), "big")
                                mask = usage_codex.read_exact(peer, 4)
                                data = usage_codex.read_exact(peer, length)
                                request = json.loads(bytes(value ^ mask[i % 4] for i, value in enumerate(data)))
                                if "id" not in request:
                                    continue
                                result = {"initialize": {}, "account/read": {"account": {"accountId": "fixture"}},
                                          "thread/loaded/list": {"data": ["fixture-worker"], "nextCursor": None},
                                          "thread/read": {"thread": {"status": {"type": "active"}}},
                                          usage_codex.SOURCE: {"rateLimits": {"limitId": "codex", "primary": {
                                              "windowDurationMins": 10080, "usedPercent": 35, "resetsAt": int(reset)},
                                              "credits": {"hasCredits": False, "ordinaryUsageAllowed": True}}}}[request["method"]]
                                response = {"id": request["id"], "result": result}
                                if failed.is_set() and request["method"] == usage_codex.SOURCE:
                                    response = {"id": request["id"], "error": {"code": -32000}}
                                data = json.dumps(response).encode()
                                peer.sendall(bytes([0x81, len(data)]) + data if len(data) < 126 else
                                             bytes([0x81, 126]) + struct.pack("!H", len(data)) + data)
                    except ConnectionError:
                        pass
                    except Exception as error:
                        errors.append(error)

            server = threading.Thread(target=serve)
            server.start()
            env = dict(os.environ, HOME=scratch, XDG_STATE_HOME=str(root / ".local/state"),
                       NORTH_CODEX_POOLED_HOME=str(pooled), NORTH_CODEX_CONVERSATION_HOME=str(pooled),
                       AGENTS_ORCHESTRATION=str(policy), AGENTS_ORG_FILE=str(state / "org.json"),
                       PATH=str(root) + os.pathsep + os.environ["PATH"])
            env.pop("THREADS_DB", None)
            command = [sys.executable, "-I", str(HERE / "pace_sampler.py"), "--provider", "codex", "--dry-run"]
            try:
                run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=15)
                self.assertEqual(run.returncode, 0, run.stderr)
                control = json.loads((state / "usage-gate.json").read_text())["codex"]["controls"]["codex"]
                self.assertFalse(control["held"], control)
                self.assertEqual(control["allowed_workers"], 3)
                self.assertTrue(pace_control.fresh(control, time.time()))
                self.assertGreater(control["windows"][0]["as_of"], rows[-1]["ts"])
                capacity = HERE.parent / "hooks/lib/spawn_capacity.py"
                event = {"tool_name": "mcp__native__spawn_agent", "session_id": "fixture", "cwd": scratch,
                         "tool_input": {"message": "[routine:watchdog]\nItem: north#9", "task_name": "fixture"}}
                spawn = subprocess.run([sys.executable, "-I", "-c",
                    "import importlib.util,json,sys; s=importlib.util.spec_from_file_location('capacity',sys.argv[1]); m=importlib.util.module_from_spec(s); s.loader.exec_module(m); print(json.dumps(m.evaluate(json.load(sys.stdin))))",
                    str(capacity)], input=json.dumps(event), env=env | {"SPAWN_CAPACITY_HOME": scratch},
                    capture_output=True, text=True, timeout=10)
                self.assertEqual(spawn.returncode, 0, spawn.stderr)
                self.assertEqual(json.loads(spawn.stdout), [None, None])
                failed.set()
                (state / "pace-control.json").write_text(json.dumps({"accounts": {}, "providers": {}, "history": []}))
                run = subprocess.run(command, env=env, capture_output=True, text=True, timeout=15)
                self.assertEqual(run.returncode, 0, run.stderr)
                state_data = json.loads((state / "pace-control.json").read_text())
                self.assertEqual(state_data["providers"]["codex"]["code"], 1)
                control = state_data["accounts"]["codex"]
                self.assertTrue(control["held"])
                self.assertEqual(control["windows"][0]["as_of"], rows[-1]["ts"])
                reserve = subprocess.run([sys.executable, "-I", "-c",
                    "import sys;sys.path.insert(0,sys.argv[1]);import pace_sampler;from pathlib import Path;print(pace_sampler.reserve(Path(sys.argv[2]),'codex','codex'))",
                    str(HERE), str(state / "usage-gate.json")], env=env, capture_output=True, text=True, timeout=10)
                self.assertEqual(reserve.returncode, 0, reserve.stderr)
                self.assertIn("old; needs <=120s", reserve.stdout)
                self.assertEqual(errors, [])
            finally:
                stopped.set()
                listener.close()
                server.join(timeout=2)
                self.assertFalse(server.is_alive())

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
                self.assertTrue(under_line["held"])
                self.assertIn("> 100%", pace_control.refusal("codex", under_line, now))
                above_line = pace_control.advance(previous, [native_week | {"used_pct": 24}], 5, now, target=target)
                self.assertTrue(above_line["held"])
                over = pace_control.advance(control, [window | {"used_pct": 99}], 2, now + 60, target=target)
                self.assertTrue(over["held"])
                self.assertIn("99.0%", pace_control.refusal("codex", over, now + 60))
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

    def test_pace_delivery_publishes_canonical_addressed_events(self):
        with tempfile.TemporaryDirectory() as scratch:
            previous = dict(os.environ)
            try:
                root = Path(scratch)
                (root / "bin").mkdir()
                ssh = root / "bin/ssh"
                ssh.write_text("#!/bin/sh\nprintf alert >>\"$ALERT_RECEIPT\"\n")
                ssh.chmod(0o700)
                directory = root / "agents"
                directory.mkdir()
                status = directory / "handoffs/lead-status.md"
                registry = directory / "org.json"
                registry.write_text(json.dumps({"version": 1, "nodes": [{"id": "lead", "provider": "codex", "state": "active", "status_file": str(status)}]}))
                os.environ.update(THREADS_DB=str(root / "events.db"), XDG_STATE_HOME=str(root),
                                  AGENTS_ORG_FILE=str(registry), ALERT_RECEIPT=str(root / "alert"),
                                  PATH=f"{root / 'bin'}:{os.environ['PATH']}")
                control = {"at": 42, "held": True, "allowed_workers": 1, "windows": []}
                pace_sampler.deliver("codex", control)
                from events import Store
                pending = Store().pending()
                self.assertEqual([(row["kind"], row["to_node"]) for row in pending], [("KICKOFF", "proxy"), ("KICKOFF", "lead")])
                self.assertTrue(all(row["body"].startswith("HOLD-SPAWNS codex allowed=1") for row in pending))
                self.assertIn("HOLD-SPAWNS", status.with_name("lead-inbox.md").read_text())
                self.assertEqual((root / "alert").read_text(), "alert")
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_account_delivery_preserves_proxy_grants_and_sampler_prunes_departed_leads(self):
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            org = state / "org.json"
            names = ("holder", "ordinary", "scoped", "exhausted", "foreign", "removed", "replaced", "finished", "provider", "billed", "billed-live")
            nodes = [{"id": name, "session": name, "provider": "codex", "state": "active",
                      "status_file": str(root / (name + "-status.md"))} for name in names]
            for node in nodes:
                if node["id"].startswith("billed"):
                    node.update(provider="claude", account="c")
            org.write_text(json.dumps({"nodes": nodes}))
            now = time.time()
            data = {"accounts": {}, "history": [],
                    "providers": {p: {"next_at": now + 86400} for p in ("claude", "codex", "gemini")}}
            pace_sampler.save(state / "pace-control.json", data)
            previous = dict(os.environ)
            try:
                os.environ.update(HOME=scratch, XDG_STATE_HOME=scratch, AGENTS_ORG_FILE=str(org),
                                  THREADS_DB=str(root / "usage.db"), AGENTS_USAGE_REMOTE="",
                                  AGENTS_ORCHESTRATION=str(root / "policy.toml"))
                (root / "policy.toml").write_text("")
                expiry = datetime.fromtimestamp(now + 3600, timezone.utc).isoformat()
                for name in names:
                    if name == "ordinary":
                        continue
                    command = [str(HERE.parents[1] / "bin/agents"), "pace", "grant",
                               "claude:c" if name.startswith("billed") else "codex", "9", "--until", expiry,
                               "--for", name, "--reason", "proxy approved scenario"]
                    if name == "scoped":
                        command += ["--task", "review"]
                    result = subprocess.run(command, capture_output=True, text=True, timeout=5)
                    self.assertEqual(result.returncode, 0, result.stderr)
                data = pace_sampler.load(state / "pace-control.json", {})
                for grant in data["grants"]:
                    if grant["lead"] == "holder":
                        grant["pending"] = [now]
                    if grant["lead"] == "exhausted":
                        grant["uses"] = 0
                    if grant["lead"] == "foreign":
                        grant["authority"] = "lead"
                pace_sampler.save(state / "pace-control.json", data)
                nodes = [n for n in nodes if n["id"] != "removed"]
                for node in nodes:
                    if node["id"] == "replaced":
                        node["session"] = "replacement"
                    if node["id"] == "finished":
                        node["state"] = "finished"
                    if node["id"] == "provider":
                        node["provider"] = "claude"
                    if node["id"] == "billed":
                        node.pop("account")
                org.write_text(json.dumps({"nodes": nodes}))
                window = {"window": "week", "used_pct": 35, "burn_pct_h": 2, "reset_h": 100,
                          "projected_pct": 235}
                control = {"held": True, "allowed_workers": 1, "windows": [window]}
                sent = []
                with patch.object(pace_sampler, "send_codex", side_effect=lambda session, line: (sent.append((session, line)) or (0, "", ""))), patch.object(pace_sampler, "run", return_value=(0, "", "")):
                    pace_sampler.deliver("codex", control)
                    self.assertNotIn("holder", [session for session, _ in sent])
                    self.assertFalse((root / "holder-inbox.md").exists())
                    for name in ("ordinary", "scoped", "exhausted", "foreign", "replacement"):
                        line = next(line for session, line in sent if session == name)
                        self.assertIn("HOLD-SPAWNS codex allowed=1", line)
                        self.assertIn("used=35.0%", line)
                    pace_sampler.deliver("claude:c", control)
                    self.assertIn("HOLD-SPAWNS claude:c", (root / "billed-inbox.md").read_text())
                    self.assertFalse((root / "billed-live-inbox.md").exists())
                    self.assertIn("INCIDENT burn HOLD-SPAWNS", (state / "handoffs/proxy-inbox.md").read_text())
                    sent.clear()
                    pace_sampler.deliver("codex", control | {"held": False})
                    self.assertNotIn("holder", [session for session, _ in sent])
                    self.assertIn("RESUME-SPAWNS", next(line for session, line in sent if session == "ordinary"))
                    data["grants"][0]["until"] = now - 1
                    pace_sampler.save(state / "pace-control.json", data)
                    sent.clear()
                    pace_sampler.deliver("codex", control)
                    self.assertIn("holder", [session for session, _ in sent])
                data["grants"][0]["until"] = now + 3600
                pace_sampler.save(state / "pace-control.json", data)
                result = subprocess.run([sys.executable, "-I", str(HERE / "pace_sampler.py"), "--dry-run"],
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                remaining = pace_sampler.load(state / "pace-control.json", {})["grants"]
                self.assertEqual(remaining, [g for g in data["grants"] if g["lead"] in ("holder", "scoped", "exhausted", "foreign", "billed-live")])
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

    def hold(self, path, seconds):
        code = "import fcntl,sys,time;f=open(sys.argv[1],'a');fcntl.flock(f,fcntl.LOCK_EX);print('held',flush=True);time.sleep(float(sys.argv[2]))"
        holder = subprocess.Popen([sys.executable, "-c", code, str(path), str(seconds)], stdout=subprocess.PIPE, text=True)
        self.addCleanup(holder.wait)
        self.addCleanup(holder.kill)
        self.assertEqual(holder.stdout.readline().strip(), "held")
        return holder

    def gate(self, scratch):
        path = Path(scratch) / "usage-gate.json"
        control = {"at": time.time(), "held": False, "workers": 0, "allowed_workers": 2, "windows": []}
        path.write_text(json.dumps({"codex": {"controls": {"codex": control}}}))
        return path

    def test_admission_proceeds_while_a_sampler_reads_usage(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = self.gate(scratch)
            self.hold(Path(scratch) / "pace-sampling.lock", 5)
            started = time.monotonic()
            self.assertIsNone(pace_sampler.reserve(path, "codex", "codex"))
            self.assertLess(time.monotonic() - started, 1)

    def test_admission_waits_out_a_short_commit_and_refuses_a_stuck_one(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = self.gate(scratch)
            self.hold(Path(scratch) / "pace-control.lock", 0.5)
            self.assertIsNone(pace_sampler.reserve(path, "codex", "codex", wait=4))
            self.hold(Path(scratch) / "pace-control.lock", 5)
            self.assertIn("stayed locked", pace_sampler.reserve(path, "codex", "codex", wait=0.3))
            self.assertEqual(json.loads(path.read_text())["codex"]["controls"]["codex"]["workers"], 1)

    def test_sampler_reads_usage_without_holding_the_admission_lock(self):
        probes = []
        class Stop(Exception):
            pass
        def probe(_path):
            with (pace_sampler.state_dir() / "pace-control.lock").open("a") as lock:
                probes.append(pace_sampler.locked(lock, 0))
            raise Stop()
        with tempfile.TemporaryDirectory() as scratch:
            old_env, old_read = dict(os.environ), pace_sampler.read_adapter
            try:
                os.environ["XDG_STATE_HOME"] = scratch
                pace_sampler.read_adapter = probe
                with self.assertRaises(Stop):
                    pace_sampler.sample("codex", notify=False)
            finally:
                pace_sampler.read_adapter = old_read
                os.environ.clear()
                os.environ.update(old_env)
        self.assertEqual(probes, [True])

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
            new = pace_control.advance(old, window(used + delta, "second"), count, 660)
            self.assertAlmostEqual(new["worker_minutes"], count * 10)
            self.assertAlmostEqual(new["burn_pct_worker_min"]["week"], delta / (count * 10))
            if used + delta + delta * 6 * hours > 100:
                self.assertTrue(new["held"])
                self.assertEqual(new["allowed_workers"], 1)
            elif used + delta + delta / (count * 10) * 60 * 3 * hours <= 90:
                self.assertEqual(new["allowed_workers"], 3)
            else:
                self.assertEqual(new["allowed_workers"], 2)
            repeated = pace_control.advance(new, window(used + delta, "second"), count, 670)
            self.assertEqual(repeated["allowed_workers"], new["allowed_workers"])
            missing = pace_control.advance(new, window(used + delta, "third"), None, 1300)
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

    def test_periodic_sampler_updates_advisory_without_workers_or_changing_ceiling(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            now = time.time()
            stamp = lambda at: datetime.fromtimestamp(at, timezone.utc).isoformat()
            rows = [dict(ts=stamp(now - ago), provider="codex", account="main", kind="window", name="week",
                         used_pct=10, window_min=10080, resets_at=stamp(now + 86400)) for ago in (600, 0)]
            canonical = {"allowed_workers": 1, "held": False}
            control = {"accounts": {"codex": canonical}, "history": rows,
                       "providers": {p: {"next_at": now + 86400} for p in ("claude", "codex", "gemini")}}
            (state / "pace-control.json").write_text(json.dumps(control))
            policy = root / "policy.toml"
            policy.write_text('[burn]\ncodex = { until = "2099-01-01T00:00:00Z", rate = "13%/h" }\n')
            env = dict(os.environ, XDG_STATE_HOME=scratch, THREADS_DB=str(root / "usage.db"),
                       AGENTS_ORCHESTRATION=str(policy), AGENTS_USAGE_REMOTE="")
            run = subprocess.run([sys.executable, "-I", str(HERE / "pace_sampler.py"), "--dry-run"],
                                 env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(run.returncode, 0, run.stderr)
            self.assertIn("parallel 2 -> 3; measured 0.00%/h", run.stdout)
            self.assertEqual(json.loads((state / "burn.json").read_text())["codex"]["parallel"], 3)
            self.assertEqual(json.loads((state / "pace-control.json").read_text())["accounts"]["codex"], canonical)

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
