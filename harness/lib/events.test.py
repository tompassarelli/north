#!/usr/bin/env python3
import json
import os
from pathlib import Path
import random
import socket
import subprocess
import sys
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import events
import org


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

    def test_inbound_acceptance_reads_launch_settings_or_profile_settings(self):
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
                        self.assertTrue(events.accepts_inbound(pid))
                finally:
                    child.kill()
                    child.wait()
                    child.stdout.close()

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
