#!/usr/bin/env python3
import functools
import json
import os
from pathlib import Path
import random
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
                store = events.Store()
                stuck = store.publish("STUCK", from_node="worker", to_node="parent", body="blocked", key="stuck")
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
                events.process(store, store.call("agent-events", "--id", land["id"])[0], {}, epoch + 3600, send=send, landed=lambda row: False)
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

    def test_claude_socket_boundary_uses_registered_native_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            helper = Path(directory) / "peer.py"
            helper.write_text("import json,os,socket,sys\nfrom pathlib import Path\np=Path(sys.argv[1])/f'{os.getpid()}.sock'\nwith socket.socket(socket.AF_UNIX) as s:\n s.bind(str(p));s.listen(1);print('ready',flush=True)\n with s.accept()[0] as c:\n  chunks=[]\n  while b:=c.recv(65536): chunks.append(b)\n  print(b''.join(chunks).decode(),flush=True)\n")
            session = "00000000-0000-4000-8000-000000000001"
            peer = subprocess.Popen([sys.executable, str(helper), directory, session], stdout=subprocess.PIPE, text=True)
            try:
                self.assertEqual(peer.stdout.readline().strip(), "ready")
                node = dict(id="parent", provider="claude", session=session, state="active", pid=peer.pid)
                locate = functools.partial(org.address, directory=directory)
                self.assertIsNone(locate(b"00000000-0000-4000-8000-000000000002"))
                events.deliver(node, "seeded STUCK", locate=locate)
                packet = json.loads(peer.stdout.readline())
                self.assertEqual(packet, {"type": "user", "message": {"role": "user", "content": "seeded STUCK"}})
                self.assertEqual(peer.wait(timeout=3), 0)
            finally:
                if peer.poll() is None:
                    peer.terminate()
                    peer.wait(timeout=3)
                peer.stdout.close()


if __name__ == "__main__":
    unittest.main()
