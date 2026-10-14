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
                    {"id": "worker", "session": "recorded-session", "parent": "parent"}]}))
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

    def test_answered_and_forwarded_rows_never_reescalate(self):
        rng = random.Random(20261014)
        for _ in range(300):
            now = rng.uniform(0, 1e6)
            row = dict(time="1970-01-01T00:00:00Z", kind=rng.choice(events.KINDS),
                       to_node="parent", answered_by=rng.randint(1, 1000), attempted_at=None)
            self.assertEqual(events.due(row, now), [])
            row.update(answered_by=None, root_event=1, delivered_at=1)
            self.assertEqual(events.due(row, now), [])

    def test_nonclaude_supervisor_uses_inboxes_without_peer_attestation(self):
        with tempfile.TemporaryDirectory() as directory:
            previous = os.environ.get("AGENTS_ORG_FILE")
            os.environ["AGENTS_ORG_FILE"] = str(Path(directory) / "org.json")
            try:
                with socket.socket(socket.AF_UNIX) as peer:
                    path = Path(directory) / "peer.sock"
                    peer.bind(str(path))
                    peer.listen(1)
                    peer.setblocking(False)
                    proxy = dict(id="proxy", provider="claude", session="recorded-session", state="active", pid=os.getpid())
                    parent = dict(proxy, id="parent", status_file=str(Path(directory) / "parent-status.md"))
                    row = dict(id=1, kind="STUCK", from_node="worker", to_node="proxy", body="seeded proxy STUCK")
                    events.mirror(row, {})
                    body = events.describe(row)
                    events.deliver(proxy, body, locate=lambda _: f"uds:{path}")
                    with self.assertRaises(BlockingIOError):
                        peer.accept()
                    inbox = Path(directory) / "handoffs/proxy-inbox.md"
                    self.assertEqual(inbox.read_text(), body + "\n")
                    row.update(id=2, to_node="parent", body="seeded unattested Claude STUCK")
                    events.deliver(parent, events.describe(row), locate=lambda _: f"uds:{path}")
                    with self.assertRaises(BlockingIOError):
                        peer.accept()
                    self.assertEqual((Path(directory) / "parent-inbox.md").read_text(), events.describe(row) + "\n")
                    row.update(id=3, to_node="proxy", body="forwarded event without publisher mirror")
                    proxy.pop("pid")
                    events.deliver(proxy, events.describe(row), locate=lambda _: f"uds:{path}")
                    self.assertIn(events.describe(row), inbox.read_text())
            finally:
                if previous is None:
                    os.environ.pop("AGENTS_ORG_FILE", None)
                else:
                    os.environ["AGENTS_ORG_FILE"] = previous

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
