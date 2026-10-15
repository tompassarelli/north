#!/usr/bin/env python3
import json
import os
from contextlib import contextmanager, ExitStack
from pathlib import Path
import random
import socket
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import events
import org


@contextmanager
def socket_process(directory, *args, env=None):
    with subprocess.Popen([sys.executable, "-c", "import time; print('ready', flush=True); time.sleep(30)", *args],
                          env=env, stdout=subprocess.PIPE) as child:
        try:
            if child.stdout.readline() != b"ready\n":
                raise RuntimeError("socket process exited before becoming ready")
            with socket.socket(socket.AF_UNIX) as peer:
                peer.bind(str(Path(directory) / f"{child.pid}.sock"))
                peer.listen(2)
                peer.setblocking(False)
                yield peer
        finally:
            child.kill()


class Supervisor(unittest.TestCase):
    def test_seeded_durable_supervision_journey(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = dict(os.environ)
            try:
                os.environ.update(THREADS_DB=str(Path(directory) / "events.db"),
                                  AGENTS_ORG_FILE=str(Path(directory) / "org.json"))
                Path(os.environ["AGENTS_ORG_FILE"]).write_text(json.dumps({"version": 1, "nodes": [
                    {"id": "worker", "session": "recorded-session", "parent": "parent"},
                    {"id": "parent", "state": "active"}]}))
                no_launcher = "north-events-" + Path(directory).name
                with org.Org() as registry:
                    registry.nodes.append({"id": no_launcher, "state": "active"})
                    registry.save()
                self.assertFalse(org.close(no_launcher, recycle=True))
                store = events.Store()
                env = dict(os.environ, CODEX_THREAD_ID="recorded-session")
                published = subprocess.run([str(ROOT / "bin/agents"), "event", "STUCK", "--body", "blocked", "--key", "stuck"],
                                           env=env, capture_output=True, text=True)
                self.assertEqual(published.returncode, 0, published.stderr)
                stuck = json.loads(published.stdout)
                self.assertEqual((stuck["from_node"], stuck["to_node"]), ("worker", "parent"))
                refused = subprocess.run([str(ROOT / "bin/agents"), "event", "QUESTION", "--body", "unknown"],
                                         env=dict(env, CODEX_THREAD_ID="unregistered"), capture_output=True, text=True)
                self.assertNotEqual(refused.returncode, 0)
                self.assertEqual(len(store.pending()), 1)
                epoch = events.timestamp(stuck["time"])
                messages = []

                def dead_peer(node, message):
                    raise OSError("seeded unavailable parent")

                events.process(store, stuck, {}, epoch + 30, send=dead_peer)
                current = store.call("agent-events", "--id", stuck["id"])[0]
                self.assertIn("unavailable", current["last_error"])
                events.process(store, current, {}, epoch + 1200, send=dead_peer)
                pending = events.Store().pending()
                escalations = [row for row in pending if row.get("root_event") == stuck["id"]]
                self.assertEqual(len(escalations), 1)
                self.assertEqual(escalations[0]["to_node"], "proxy")
                events.process(store, store.call("agent-events", "--id", stuck["id"])[0], {}, epoch + 1230, send=dead_peer)
                self.assertEqual(len(store.pending()), 2)
                store.publish("ANSWER", from_node="proxy", to_node="worker", body="resolved", answer=escalations[0]["id"])
                self.assertEqual(store.pending(), [])
                land = store.publish("LAND-READY", from_node="worker", to_node="parent", body="reviewed", lane=directory, sha="candidate")
                epoch = events.timestamp(land["time"])
                send = lambda node, message: messages.append(message)
                events.process(store, land, {"parent": {}}, epoch + 1800, send=send, landed=lambda row: False)
                events.process(store, store.call("agent-events", "--id", land["id"])[0], {}, epoch + 2399, send=send, landed=lambda row: False)
                self.assertEqual([row["to_node"] for row in store.pending() if row.get("root_event") == land["id"]], ["parent"])
                events.process(store, store.call("agent-events", "--id", land["id"])[0], {}, epoch + 2400, send=send, landed=lambda row: False)
                forwarding = [row for row in store.pending() if row.get("root_event") == land["id"]]
                self.assertEqual([row["to_node"] for row in forwarding], ["parent", "proxy"])
                for row in forwarding:
                    events.process(store, row, {}, epoch + 7200, send=send)
                self.assertEqual(len(store.pending()), 3)
                recycle = store.publish("RECYCLE-READY", from_node="lead", to_node="parent", body="settled", packet=str(Path(directory) / "packet.md"))
                replacements = []

                def successor(row):
                    replacements.append(row["packet"])
                    return "fresh-session"

                epoch = events.timestamp(recycle["time"])
                events.process(store, recycle, {}, epoch + 900, send=send, restart=successor)
                reopened = events.Store().call("agent-events", "--id", recycle["id"])[0]
                events.process(store, reopened, {}, epoch + 960, send=send, restart=successor)
                self.assertEqual(replacements, [recycle["packet"]])
                self.assertEqual(reopened["successor"], "fresh-session")
                self.assertEqual(len(store.pending()), 4)
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def refused(self, directory, nodes, to_node):
        env = dict(os.environ, THREADS_DB=str(Path(directory) / "events.db"),
                   AGENTS_ORG_FILE=str(Path(directory) / "org.json"))
        Path(env["AGENTS_ORG_FILE"]).write_text(json.dumps({"version": 1, "nodes": nodes}))
        published = subprocess.run([str(ROOT / "bin/agents"), "event", "KICKOFF", "--from-node", "parent",
                                    "--to-node", to_node, "--body", "seeded ask", "--key", "ask"],
                                   env=env, capture_output=True, text=True)
        self.assertNotEqual(published.returncode, 0)
        self.assertIn(f"refused: {to_node} is", published.stderr)
        self.assertIn("rerouted to proxy", published.stderr)
        row = json.loads(published.stdout)
        previous = dict(os.environ)
        try:
            os.environ.update(env)
            store = events.Store()
            pending = store.pending()
            sent = []
            for current in pending:
                events.process(store, current, {}, events.timestamp(current["time"]) + 30,
                               send=lambda node, message: sent.append(message))
        finally:
            os.environ.clear()
            os.environ.update(previous)
        self.assertEqual(row["successor"], "proxy")
        self.assertEqual([current["to_node"] for current in pending], [to_node, "proxy"])
        self.assertEqual(len(sent), 1)
        self.assertIn("to=proxy", sent[0])
        self.assertIn("seeded ask", (Path(directory) / "handoffs/proxy-inbox.md").read_text())
        self.assertFalse((Path(directory) / f"{to_node}-inbox.md").exists())

    def test_kickoff_to_finished_node_is_refused_and_rerouted_to_proxy(self):
        with tempfile.TemporaryDirectory() as directory:
            status = Path(directory) / "lead-status.md"
            status.write_text("working\nFINISHED 03:51Z handed off\n")
            self.refused(directory, [{"id": "parent", "state": "active"},
                                     {"id": "lead", "state": "active", "parent": "parent", "status_file": str(status)}], "lead")

    def test_kickoff_to_removed_node_is_refused_and_rerouted_to_proxy(self):
        with tempfile.TemporaryDirectory() as directory:
            self.refused(directory, [{"id": "parent", "state": "active"}], "lead")

    def test_kickoff_to_dead_session_is_refused_and_rerouted_to_proxy(self):
        with tempfile.TemporaryDirectory() as directory:
            self.refused(directory, [{"id": "lead", "state": "active", "died": "recorded exit"}], "lead")

    def test_unacked_ask_is_repushed_at_one_minute_and_alarms_proxy_once_at_two(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = dict(os.environ)
            try:
                os.environ.update(THREADS_DB=str(Path(directory) / "events.db"),
                                  AGENTS_ORG_FILE=str(Path(directory) / "org.json"))
                store = events.Store()
                status = Path(directory) / "lead-status.md"
                status.write_text("started\n")
                os.utime(status, (0, 0))
                registry = {"lead": {"id": "lead", "parent": "proxy", "state": "active", "status_file": str(status)},
                            "proxy": {"id": "proxy", "state": "active"}}
                sent = []
                send = lambda node, message: sent.append(message)
                for kind in ("KICKOFF", "STUCK"):
                    sent.clear()
                    ask = store.publish(kind, from_node="proxy", to_node="lead", body=f"unacked {kind}")
                    epoch = events.timestamp(ask["time"])
                    for now in (epoch + 30, epoch + 59, epoch + 60, epoch + 90, epoch + 120, epoch + 1800):
                        events.process(store, store.call("agent-events", "--id", ask["id"])[0], registry, now, send=send)
                    current = store.call("agent-events", "--id", ask["id"])[0]
                    self.assertEqual(sum(message.startswith(f"EVENT {ask['id']} ") for message in sent), 2)
                    self.assertEqual(len(sent), 3)
                    self.assertIn("to=proxy", sent[-1])
                    self.assertEqual((current["resend_at"], current["proxy_escalated_at"]), (epoch + 60, epoch + 120))
                    alarms = [row for row in store.pending() if row.get("root_event") == ask["id"]]
                    self.assertEqual([row["to_node"] for row in alarms], ["proxy"])
                    self.assertTrue(alarms[0]["body"].startswith(f"ALARM ask {ask['id']} has no owner ACK"))
                sent.clear()
                acked = store.publish("KICKOFF", from_node="proxy", to_node="lead", body="owner wrote ACK")
                epoch = events.timestamp(acked["time"])
                events.process(store, acked, registry, epoch + 5, send=send)
                status.write_text(f"started\nACK {acked['id']} 06:50: plan, ETA 5 min\n")
                os.utime(status, (epoch + 30,) * 2)
                for now in (epoch + 60, epoch + 120, epoch + 1800):
                    events.process(store, store.call("agent-events", "--id", acked["id"])[0], registry, now, send=send)
                current = store.call("agent-events", "--id", acked["id"])[0]
                self.assertEqual(len(sent), 1)
                self.assertEqual((current["resend_at"], current["proxy_escalated_at"]), (None, None))
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_old_asks_to_unavailable_owners_forward_once_to_the_senders_parent(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "THREADS_DB": str(Path(directory) / "events.db"),
                "AGENTS_ORG_FILE": str(Path(directory) / "org.json")}):
            store = events.Store()
            status = Path(directory) / "parent-status.md"
            status.write_text("started\n")
            finished = Path(directory) / "finished-status.md"
            finished.write_text("FINISHED handed off\n")
            cases = [(None, "parent"), (dict(state="finished"), "parent"),
                     (dict(state="active", status_file=str(finished)), "parent"),
                     (None, None), (None, "removed-parent")]
            for owner, parent in cases:
                with socket_process(directory, "--session-id", "recorded-parent", "--settings", json.dumps(events.INBOUND)) as peer:
                    with self.subTest(owner=owner, parent=parent):
                        receiver = "parent" if parent == "parent" else "proxy"
                        node = dict(id=receiver, state="active", session="recorded-parent", status_file=str(status))
                        registered = [dict(id="sender", state="active", parent=parent)]
                        if receiver != "proxy":
                            registered.append(node)
                        if owner is not None:
                            registered.append(dict(owner, id="gone"))
                        Path(os.environ["AGENTS_ORG_FILE"]).write_text(json.dumps(
                            dict(version=1, nodes=registered, proxy=dict(node, id="proxy"))))
                        registry = events.nodes()
                        for kind in events.ACKED:
                            ask = store.publish(kind, from_node="sender", to_node="gone", body="old live-rule ask")
                            epoch = events.timestamp(ask["time"])
                            self.assertGreater(epoch, events.timestamp(events.DEFAULTS["event_ack_since"]))
                            for age in (20 * 3600, 20 * 3600 + 30):
                                events.process(store, store.call("agent-events", "--id", ask["id"])[0], registry, epoch + age,
                                               send=lambda node, body: events.deliver(node, body,
                                                   locate=lambda session: events.session_address(session, directory)))
                            forwards = [row for row in store.pending() if row.get("root_event") == ask["id"]]
                            self.assertEqual([row["to_node"] for row in forwards], [receiver])
                            self.assertFalse(forwards[0]["body"].startswith("ALARM"))
                            current = store.call("agent-events", "--id", ask["id"])[0]
                            self.assertEqual(current["parent_escalated_at"], epoch + 20 * 3600)
                            self.assertIsNone(current["resend_at"])
                            self.assertIsNone(current["proxy_escalated_at"])
                            self.assertIsNone(current["attempted_at"])
                            connection, _ = peer.accept()
                            with connection:
                                message = json.loads(connection.makefile().readline())["message"]["content"]
                            self.assertIn(f"to={receiver}", message)
                            self.assertIn(f"EVENT {ask['id']} ", message)
                            with self.assertRaises(BlockingIOError):
                                peer.accept()

    def test_asks_before_ack_activation_do_not_enter_ack_supervision(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "THREADS_DB": str(Path(directory) / "events.db"),
                "AGENTS_ORG_FILE": str(Path(directory) / "org.json")}):
            status = Path(directory) / "lead-status.md"
            status.write_text("started\n")
            Path(os.environ["AGENTS_ORG_FILE"]).write_text(json.dumps(dict(version=1, nodes=[
                dict(id="lead", state="active", status_file=str(status))])))
            store = events.Store()
            ask = store.publish("KICKOFF", from_node="proxy", to_node="lead", body="pre-rule ask")
            before = "2026-10-15T07:34:00Z"
            with sqlite3.connect(os.environ["THREADS_DB"]) as db:
                db.execute("UPDATE events SET ts=? WHERE seq=?", (before, ask["id"]))
            epoch = events.timestamp(before)
            store.update(ask["id"], delivered_at=epoch + 5)
            sent = []
            for age in (60, 120, 20 * 3600, 20 * 3600 + 30):
                current = store.call("agent-events", "--id", ask["id"])[0]
                events.process(store, current, events.nodes(), epoch + age,
                               send=lambda node, body: sent.append(body))
            self.assertEqual(sent, [])
            self.assertEqual([row["id"] for row in store.pending()], [ask["id"]])
            current = store.call("agent-events", "--id", ask["id"])[0]
            self.assertIsNone(current["resend_at"])
            self.assertIsNone(current["proxy_escalated_at"])
            for kind in events.ACKED:
                self.assertEqual(events.due(dict(current, kind=kind), epoch + 120, progressed=False), [])
            self.assertEqual(events.due(dict(current, to_node="gone"), epoch + 20 * 3600,
                                        progressed=False, active=False), [])

    def test_unrelated_ack_and_worker_start_cannot_silence_an_unanswered_request(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "THREADS_DB": str(Path(directory) / "events.db"),
                "AGENTS_ORG_FILE": str(Path(directory) / "org.json")}):
            store = events.Store()
            ask = store.publish("KICKOFF", from_node="proxy", to_node="lead", body="unseen request")
            epoch = events.timestamp(ask["time"])
            status = Path(directory) / "lead-status.md"
            status.write_text(f"ACK {ask['id'] + 1} 00:00: another request\nACK {ask['id']}0 prefix collision\nworking\n")
            os.utime(status, (epoch + 30,) * 2)
            registry = {"lead": dict(id="lead", state="active", status_file=str(status)),
                        "worker": dict(id="worker", parent="lead", started=ask["time"]),
                        "proxy": dict(id="proxy", state="active")}
            sent = []
            for age in (5, 60, 120):
                events.process(store, store.call("agent-events", "--id", ask["id"])[0], registry, epoch + age,
                               send=lambda node, message: sent.append(node["id"]))
            self.assertEqual(sent, ["lead", "lead", "proxy"])
            status.write_text(f"ACK {ask['id']} 00:02: this request\n")
            os.utime(status, (0, 0))
            self.assertTrue(events.progressed(ask, registry))
            store.publish("ANSWER", from_node="lead", to_node="proxy", answer=ask["id"], body="resolved")
            answered = store.call("agent-events", "--id", ask["id"])[0]
            self.assertEqual(events.due(answered, epoch + 180, progressed=False), [])

    def test_failed_repush_retries_after_throttle_even_after_alarm(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "THREADS_DB": str(Path(directory) / "events.db"),
                "AGENTS_ORG_FILE": str(Path(directory) / "org.json")}):
            store = events.Store()
            ask = store.publish("KICKOFF", from_node="proxy", to_node="lead", body="retry request")
            epoch = events.timestamp(ask["time"])
            registry = {name: dict(id=name, state="active") for name in ("lead", "proxy")}
            sent = []

            def send(node, message):
                sent.append(node["id"])
                if node["id"] == "lead" and sent.count("lead") in (2, 3):
                    raise OSError("seeded socket push failure")

            for age in (5, 60, 75, 120, 135):
                events.process(store, store.call("agent-events", "--id", ask["id"])[0], registry, epoch + age, send=send)
                current = store.call("agent-events", "--id", ask["id"])[0]
                self.assertIsNone(current["resend_at"])
                if age >= 60:
                    self.assertIn("seeded socket push failure", current["last_error"])
                    self.assertEqual(current["delivered_at"], epoch + 5)
            self.assertEqual(sent, ["lead", "lead", "lead", "proxy"])
            events.process(store, current, registry, epoch + 180, send=send)
            current = store.call("agent-events", "--id", ask["id"])[0]
            self.assertEqual(sent, ["lead", "lead", "lead", "proxy", "lead"])
            self.assertEqual(current["resend_at"], epoch + 180)
            self.assertEqual(current["attempted_at"], epoch + 180)
            self.assertIsNone(current["last_error"])
            events.process(store, current, registry, epoch + 240, send=send)
            self.assertEqual(len(sent), 5)

    def test_delayed_supervisor_repushes_before_alarm_on_the_same_pass(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
                "THREADS_DB": str(Path(directory) / "events.db"),
                "AGENTS_ORG_FILE": str(Path(directory) / "org.json")}):
            store = events.Store()
            ask = store.publish("KICKOFF", from_node="proxy", to_node="lead", body="delayed request")
            epoch = events.timestamp(ask["time"])
            store.update(ask["id"], delivered_at=epoch + 5)
            registry = {name: dict(id=name, state="active") for name in ("lead", "proxy")}
            sent = []
            events.process(store, store.call("agent-events", "--id", ask["id"])[0], registry, epoch + 121,
                           send=lambda node, message: sent.append(node["id"]))
            self.assertEqual(sent, ["lead", "proxy"])
            current = store.call("agent-events", "--id", ask["id"])[0]
            self.assertEqual((current["resend_at"], current["proxy_escalated_at"]), (epoch + 121,) * 2)
            self.assertEqual(events.due(current, epoch + 181, progressed=False), [])

    def test_answered_and_forwarded_rows_never_reescalate(self):
        rng = random.Random(20261014)
        for _ in range(300):
            now = rng.uniform(0, 1e6)
            row = dict(time="1970-01-01T00:00:00Z", kind=rng.choice(events.KINDS),
                       to_node="parent", answered_by=rng.randint(1, 1000), attempted_at=None)
            self.assertEqual(events.due(row, now), [])
            row.update(answered_by=None, root_event=1, delivered_at=1)
            self.assertEqual(events.due(row, now), [])

    def test_claude_delivery_appends_inbox_and_pushes_socket_or_raises(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = os.environ.get("AGENTS_ORG_FILE")
            os.environ["AGENTS_ORG_FILE"] = str(Path(directory) / "org.json")
            try:
                with socket.socket(socket.AF_UNIX) as peer:
                    path = Path(directory) / "4242.sock"
                    peer.bind(str(path))
                    peer.listen(2)
                    peer.settimeout(5)
                    proxy = dict(id="proxy", provider="claude", session="recorded-session", state="active")
                    parent = dict(proxy, id="parent", status_file=str(Path(directory) / "parent-status.md"))
                    located = lambda session: f"uds:{path}" if session == b"recorded-session" else None
                    checked = []
                    accept = lambda pid: checked.append(pid) or True
                    for node, inbox in ((parent, Path(directory) / "parent-inbox.md"),
                                        (proxy, Path(directory) / "handoffs/proxy-inbox.md")):
                        body = events.describe(dict(id=1, kind="KICKOFF", from_node="worker", to_node=node["id"], body="seeded ask"))
                        events.deliver(node, body, locate=located, accepts=accept)
                        connection, _ = peer.accept()
                        with connection:
                            received = connection.makefile().readline()
                        self.assertEqual(json.loads(received), {"type": "user", "message": {"role": "user", "content": body}})
                        self.assertEqual(inbox.read_text(), body + "\n")
                    self.assertEqual(checked, ["4242", "4242"])
                    with self.assertRaisesRegex(RuntimeError, "no live socket"):
                        events.deliver(parent, "unreachable", locate=lambda session: None, accepts=accept)
                    with self.assertRaisesRegex(RuntimeError, "crossSessionInbound=accept"):
                        events.deliver(parent, "dropped", locate=located, accepts=lambda pid: False)
                    peer.setblocking(False)
                    with self.assertRaises(BlockingIOError):
                        peer.accept()
            finally:
                if previous is None:
                    os.environ.pop("AGENTS_ORG_FILE", None)
                else:
                    os.environ["AGENTS_ORG_FILE"] = previous

    def test_inbound_acceptance_reads_launch_settings(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory)
            sleeper = [sys.executable, "-c", "from pathlib import Path; import time; print(Path('/proc/self/stat').read_text().split()[0], flush=True); time.sleep(30)"]
            cases = [(sleeper + ["--settings", json.dumps(events.INBOUND)], {"CLAUDE_CONFIG_DIR": str(profile)}, True),
                     (sleeper, {"CLAUDE_CONFIG_DIR": str(profile)}, False)]
            (profile / "settings.json").write_text(json.dumps({"crossSessionInbound": "ask"}))
            for argv, extra, expected in cases:
                child = subprocess.Popen(argv, env=dict(os.environ, **extra), stdout=subprocess.PIPE)
                try:
                    pid = child.stdout.readline().strip().decode()
                    self.assertEqual(events.accepts_inbound(pid), expected)
                    if not expected:
                        (profile / "settings.json").write_text(json.dumps(events.INBOUND))
                        child.kill()
                        child.wait()
                        child.stdout.close()
                        child = subprocess.Popen(sleeper + ["--settings", json.dumps(events.INBOUND)],
                                                 env=dict(os.environ, **extra), stdout=subprocess.PIPE)
                        pid = child.stdout.readline().strip().decode()
                        self.assertTrue(events.accepts_inbound(pid))
                finally:
                    child.kill()
                    child.wait()
                    child.stdout.close()

    def test_socket_push_reaches_only_the_lead_and_excludes_forks_and_mentions(self):
        with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
            peers = [stack.enter_context(socket_process(directory, *args, "--settings", json.dumps(events.INBOUND)))
                     for args in (("--resume", "recorded-session", "--fork-session"),
                                  ("--name", "recorded-session"), ("--session-id", "recorded-session"))]
            node = dict(id="lead", provider="claude", state="active", session="recorded-session",
                        status_file=str(Path(directory) / "lead-status.md"))
            events.deliver(node, "lead only", locate=lambda session: events.session_address(session, directory))
            connection, _ = peers[-1].accept()
            with connection:
                self.assertEqual(json.loads(connection.makefile().readline())["message"]["content"], "lead only")
            for peer in peers[:-1]:
                with self.assertRaises(BlockingIOError):
                    peer.accept()
            self.assertEqual((Path(directory) / "lead-inbox.md").read_text(), "lead only\n")

    def test_ambiguous_session_sockets_keep_the_event_in_the_inbox(self):
        with tempfile.TemporaryDirectory() as directory, ExitStack() as stack:
            peers = [stack.enter_context(socket_process(directory, "--resume", "recorded-session",
                                                        "--settings", json.dumps(events.INBOUND))) for _ in range(2)]
            node = dict(id="lead", provider="claude", state="active", session="recorded-session",
                        status_file=str(Path(directory) / "lead-status.md"))
            with self.assertRaisesRegex(RuntimeError, "no live socket"):
                events.deliver(node, "ambiguous", locate=lambda session: events.session_address(session, directory))
            for peer in peers:
                with self.assertRaises(BlockingIOError):
                    peer.accept()
            self.assertEqual((Path(directory) / "lead-inbox.md").read_text(), "ambiguous\n")

    def test_profile_acceptance_without_inline_launch_settings_keeps_the_event_in_the_inbox(self):
        with tempfile.TemporaryDirectory() as directory:
            profile = Path(directory) / "settings.json"
            profile.write_text(json.dumps(events.INBOUND))
            node = dict(id="lead", provider="claude", state="active", session="recorded-session",
                        status_file=str(Path(directory) / "lead-status.md"))
            cases = [("--setting-sources", "project,local"), ("--settings", str(profile)),
                     ("--settings", "invalid"), ("--settings", '{"crossSessionInbound":"ask"}'),
                     ("--settings", json.dumps(events.INBOUND), "--settings", '{"crossSessionInbound":"ask"}')]
            for index, args in enumerate(cases):
                with self.subTest(args=args), socket_process(directory, "--session-id", "recorded-session", *args,
                        env=dict(os.environ, CLAUDE_CONFIG_DIR=directory)) as peer:
                    with self.assertRaisesRegex(RuntimeError, "crossSessionInbound=accept"):
                        events.deliver(node, f"unaccepted {index}",
                                       locate=lambda session: events.session_address(session, directory))
                    with self.assertRaises(BlockingIOError):
                        peer.accept()
                    self.assertIn(f"unaccepted {index}\n", (Path(directory) / "lead-inbox.md").read_text())

    def test_registered_codex_proxy_receives_control_without_restart(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = dict(os.environ)
            previous_root = events.ROOT
            root = Path(directory)
            try:
                os.environ.update(AGENTS_ORG_FILE=str(root / "org.json"),
                                  EVENT_CONTROL_RECEIPT=str(root / "control.json"))
                session = "01a13b4d-b6d0-7221-8f25-0fa0e99d558d"
                inbox = root / "handoffs/proxy-inbox.md"
                proxy = dict(id="proxy", provider="codex", session=session,
                             state="active", pid=os.getpid(), status_file=str(inbox))
                Path(os.environ["AGENTS_ORG_FILE"]).write_text(json.dumps(
                    dict(version=1, nodes=[], proxy=proxy)))
                (root / "bin").mkdir()
                control = root / "bin/codex-lead"
                control.write_text("#!/usr/bin/env python3\nimport json, os, pathlib, sys\n"
                                   "pathlib.Path(os.environ['EVENT_CONTROL_RECEIPT']).write_text("
                                   "json.dumps([sys.argv[1:], os.environ.get('CODEX_LEAD_THREAD')]))\n"
                                   "sys.exit(int(os.environ.get('EVENT_CONTROL_FAIL', '0')))\n")
                control.chmod(0o700)
                events.ROOT = root
                row = dict(id=4, kind="QUESTION", from_node="worker", to_node="proxy",
                           body="recorded Codex takeover checkpoint")
                registry = events.nodes()
                events.mirror(row, registry)
                body = events.describe(row)
                events.deliver(registry["proxy"], body)
                self.assertEqual(json.loads((root / "control.json").read_text()),
                                 [["send", body], session])
                self.assertEqual(inbox.read_text(), body + "\n")
                self.assertFalse((root / "handoffs/proxy-inbox.md-inbox.md").exists())
                os.environ["EVENT_CONTROL_FAIL"] = "1"
                with self.assertRaisesRegex(RuntimeError, "Codex delivery failed"):
                    events.deliver(registry["proxy"], body)
                registry["proxy"]["died"] = "recorded absent root"
                with self.assertRaisesRegex(RuntimeError, "proxy.*unavailable"):
                    events.deliver(registry["proxy"], body)
            finally:
                events.ROOT = previous_root
                os.environ.clear()
                os.environ.update(previous)


if __name__ == "__main__":
    unittest.main()
