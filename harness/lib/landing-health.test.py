#!/usr/bin/env python3
import json
import fcntl
import os
from contextlib import contextmanager, ExitStack
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import events
import landing_health


@contextmanager
def socket_recipient(directory, session):
    with subprocess.Popen([sys.executable, "-c",
                           "import sys; from pathlib import Path; print(Path('/proc/self/stat').read_text().split()[0], flush=True); sys.stdin.read()",
                           "--session-id", session, "--settings", json.dumps(events.INBOUND)],
                          stdin=subprocess.PIPE, stdout=subprocess.PIPE) as child:
        try:
            pid = child.stdout.readline().strip().decode()
            if not pid.isdecimal():
                raise RuntimeError("socket recipient exited before becoming ready")
            with socket.socket(socket.AF_UNIX) as peer:
                peer.bind(str(directory / f"{pid}.sock"))
                peer.listen(128)
                peer.settimeout(3)
                yield peer
        finally:
            child.kill()


class LandingHealth(unittest.TestCase):
    def lane_observation(self, directory, age, busy=False, update=False):
        now = int(time.time())
        source = directory / "source"
        repository = directory / "clone"
        source.mkdir()
        repository.mkdir()

        def git(path, *arguments, at=None):
            environment = dict(os.environ, GIT_COMMITTER_DATE=f"@{at or now - 7200} +0000",
                               GIT_AUTHOR_DATE=f"@{now - 7200} +0000")
            result = subprocess.run(["git", "-C", str(path), *arguments], env=environment,
                                    capture_output=True, text=True, check=True)
            return result.stdout.strip()

        git(source, "init", "-q", "-b", "main")
        git(source, "config", "user.name", "Lane fixture")
        git(source, "config", "user.email", "fixture@example.invalid")
        git(source, "-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-qm", "main")
        main = git(source, "rev-parse", "HEAD")
        git(source, "checkout", "-qb", "work")
        git(source, "-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-qm", "old lane commit")
        for index in range(4):
            git(source, "branch", f"claude/land-{index}")
        git(repository, "init", "-q")
        git(repository, "remote", "add", "origin", "https://github.com/tompassarelli/fixture.git")
        git(repository, "fetch", "-q", str(source), "+refs/heads/*:refs/remotes/origin/*", at=now - age)
        if update:
            git(source, "-c", "core.hooksPath=/dev/null", "commit", "--allow-empty", "-qm", "another old commit")
            for index in range(4):
                git(source, "branch", "-f", f"claude/land-{index}")
            git(repository, "fetch", "-q", str(source), "+refs/heads/*:refs/remotes/origin/*", at=now - 300)
        rows = [dict(ref=f"refs/heads/busy-{index}", before=main, after=main,
                     timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now - index)))
                for index in range(105)] if busy else []
        if busy:
            rows.append(dict(ref="refs/heads/main", before="0" * 40, after=main,
                             timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(now - 3500))))
        calls = []
        external = landing_health.command

        def command(argv, deadline=None):
            if argv[:2] != ["gh", "api"]:
                if argv[:4] == ["git", "-C", str(repository), "fetch"]:
                    argv = [str(source) if argument == "origin" else argument for argument in argv]
                return external(argv, deadline)
            calls.append(argv[2])
            selected = [row for row in rows if row["ref"] == "refs/heads/main"] if "&ref=refs/heads/main" in argv[2] else rows
            page = int(argv[2].split("page=")[-1].split("&")[0])
            return json.dumps(selected[(page - 1) * 100:page * 100])

        with patch.object(landing_health, "command", side_effect=command):
            observation = landing_health.sample(repository, {}, now, deadline=time.monotonic() + 10)
        self.assertEqual(len(calls), 1)
        self.assertIn("&ref=refs/heads/main", calls[0])
        self.assertEqual(len(observation["lanes"]), 4)
        self.assertEqual(observation["lane_age_fallbacks"], {})
        return observation, now

    def test_busy_pushes_leave_recently_fetched_old_commits_young(self):
        with tempfile.TemporaryDirectory() as temporary:
            observation, now = self.lane_observation(Path(temporary), 600, busy=True)
            self.assertEqual(set(observation["lanes"].values()), {now - 600})
            self.assertNotIn("LANDING-STALL", landing_health.anomalies(observation, now))
            self.assertEqual(len(observation["arrivals"]), 1)

    def test_old_first_fetch_stalls_without_main_arrivals_after_tip_update(self):
        with tempfile.TemporaryDirectory() as temporary:
            observation, now = self.lane_observation(Path(temporary), 2400, update=True)
            self.assertTrue(all(at <= now - 2400 for at in observation["lanes"].values()))
            self.assertEqual(observation["arrivals"], {})
            self.assertIn("LANDING-STALL", landing_health.anomalies(observation, now))

    def test_source_gap_reaches_only_proxy_once_while_delivery_waits(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            repository = directory / "repos/fixture/main"
            repository.mkdir(parents=True)
            for arguments in (("init", "-q"), ("remote", "add", "origin", "https://github.com/tompassarelli/fixture.git")):
                subprocess.run(["git", "-C", str(repository), *arguments], check=True, capture_output=True)
            registry = {f"lead-{index}": dict(id=f"lead-{index}", role="lead", domain="fixture", state="active", provider="claude")
                        for index in range(3)}

            def unavailable(*args, **kwargs):
                raise RuntimeError("repository activity exceeds five-page coverage budget")

            with patch.dict(os.environ, THREADS_DB=str(directory / "events.db"), AGENTS_ORG_FILE=str(directory / "org.json")):
                store = events.Store()
                deferred = []
                real_run = subprocess.run

                def defer_publish(argv, **kwargs):
                    if argv[:2] == [str(ROOT / "bin/threads"), "agent-event"]:
                        deferred.append(argv)
                        return subprocess.CompletedProcess(argv, 0, "", "")
                    return real_run(argv, **kwargs)

                with patch("subprocess.run", side_effect=defer_publish):
                    for _ in range(2):
                        landing_health.supervise(store, registry, directory, time.time(), root=directory / "repos", observe=unavailable)
                self.assertEqual(len(deferred), 2)
                self.assertEqual(deferred[0], deferred[1])
                self.assertEqual(store.pending(), [])
                for _ in range(2):
                    landing_health.supervise(store, registry, directory, time.time(), root=directory / "repos", observe=unavailable)
                    rows = store.pending()
                    self.assertEqual(len(rows), 1)
                    self.assertEqual(rows[0]["to_node"], "proxy")
                    self.assertEqual(rows[0]["key"], deferred[0][deferred[0].index("--key") + 1])
                    self.assertIsNone(rows[0]["delivered_at"])
                saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
                self.assertEqual(saved["episodes"]["LANDING-HEALTH-SOURCE"]["sent"], [])

    def test_recorded_supervisor_git_activity_and_inbox_journey(self):
        with tempfile.TemporaryDirectory() as temporary, ExitStack() as stack:
            directory = Path(temporary)
            sockets = directory / "sockets"
            sockets.mkdir()
            recipients = {name: stack.enter_context(socket_recipient(sockets, session)) for name, session in
                          (("fixture-lead", "recorded-native-recipient"), ("proxy", "recorded-proxy-recipient"))}
            repository = directory / "repos/fixture/main"
            repository.mkdir(parents=True)
            environment = dict(os.environ, AGENTS_REPO_ROOT=str(directory / "repos"),
                               AGENTS_ORG_FILE=str(directory / "org.json"),
                               AGENTS_CLAUDE_SOCKET_DIR=str(sockets),
                               THREADS_DB=str(directory / "events.db"), PYTHONDONTWRITEBYTECODE="1")
            real_git = shutil.which("git")

            def git(*arguments, at=None):
                result = subprocess.run([real_git, "-C", str(repository), *arguments],
                                        env=environment if at is None else dict(environment, GIT_COMMITTER_DATE=f"@{int(at)} +0000"),
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return result.stdout.strip()

            git("init", "-q")
            git("config", "user.name", "Recorded fixture")
            git("config", "user.email", "fixture@example.invalid")
            git("remote", "add", "origin", "https://github.com/tompassarelli/fixture.git")
            (repository / ".gitattributes").write_text("generated.tsv merge=fixture-generated\n")
            (repository / ".safe-push").write_text('[generated "declared.tsv"]\n input = primary.ts\n')
            (repository / "primary.ts").write_text("primary source\n")
            (repository / "declared.tsv").write_text("seed\n")
            (repository / "generated.tsv").write_text("seed\n")
            (repository / "arbitrary.txt").write_text("seed\n")

            def commit(at=None):
                git("add", ".")
                git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "recorded input", at=at)
                return git("rev-parse", "HEAD")

            base = commit()
            (repository / "generated.tsv").write_text("".join(f"row{index}\n" for index in range(801)))
            (repository / "arbitrary.txt").write_text("".join(f"unmarked{index}\n" for index in range(900)))
            (repository / "declared.tsv").write_text("".join(f"declared{index}\n" for index in range(801)))
            first = commit()
            (repository / "generated.tsv").write_text("".join(f"changed{index}\n" for index in range(801)))
            (repository / "arbitrary.txt").write_text("".join(f"other{index}\n" for index in range(900)))
            (repository / "declared.tsv").write_text("".join(f"updated{index}\n" for index in range(801)))
            second = commit()
            git("update-ref", "refs/remotes/origin/main", second)
            now = time.time()
            rows = [dict(id=1, ref="refs/heads/main", before=base, after=second,
                         timestamp=now - 1900)]
            for index in range(4):
                (repository / "lane.txt").write_text(str(index))
                lane = commit(at=now - 7200)
                git("update-ref", f"refs/remotes/origin/claude/land-{index}", lane, at=now - 1800)
                rows.append(dict(id=index + 2, ref=f"refs/heads/claude/land-{index}", before="0" * 40,
                                 after=lane, timestamp=now - 1800))
                git("reset", "--hard", second)
            (repository / "healthy.txt").write_text("first\n")
            third = commit()
            git("update-ref", "refs/remotes/origin/main", third)
            rows.append(dict(id=6, ref="refs/heads/main", before=second, after=third, timestamp=now))
            fixture = directory / "activity.json"

            def save_rows():
                copied = [dict(row, timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(row["timestamp"]))) for row in rows]
                fixture.write_text(json.dumps(copied))

            save_rows()
            tools = directory / "tools"
            tools.mkdir()
            (tools / "gh").write_text(f"#!/bin/sh\ncat '{fixture}'\n")
            (tools / "git").write_text(f"#!/bin/sh\nfor arg in \"$@\"; do [ \"$arg\" != fetch ] || exit 0; done\nexec '{real_git}' \"$@\"\n")
            for executable in tools.iterdir():
                executable.chmod(0o755)
            environment["PATH"] = str(tools) + ":" + environment["PATH"]
            status = directory / "lead-status.md"
            Path(environment["AGENTS_ORG_FILE"]).write_text(json.dumps(dict(version=1,
                proxy=dict(provider="claude", session="recorded-proxy-recipient"), nodes=[dict(
                id="fixture-lead", domain="fixture", role="lead", provider="claude", state="active",
                status_file=str(status), session="recorded-native-recipient")])) )

            def supervise():
                result = subprocess.run([str(ROOT / "bin/agents"), "supervise", "--once"],
                                        env=environment, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")

            def pending():
                result = subprocess.run([str(ROOT / "bin/threads"), "agent-events"],
                                        env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return json.loads(result.stdout)

            def refresh():
                state = directory / "landing-health.json"
                saved = json.loads(state.read_text())
                saved["fixture"]["attempted_at"] = 0
                state.write_text(json.dumps(saved))

            def assert_pushed(rows):
                for name, peer in recipients.items():
                    expected = [events.describe(row) for row in rows if row["to_node"] == name]
                    received = []
                    for _ in expected:
                        connection, _ = peer.accept()
                        with connection, connection.makefile("r") as stream:
                            received.append(json.loads(stream.readline()))
                    self.assertCountEqual(received, [dict(type="user", message=dict(role="user", content=body))
                                                     for body in expected])

            supervise()
            captured = pending()
            self.assertEqual(len(captured), 6)
            self.assertEqual({row["to_node"] for row in captured}, {"fixture-lead", "proxy"})
            self.assertTrue(all(row["from_node"] == "landing-health" for row in captured))
            self.assertTrue(all(row["delivered_at"] and not row["last_error"] for row in captured))
            assert_pushed(captured)
            self.assertEqual(sum("LANDING-STALL" in row["body"] for row in captured), 2)
            self.assertEqual(sum("REGEN-CHURN" in row["body"] for row in captured), 4)
            self.assertTrue(all("arbitrary.txt" not in row["body"] for row in captured))
            for inbox in (directory / "lead-inbox.md", directory / "handoffs/proxy-inbox.md"):
                self.assertIn("LANDING-STALL", inbox.read_text())
                self.assertIn("REGEN-CHURN", inbox.read_text())
            refresh()
            supervise()
            self.assertEqual(len(pending()), 6)
            (repository / "healthy.txt").write_text("second\n")
            fourth = commit()
            git("update-ref", "refs/remotes/origin/main", fourth)
            rows.append(dict(id=7, ref="refs/heads/main", before=third, after=fourth, timestamp=time.time()))
            save_rows()
            refresh()
            supervise()
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertFalse(saved["episodes"]["LANDING-STALL"]["active"])
            self.assertEqual(sum(at >= time.time() - 1800 for at in saved["observation"]["arrivals"].values()), 2)
            rows[-1]["after"] = second
            save_rows()
            refresh()
            actual_deliver = events.deliver

            def unavailable_inbox(node, body, **kwargs):
                if node["id"] == "proxy":
                    raise OSError("seeded unavailable recipient inbox")
                return actual_deliver(node, body, **kwargs)

            with patch.dict(os.environ, environment), patch.object(events, "deliver", side_effect=unavailable_inbox):
                events.main(["supervise", "--once"])
            failed = next(row for row in pending() if "LANDING-HEALTH-SOURCE" in row["body"] and row["to_node"] == "proxy")
            self.assertIsNone(failed["delivered_at"])
            self.assertIn("unavailable recipient inbox", failed["last_error"])
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertNotIn("proxy", saved["episodes"]["LANDING-HEALTH-SOURCE"]["sent"])
            self.assertTrue(all("arrival coverage unknown" in row["body"] for row in pending() if "LANDING-HEALTH-SOURCE" in row["body"]))
            (tools / "gh").write_text("#!/bin/sh\necho 'seeded API unavailable' >&2\nexit 1\n")
            refresh()
            with patch.dict(os.environ, environment), patch("time.time", return_value=time.time() + 61):
                events.main(["supervise", "--once"])
            retried = next(row for row in pending() if row["id"] == failed["id"])
            self.assertEqual(retried["body"], failed["body"])
            self.assertTrue(retried["delivered_at"])
            self.assertIsNone(retried["last_error"])
            assert_pushed([retried])
            self.assertIn(events.describe(retried), (directory / "handoffs/proxy-inbox.md").read_text())
            self.assertEqual(sum("LANDING-HEALTH-SOURCE" in row["body"] for row in pending()), 1)
            refresh()
            supervise()
            self.assertEqual(len(pending()), 7)
            (tools / "gh").write_text(f"#!/bin/sh\ncat '{fixture}'\n")
            fixture.write_text("[null]")
            refresh()
            supervise()
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertIn("schema incomplete", saved["source_error"])
            self.assertEqual(len(pending()), 7)
            fixture.write_text("[]")
            for index in range(4):
                ref = f"refs/remotes/origin/claude/land-{index}"
                lane = git("rev-parse", ref)
                git("update-ref", "-d", ref)
                git("update-ref", ref, lane, at=time.time() - 3600)
            state_path = directory / "landing-health.json"
            state_data = json.loads(state_path.read_text())
            state_data["fixture"]["observation"]["lanes"] = {}
            state_path.write_text(json.dumps(state_data))
            refresh()
            supervise()
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertIsNone(saved["source_error"])
            self.assertTrue(saved["episodes"]["LANDING-STALL"]["active"])
            self.assertTrue(all(at <= time.time() - 3600 for at in saved["observation"]["lanes"].values()))
            self.assertEqual(sum("LANDING-STALL" in row["body"] for row in pending()), 4)
            tip = git("rev-parse", "refs/remotes/origin/claude/land-0")
            for index in range(4, 70):
                git("update-ref", f"refs/remotes/origin/claude/land-{index}", tip)
            git("update-ref", "refs/remotes/origin/claude/land-merged", fourth)
            rows[:] = [dict(ref="refs/heads/claude/land-0", before=second, after=tip,
                            timestamp=time.time(), activity_type="branch_creation")]
            git("update-ref", "-d", "refs/remotes/origin/claude/land-0")
            git("update-ref", "refs/remotes/origin/claude/land-0", tip, at=rows[0]["timestamp"])
            save_rows()
            refresh()
            simulated = [0.0]
            calls = []
            external_run = subprocess.run
            external_monotonic = time.monotonic
            health_scope = threading.local()
            health_supervise = landing_health.supervise
            latency = [4]
            pending_calls = []
            backlog_started = threading.Event()
            health_completed = threading.Event()
            backlog_hold = [True]

            def scoped_health(*args, **kwargs):
                health_scope.active = True
                try:
                    if backlog_hold[0]:
                        self.assertTrue(backlog_started.wait(5))
                    return health_supervise(*args, **kwargs)
                finally:
                    health_scope.active = False
                    health_completed.set()

            def measured_monotonic():
                return simulated[0] if getattr(health_scope, "active", False) else external_monotonic()

            def measured_run(argv, **kwargs):
                if not getattr(health_scope, "active", False):
                    pending_calls.append((argv, kwargs.get("timeout")))
                    result = external_run(argv, **kwargs)
                    if backlog_hold[0]:
                        backlog_started.set()
                        self.assertTrue(health_completed.wait(5))
                        backlog_hold[0] = False
                    return result
                remaining = kwargs["timeout"]
                calls.append((argv, remaining, simulated[0]))
                simulated[0] += min(latency[0], remaining)
                if remaining < latency[0]:
                    raise subprocess.TimeoutExpired(argv, remaining)
                return external_run(argv, **kwargs)

            with patch.dict(os.environ, environment), patch("time.monotonic", side_effect=measured_monotonic), patch("subprocess.run", side_effect=measured_run), patch.object(landing_health, "supervise", side_effect=scoped_health):
                events.main(["supervise", "--once"])
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertIsNone(saved["source_error"])
            self.assertEqual(len(saved["observation"]["lanes"]), 70)
            self.assertGreater(saved["observation"]["lanes"]["refs/heads/claude/land-0"], time.time() - 60)
            self.assertLessEqual(simulated[0], 90)
            self.assertTrue(backlog_started.is_set() and health_completed.is_set())
            self.assertLessEqual(sum(argv[0] == "git" for argv, _, _ in calls), 9)
            refresh()
            simulated[0] = 0
            original = fixture.read_text()
            fixture.write_text(json.dumps([dict(ref="refs/heads/main", before=third, after=fourth,
                                                  timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()))] * 90))
            with patch.dict(os.environ, environment), patch("time.monotonic", side_effect=measured_monotonic), patch("subprocess.run", side_effect=measured_run), patch.object(landing_health, "supervise", side_effect=scoped_health):
                events.main(["supervise", "--once"])
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertIsNotNone(saved["source_error"])
            self.assertLessEqual(simulated[0], 90)
            self.assertTrue(saved["episodes"]["LANDING-HEALTH-SOURCE"]["active"])
            self.assertEqual(set(saved["episodes"]["LANDING-HEALTH-SOURCE"]["sent"]), {"proxy"})
            self.assertTrue(all(at + remaining <= 90 for _, remaining, at in calls))
            self.assertTrue(all(timeout == 20 for _, timeout in pending_calls))
            peer = directory / "repos/peer/main"
            shutil.copytree(repository, peer)
            simulated[0] = 0
            calls.clear()
            latency[0] = 2
            fair_state = json.loads(state_path.read_text())
            fair_state["fixture"]["attempted_at"] = time.time() - 121
            state_path.write_text(json.dumps(fair_state))
            with patch.dict(os.environ, environment), patch("time.monotonic", side_effect=measured_monotonic), patch("subprocess.run", side_effect=measured_run), patch.object(landing_health, "supervise", side_effect=scoped_health):
                events.main(["supervise", "--once"])
            fair = json.loads(state_path.read_text())
            self.assertLess(fair["peer"]["attempted_at"], fair["fixture"]["attempted_at"])
            self.assertTrue(all(fair[name]["source_error"] for name in ("peer", "fixture")))
            self.assertEqual(set(fair["peer"]["episodes"]["LANDING-HEALTH-SOURCE"]["sent"]), {"proxy"})
            self.assertEqual(set(fair["fixture"]["episodes"]["LANDING-HEALTH-SOURCE"]["sent"]), {"proxy"})
            self.assertLessEqual(simulated[0], 90)
            self.assertTrue(all(at + remaining <= 90 for _, remaining, at in calls))
            discovery = [call for call in calls if call[0][0] == "git" and call[0][-3:] == ["remote", "get-url", "origin"]]
            self.assertEqual(len(discovery), 4)
            self.assertLess(discovery[2][2], discovery[3][2])
            for index, start in enumerate(discovery[2:]):
                begin = calls.index(start)
                end = calls.index(discovery[3]) if index == 0 else len(calls)
                share = (90 - start[2]) / (2 - index)
                source_deadline = start[2] + share * 2 / 3
                repository_deadline = start[2] + share
                source_calls = [call for call in calls[begin:end] if call[0][0] in ("git", "gh")]
                delivery_calls = [call for call in calls[begin:end] if call[0][0] == str(ROOT / "bin/threads")]
                self.assertTrue(source_calls)
                if index == 0:
                    self.assertTrue(delivery_calls)
                self.assertTrue(all(at + remaining <= source_deadline + 1e-9 for _, remaining, at in source_calls))
                self.assertTrue(all(at + remaining <= repository_deadline + 1e-9 for _, remaining, at in delivery_calls))
            fixture.write_text(original)
            with patch.dict(os.environ, environment):
                registry = events.nodes()
                inbox = status.with_name("lead-inbox.md")
                row = next(row for row in pending() if row["to_node"] == "fixture-lead")
                with inbox.open("a+b") as held:
                    fcntl.flock(held, fcntl.LOCK_EX)
                    deadline = time.monotonic() + 1
                    with self.assertRaises(TimeoutError):
                        events.deliver(registry["fixture-lead"], "locked owner receipt", deadline=deadline)
                    with self.assertRaises(TimeoutError):
                        events.mirror(row, registry, deadline=deadline)
                    events.deliver(registry["proxy"], "proxy progresses past locked owner", deadline=deadline)
                events.deliver(registry["fixture-lead"], "owner receipt after unlock", deadline=time.monotonic() + 1)
                self.assertIn("owner receipt after unlock", inbox.read_text())
                self.assertIn("proxy progresses past locked owner", (directory / "handoffs/proxy-inbox.md").read_text())
            observation = dict(lanes={str(index): 0 for index in range(4)}, arrivals={}, changes={})
            self.assertNotIn("LANDING-STALL", landing_health.anomalies(observation, 1799))
            self.assertIn("LANDING-STALL", landing_health.anomalies(observation, 1800))


if __name__ == "__main__":
    unittest.main()
