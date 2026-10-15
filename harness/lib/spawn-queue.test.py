#!/usr/bin/env python3
import importlib.util
import json
import os
import sys
import tempfile
import time
import unittest
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))



class Scratch:
    def __init__(self, test):
        self.test = test
        self.dir = tempfile.TemporaryDirectory()
        self.root = Path(self.dir.name)
        self.state = self.root / ".local/state/agents"
        self.state.mkdir(parents=True)
        self.probe = self.root / "probe.json"
        self.policy = self.root / "orchestration.toml"
        self.policy.write_text("")
        self.previous = dict(os.environ)
        for name in ("AGENT_ORG_NAME", "AGENT_DELEGATION_BUDGET", "CLAUDE_CODE_ENTRYPOINT"):
            os.environ.pop(name, None)
        os.environ.update(HOME=str(self.root), XDG_STATE_HOME=str(self.root / ".local/state"),
                          SPAWN_CAPACITY_HOME=str(self.root), SPAWN_CAPACITY_STATUS=str(self.probe),
                          AGENTS_ORG_FILE=str(self.state / "org.json"), AGENTS_ORCHESTRATION=str(self.policy),
                          CLAUDE_CONFIG_DIR=str(self.root / ".claude"), CODEX_BEHAVIOR_STATE=str(self.root / "behavior"),
                          AGENT_ROLE="lead")
        self.cpus(0)
        spec = importlib.util.spec_from_file_location("queue_capacity", HERE.parent / "hooks/lib/spawn_capacity.py")
        self.capacity = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.capacity)

    def cpus(self, committed, holding=()):
        self.probe.write_text(json.dumps({"committedBatchCpus": committed, "aggregateCpuLimit": 20,
                                          "protectedCpuSomeAvg10": 0, "holding": list(holding)}))

    def gate(self, provider, verdict):
        (self.state / "usage-gate.json").write_text(json.dumps({provider: {"epoch": time.time(), "stale_min": 30,
                                                                           "refusal": None, **verdict}}))

    def close(self):
        os.environ.clear()
        os.environ.update(self.previous)
        self.dir.cleanup()


def agent(description, session="lead-session", **extra):
    return {"tool_name": "Agent", "session_id": session, "cwd": "/tmp",
            "tool_input": {"subagent_type": "worker", "description": description, "prompt": "do it"}, **extra}


def held_control(used=50.0, resets_at="2026-10-20T00:00:00+00:00"):
    window = {"window": "week", "as_of": datetime.now(timezone.utc).isoformat(), "used_pct": used,
              "burn_pct_h": 0, "reset_h": 24, "resets_at": resets_at, "projected_pct": used}
    return {"at": time.time(), "held": True, "workers": 1, "allowed_workers": 1, "windows": [window]}


class Queue(unittest.TestCase):
    def setUp(self):
        self.scratch = Scratch(self)
        self.addCleanup(self.scratch.close)
        self.scratch.gate("claude", {})

    def enqueue(self, request, eta=None, lane="claude/unassigned"):
        import spawn_queue
        return spawn_queue.enqueue(self.scratch.state, lane, request, eta or time.time() + 60, "capacity")

    def test_freed_capacity_admits_the_oldest_waiting_request_first(self):
        import spawn_queue
        state, lane = self.scratch.state, "claude/unassigned"
        self.assertEqual(self.enqueue("first")[0], 1)
        self.assertEqual(self.enqueue("second")[0], 2)
        self.assertEqual([e["request"] for e in spawn_queue.ahead(state, lane, "third")], ["first", "second"])
        self.assertEqual(self.enqueue("third")[0], 3)
        self.assertEqual(self.enqueue("second")[0], 2)
        for request in ("first", "second", "third"):
            self.assertEqual(spawn_queue.ahead(state, lane, request), [])
            spawn_queue.admit(state, lane, request)
        self.assertEqual(json.loads((state / "spawn-queue.json").read_text())["requests"], [])

    def test_cpu_refusal_names_its_queue_position_and_the_next_lease_end(self):
        import spawn_queue
        s = self.scratch
        s.cpus(20, [{"kind": "run", "remainingSeconds": 900}, {"kind": "run", "remainingSeconds": 300},
                    {"kind": "agent", "remainingSeconds": 30}])
        event = agent("worker")
        reason = s.capacity.evaluate(event)[0]
        self.assertTrue(reason.startswith("Capacity leases and unleased heavy load commit 20 of the 20 CPUs"), reason)
        eta = s.capacity.refusal_eta(event, reason)
        notice = spawn_queue.notice(*self.enqueue("worker", eta))
        self.assertIn("Queued #1, ETA", notice)
        self.assertLess(abs(eta - (time.time() + 300)), 5)
        self.assertIn("wait until then and retry this same spawn", notice)

    def test_pace_refusal_names_its_queue_position_and_the_window_reset(self):
        import spawn_queue
        s = self.scratch
        s.gate("codex", {"pace_control": True, "controls": {"codex": held_control(99.0, "2026-10-20T06:00:00+00:00")}})
        event = {"tool_name": "spawn_agent", "session_id": "codex-lead", "cwd": "/tmp",
                 "tool_input": {"task_name": "fix", "message": "go"}}
        reason = s.capacity.evaluate(event)[0]
        self.assertTrue(reason.startswith("Hold: codex week used 99.0%"), reason)
        eta = s.capacity.refusal_eta(event, reason)
        self.assertIn("Queued #1, ETA 2026-10-20T06:00:00Z", spawn_queue.notice(*self.enqueue("fix", eta)))

    def test_exempt_and_delegation_refusals_do_not_queue(self):
        s = self.scratch
        s.cpus(20)
        os.environ["AGENT_DELEGATION_BUDGET"] = "0"
        reason = s.capacity.evaluate(agent("worker"))[0]
        self.assertTrue(reason.startswith("Your delegation budget is 0"), reason)
        self.assertNotIn("Queued", reason)
        del os.environ["AGENT_DELEGATION_BUDGET"]
        urgent = agent("urgent")
        urgent["tool_input"]["prompt"] = 'CASE=URGENT FACT="production is down for every user"'
        self.assertIsNone(s.capacity.evaluate(urgent)[0])
        self.assertFalse((s.state / "spawn-queue.json").exists())

    def test_a_lapsed_request_gives_up_its_place(self):
        import spawn_queue
        state = self.scratch.state
        self.enqueue("gone")
        data = json.loads((state / "spawn-queue.json").read_text())
        data["requests"][0].update(eta=time.time() - 3600, seen=time.time() - 3600)
        (state / "spawn-queue.json").write_text(json.dumps(data))
        self.assertEqual(spawn_queue.ahead(state, "claude/unassigned", "fresh"), [])


if __name__ == "__main__":
    unittest.main()
