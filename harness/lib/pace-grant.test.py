#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pace_sampler


class Grants(unittest.TestCase):
    def test_proxy_grant_journey_preserves_scope_ceiling_money_and_expiry(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / ".local/state/agents"
            state.mkdir(parents=True)
            org = state / "org.json"
            org.write_text(json.dumps({"nodes": [{"id": "lead", "session": "parent", "state": "active", "provider": "codex"}]}))
            control = {"at": time.time() - 600, "held": True, "workers": 0, "allowed_workers": 1, "windows": []}
            (state / "pace-control.json").write_text(json.dumps({"accounts": {"codex": control}}))
            gate = state / "usage-gate.json"
            verdict = {"epoch": time.time() - 600, "stale_min": 30, "pace_control": True,
                       "refusal": None, "controls": {"codex": control}}
            gate.write_text(json.dumps({"codex": verdict}))
            probe = root / "probe.json"
            probe.write_text(json.dumps({"committedBatchCpus": 0, "aggregateCpuLimit": 20, "protectedCpuSomeAvg10": 0}))
            previous = dict(os.environ)
            try:
                os.environ.update(HOME=str(root), XDG_STATE_HOME=str(root / ".local/state"),
                                  AGENTS_ORG_FILE=str(org), SPAWN_CAPACITY_HOME=str(root),
                                  SPAWN_CAPACITY_STATUS=str(probe), CODEX_BEHAVIOR_STATE=str(root / "behavior"))
                os.environ.pop("AGENT_ORG_NAME", None)
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                result = subprocess.run([str(HERE.parents[1] / "bin/agents"), "pace", "grant", "codex", "2",
                                         "--until", expiry, "--for", "lead", "--reason", "proxy approved playtest"],
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["authority"], "proxy")
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "unrelated"))
                self.assertIsNone(pace_sampler.grant_for(state, "claude:c", "parent"))
                spec = importlib.util.spec_from_file_location("grant_capacity", HERE.parent / "hooks/lib/spawn_capacity.py")
                capacity = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(capacity)
                event = {"tool_name": "collaborationspawn_agent", "session_id": "parent", "cwd": str(root),
                         "tool_input": {"task_name": "playtest", "message": "[routine:watchdog]"}}
                self.assertIsNone(capacity.evaluate(event)[0])
                self.assertIsNone(capacity.evaluate(event)[0])
                self.assertIn("2/2", capacity.evaluate(event)[0])
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent", time.time() + 3601))
                fresh = control | {"at": time.time(), "windows": [{"window": "week", "age_s": 0,
                    "as_of": datetime.now(timezone.utc).isoformat(), "used_pct": 32, "burn_pct_h": 0,
                    "reset_h": 65, "projected_pct": 32, "resets_at": "reset"}]}
                gate.write_text(json.dumps({"codex": verdict | {"epoch": time.time(), "controls": {"codex": fresh},
                    "refusal": "No: a new codex worker could spend money: fixture."}}))
                self.assertIn("could spend money", capacity.evaluate(event)[0])
                gate.write_text(json.dumps({"codex": verdict | {"epoch": time.time(), "controls": {"codex": fresh}}}))
                pace_sampler.grant("codex", 3, expiry, "lead", "proxy one reviewer", "review", 1)
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent", task="playtest"))
                self.assertIsNone(pace_sampler.reserve(gate, "codex", "codex", session="parent", task="review"))
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent", task="review"))
                data = json.loads((state / "pace-control.json").read_text())
                data["grants"][0]["until"] = time.time() - 1
                (state / "pace-control.json").write_text(json.dumps(data))
                self.assertIsNotNone(capacity.evaluate(event)[0])
                org.write_text(json.dumps({"nodes": [{"id": "lead", "session": "replacement", "state": "active", "provider": "codex"}]}))
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent"))
                rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                self.assertEqual(sum(r["event"] == "grant-admission" for r in rows), 3)
            finally:
                os.environ.clear()
                os.environ.update(previous)


if __name__ == "__main__":
    unittest.main()
