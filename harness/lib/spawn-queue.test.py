#!/usr/bin/env python3
import importlib.util
import json
import os
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
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


class DurableLaunch(unittest.TestCase):
    def setUp(self):
        import spawn
        self.spawn = spawn
        self.scratch = Scratch(self)
        self.addCleanup(self.scratch.close)
        self.state = self.scratch.state
        self.decision = {"kind": "launch", "provider": "codex", "account": "codex",
                         "tier": "codex:sol-high", "model": "gpt-6.1-sol", "effort": "high"}
        self.enterContext(patch.object(spawn.placement, "place_spawn", return_value=self.decision, create=True))
        self.original_launch = spawn.org.launch
        self.launch = self.enterContext(patch.object(spawn.org, "launch", return_value="worker-session"))
        with spawn.org.Org() as registry:
            spawn.org.add(registry, "parent", {"role": "lead", "session": "parent-session"})
        os.environ.update(AGENT_ORG_NAME="parent", CLAUDE_CODE_SESSION_ID="parent-session")

    def queued(self, ident, parent="parent"):
        job = {"spawn_id": ident, "parent": parent, "session": "parent-session", "provider": "codex",
               "account": "codex", "tier": "sol-high", "cwd": str(self.scratch.root)}
        directory = self.state / "spawn"
        directory.mkdir(exist_ok=True)
        (directory / f"{ident}.md").write_text(f"Item: north#{ident}\nDo the work.\n")
        self.spawn.spawn_queue.enqueue_spawn(self.state, job, {"reason": "capacity"})
        return job

    def test_interrupted_launch_keeps_work_queued_until_one_worker_starts(self):
        spawn = self.spawn
        job = self.queued("111111111111")
        def interrupted(node, *args, **kwargs):
            with spawn.org.Org() as registry:
                claim = registry.need(node["id"])
                self.assertEqual(claim["state"], "launching")
                self.assertEqual(claim["spawn_id"], job["spawn_id"])
                self.assertFalse(claim["session"])
            self.assertEqual(spawn.main([job["spawn_id"]]), 0)
            self.assertEqual(len(spawn.spawn_queue.read(self.state, 0)), 1)
            raise OSError("launcher unavailable")
        self.launch.side_effect = interrupted
        self.assertEqual(spawn.release(), 0)
        self.assertEqual([row["spawn_id"] for row in spawn.spawn_queue.read(self.state, 0)], [job["spawn_id"]])
        with spawn.org.Org() as registry:
            self.assertEqual(registry.need("spawn-" + job["spawn_id"])["state"], "failed")
        self.launch.side_effect = None
        self.assertEqual(spawn.release(), 0)
        self.assertEqual(spawn.release(), 0)
        self.assertEqual(self.launch.call_count, 2)
        self.assertEqual(spawn.spawn_queue.read(self.state, 0), [])
        stale = self.queued("222222222222")
        with spawn.org.Org() as registry:
            node = spawn.org.add(registry, "spawn-" + stale["spawn_id"], {"role": "worker", "parent": "parent"})
            node.update(state="launching", spawn_id=stale["spawn_id"], launch_deadline=time.time() - 10)
            registry.save()
        with patch.object(spawn.subprocess, "run", return_value=type("Result", (), {"returncode": 1})()):
            self.assertEqual(spawn.release(), 0)
        self.assertEqual(self.launch.call_count, 3)
        self.assertEqual(spawn.spawn_queue.read(self.state, 0), [])

    def test_started_spawn_cannot_relaunch_after_finish_and_org_cleanup(self):
        spawn = self.spawn
        job = self.queued("333333333333")
        self.assertEqual(spawn.release(), 0)
        with spawn.org.Org() as registry:
            node = registry.need("spawn-" + job["spawn_id"])
            node["state"] = "finished"
            registry.save()
        self.assertEqual(spawn.main([job["spawn_id"]]), 0)
        with spawn.org.Org() as registry:
            spawn.org.remove(registry, "spawn-" + job["spawn_id"])
        spawn.spawn_queue.enqueue_spawn(self.state, job, {"reason": "queue removal interrupted"})
        self.assertEqual(spawn.main([job["spawn_id"]]), 0)
        spawn.spawn_queue.enqueue_spawn(self.state, job, {"reason": "queue removal interrupted"})
        self.assertEqual(spawn.release(), 0)
        self.assertEqual(self.launch.call_count, 1)
        self.assertEqual(spawn.spawn_queue.read(self.state, 0), [])
        self.assertEqual((self.state / "spawn-receipts").read_text().splitlines(), [job["spawn_id"]])

    def test_bad_queued_jobs_do_not_block_newer_work_or_the_sampler(self):
        import events
        spawn = self.spawn
        orphan = self.queued("444444444444", parent="removed-parent")
        missing = self.queued("555555555555")
        broken = self.queued("666666666666")
        newer = self.queued("777777777777")
        def place(text, job, *args):
            if job["spawn_id"] == broken["spawn_id"]:
                raise RuntimeError("bad placement")
            return dict(self.decision, provider="claude", account="claude:missing") if job["spawn_id"] == missing["spawn_id"] else self.decision
        def launch(node, *args, **kwargs):
            if node["provider"] == "claude":
                return self.original_launch(node, *args, **kwargs)
            return "newer-session"
        self.launch.side_effect = launch
        self.enterContext(patch.object(spawn.pace_sampler.usage, "DB", str(self.scratch.root / "usage.db")))
        self.enterContext(patch.object(spawn.pace_sampler.usage, "REMOTE", False))
        with patch.object(spawn.placement, "place_spawn", side_effect=place), patch.object(events.Store, "publish") as publish:
            spawn.pace_sampler.sample("fixture-no-adapters", notify=False)
            self.assertEqual([row["spawn_id"] for row in spawn.spawn_queue.read(self.state, 0)], [missing["spawn_id"], broken["spawn_id"]])
            with spawn.org.Org() as registry:
                self.assertEqual(registry.need("spawn-" + newer["spawn_id"])["session"], "newer-session")
            self.assertEqual(publish.call_count, 1)
            self.assertEqual(publish.call_args.args[0], "STUCK")
            self.assertEqual(publish.call_args.kwargs["to_node"], "proxy")
            self.assertIn(orphan["spawn_id"], publish.call_args.kwargs["body"])
            spawn.pace_sampler.sample("fixture-no-adapters", notify=False)
            self.assertEqual(publish.call_count, 1)


if __name__ == "__main__":
    unittest.main()
