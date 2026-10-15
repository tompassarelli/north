#!/usr/bin/env python3
import fcntl
import hashlib
import importlib.util
import json
import os
import sys
import subprocess
import tempfile
import threading
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
                          THREADS_DB=str(self.root / "threads.db"),
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
        self.launch = self.enterContext(patch.object(spawn.org, "launch", side_effect=self.worker_started))
        with spawn.org.Org() as registry:
            spawn.org.add(registry, "parent", {"role": "lead", "session": "parent-session"})
        os.environ.update(AGENT_ORG_NAME="parent", CLAUDE_CODE_SESSION_ID="parent-session")

    def worker_started(self, node, *args, **kwargs):
        subprocess.run([sys.executable, str(HERE / "spawn.py"), "started", os.environ["AGENT_SPAWN_CLAIM"]], check=True)
        return "worker-session"

    def queued(self, ident, parent="parent"):
        job = {"spawn_id": ident, "parent": parent, "session": "parent-session", "provider": "codex",
               "account": "codex", "tier": "sol-high", "cwd": str(self.scratch.root)}
        directory = self.state / "spawn"
        directory.mkdir(exist_ok=True)
        (directory / f"{ident}.md").write_text(f"Item: north#{ident}\nDo the work.\n")
        self.spawn.spawn_queue.enqueue_spawn(self.state, job, {"reason": "capacity"})
        return job

    def test_manual_retry_during_slow_launch_starts_only_one_worker(self):
        spawn = self.spawn
        job = self.queued("111111111111")
        launches = self.scratch.root / "launches"
        launches.write_text("launch\n")
        def slow(node, *args, **kwargs):
            with spawn.org.Org() as registry:
                registry.need(node["id"])["launch_deadline"] = time.time() - 10
                registry.save()
            retry = """
import spawn
import sys
from pathlib import Path
def duplicate(*args, **kwargs):
    with Path(sys.argv[2]).open("a") as target:
        target.write("duplicate\\n")
    return "duplicate-session"
spawn.org.launch = duplicate
spawn.main([sys.argv[1]])
"""
            result = subprocess.run([sys.executable, "-c", retry, job["spawn_id"], str(launches)],
                                    env=dict(os.environ, PYTHONPATH=str(HERE)), capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("already active", result.stdout)
            self.assertEqual(len(spawn.spawn_queue.read(self.state, 0)), 1)
            return self.worker_started(node)
        self.launch.side_effect = slow
        self.assertEqual(spawn.release(), 0)
        self.assertEqual(launches.read_text().splitlines(), ["launch"])
        self.assertEqual(self.launch.call_count, 1)
        self.assertEqual(spawn.spawn_queue.read(self.state, 0), [])

    def test_worker_started_mark_prevents_relaunch_after_launcher_crash_and_org_cleanup(self):
        spawn = self.spawn
        job = self.queued("333333333333")
        class LauncherCrash(BaseException):
            pass
        def crash(node, *args, **kwargs):
            self.worker_started(node)
            with spawn.org.Org() as registry:
                current = registry.need(node["id"])
                current["state"] = "finished"
                registry.save()
                spawn.org.remove(registry, node["id"])
            raise LauncherCrash()
        self.launch.side_effect = crash
        with self.assertRaises(LauncherCrash):
            spawn.release()
        path = spawn.claim_path(self.state, job["spawn_id"])
        owner = json.loads(path.read_text().splitlines()[0])
        owner["start"] = "dead-owner"
        path.write_text(json.dumps(owner) + "\n" + "\n".join(path.read_text().splitlines()[1:]) + "\n")
        self.assertIn("started", path.read_text().splitlines())
        self.launch.side_effect = self.worker_started
        self.assertEqual(spawn.main([job["spawn_id"]]), 0)
        spawn.spawn_queue.enqueue_spawn(self.state, job, {"reason": "queue removal interrupted"})
        self.assertEqual(spawn.release(), 0)
        self.assertEqual(self.launch.call_count, 1)
        self.assertEqual(spawn.spawn_queue.read(self.state, 0), [])
        self.assertIn("started", path.read_text().splitlines())

    def test_release_deadline_leaves_claim_intact_when_org_lock_is_held(self):
        spawn = self.spawn
        job = self.queued("888888888888")
        holder = (self.state / "org.json.lock").open("a")
        self.addCleanup(holder.close)
        def interrupted(node, *args, **kwargs):
            fcntl.flock(holder, fcntl.LOCK_EX | fcntl.LOCK_NB)
            unlock = threading.Timer(0.6, fcntl.flock, args=(holder, fcntl.LOCK_UN))
            unlock.start()
            self.addCleanup(unlock.join)
            time.sleep(1)
        self.launch.side_effect = interrupted
        began = time.monotonic()
        self.assertEqual(spawn.release(wait=0.1), 2)
        self.assertLess(time.monotonic() - began, 0.2)
        path = spawn.claim_path(self.state, job["spawn_id"])
        self.assertTrue(path.exists())
        self.assertNotIn("failed", path.read_text().splitlines())
        self.assertEqual(len(spawn.spawn_queue.read(self.state, 0)), 1)
        began = time.monotonic()
        self.assertEqual(spawn.release(wait=0.1), 2)
        self.assertLess(time.monotonic() - began, 0.2)
        self.assertEqual(self.launch.call_count, 1)
        fcntl.flock(holder, fcntl.LOCK_UN)

    def test_dead_unstarted_owner_queues_once_and_reports_to_proxy(self):
        import events
        spawn = self.spawn
        job = self.queued("999999999999")
        self.launch.side_effect = OSError("launcher unavailable")
        self.assertEqual(spawn.release(), 0)
        path = spawn.claim_path(self.state, job["spawn_id"])
        owner = json.loads(path.read_text().splitlines()[0])
        owner["start"] = "dead-owner"
        path.write_text(json.dumps(owner) + "\n")
        spawn.spawn_queue.admit(self.state, "codex", job["spawn_id"])
        with patch.object(events.Store, "publish", wraps=events.Store().publish) as publish, patch.object(spawn.spawn_queue, "enqueue_spawn", wraps=spawn.spawn_queue.enqueue_spawn) as enqueue:
            self.assertEqual(spawn.main([job["spawn_id"]]), 0)
            self.assertEqual(spawn.release(), 0)
            self.assertEqual(spawn.release(), 0)
            self.assertEqual(enqueue.call_count, 1)
            self.assertEqual(publish.call_count, 1)
            self.assertEqual(publish.call_args.kwargs["to_node"], "proxy")
        self.assertEqual(self.launch.call_count, 1)
        self.assertEqual(len(spawn.spawn_queue.read(self.state, 0)), 1)
        self.assertIn("failed", path.read_text().splitlines())

    def test_launch_wrappers_mark_started_before_executing_the_brief(self):
        spawn = self.spawn
        stubs = self.scratch.root / "bin"
        stubs.mkdir()
        pooled = self.scratch.root / "pooled"
        (pooled / "sqlite").mkdir(parents=True)
        key = hashlib.sha256(f"{pooled}\n{pooled}/sqlite".encode()).hexdigest()[:16]
        endpoint = f"unix://{os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')}/codex-shared-{key}/app-server.sock"
        owner = subprocess.Popen([sys.executable, "-c", "import signal; signal.pause()", "app-server", "--listen", endpoint])
        self.addCleanup(owner.wait)
        self.addCleanup(owner.terminate)
        commands = {
            "agents": '#!/bin/sh\n[ "${2:-}" != --burn ]\n',
            "sqlite3": '#!/bin/sh\necho fixture-session\n',
            "ss": f'#!/bin/sh\necho \'/tmp/codex-daemon-{os.getuid()}/abcdef users:(("codex",pid={owner.pid},fd=31))\'\n',
            "websocat": '#!/bin/sh\ncat >/dev/null\necho \'{"id":2,"result":{}}\'\n',
            "spawn-quiet": '#!/bin/sh\nwhile [ "$1" != -e ]; do shift; done\nshift\nexec "$@"\n',
            "claude": '#!/bin/sh\ngrep -qx started "$CLAIM_PROBE" || exit 1\n[ -z "${AGENT_SPAWN_CLAIM:-}" ] || exit 1\necho claude >>"$WORKER_PROBE"\n',
            "codex": '#!/bin/sh\ngrep -qx started "$CLAIM_PROBE" || exit 1\n[ -z "${AGENT_SPAWN_CLAIM:-}" ] || exit 1\necho codex >>"$WORKER_PROBE"\n',
        }
        for name, script in commands.items():
            path = stubs / name
            path.write_text(script)
            path.chmod(0o755)
        observed = self.scratch.root / "workers"
        self.launch.side_effect = self.original_launch
        with patch.dict(os.environ, PATH=f"{stubs}:{HERE.parents[1] / 'bin'}:{os.environ['PATH']}",
                        AGENTS_SESSION_HOST="ghostty", CODEX_LEAD_COACH="0", WORKER_PROBE=str(observed),
                        NORTH_CODEX_POOLED_HOME=str(pooled), NORTH_CODEX_CONVERSATION_HOME=str(pooled),
                        NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(pooled / "sqlite")), \
             patch.object(spawn.org, "selected_profile", return_value=None), patch.object(spawn.org, "trust"):
            for provider, ident in (("claude", "aaaaaaaaaaaa"), ("codex", "bbbbbbbbbbbb")):
                with self.subTest(provider=provider):
                    job = self.queued(ident)
                    os.environ["CLAIM_PROBE"] = str(spawn.claim_path(self.state, ident))
                    decision = dict(self.decision, provider=provider)
                    self.assertEqual(spawn.start(self.state, job, decision, unattended=True), 0)
                    spawn.spawn_queue.admit(self.state, "codex", ident)
                    self.assertIn("started", spawn.claim_path(self.state, ident).read_text().splitlines())
        self.assertEqual(observed.read_text().splitlines(), ["claude", "codex"])

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
            self.worker_started(node)
            return "newer-session"
        self.launch.side_effect = launch
        self.enterContext(patch.object(spawn.pace_sampler.usage, "DB", str(self.scratch.root / "usage.db")))
        self.enterContext(patch.object(spawn.pace_sampler.usage, "REMOTE", False))
        with patch.object(spawn.placement, "place_spawn", side_effect=place), patch.object(events.Store, "publish") as publish:
            spawn.pace_sampler.sample("fixture-no-adapters", notify=False)
            self.assertEqual([row["spawn_id"] for row in spawn.spawn_queue.read(self.state, 0)], [missing["spawn_id"], broken["spawn_id"]])
            with spawn.org.Org() as registry:
                self.assertEqual(registry.need("spawn-" + newer["spawn_id"])["session"], "newer-session")
                failed = registry.need("spawn-" + missing["spawn_id"])
                self.assertEqual(failed["state"], "failed")
                self.assertTrue(failed.get("died"))
            self.assertEqual(publish.call_count, 1)
            self.assertEqual(publish.call_args.args[0], "STUCK")
            self.assertEqual(publish.call_args.kwargs["to_node"], "proxy")
            self.assertIn(orphan["spawn_id"], publish.call_args.kwargs["body"])
            spawn.pace_sampler.sample("fixture-no-adapters", notify=False)
            self.assertEqual(publish.call_count, 1)


if __name__ == "__main__":
    unittest.main()
