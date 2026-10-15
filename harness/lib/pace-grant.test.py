#!/usr/bin/env python3
import importlib.util
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import socket
import sqlite3
import struct
import sys
import tempfile
import time
import threading
import unittest
from unittest.mock import patch
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pace_sampler


class ProviderPeer:
    def __init__(self, root, statuses):
        self.statuses = statuses
        self.pool = root / "pool"
        (self.pool / "sqlite").mkdir(parents=True)
        self.db = self.pool / "sqlite/state_5.sqlite"
        with sqlite3.connect(self.db) as db:
            db.executescript("CREATE TABLE threads(id PRIMARY KEY, archived); CREATE TABLE thread_spawn_edges(parent_thread_id,child_thread_id);")
            db.execute("INSERT INTO threads VALUES ('parent',0)")
        directory = self.pool / "app-server-control"
        directory.mkdir()
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.bind(str(directory / "app-server-control.sock"))
        self.socket.listen()
        self.socket.settimeout(0.1)
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    def serve(self):
        def read(peer, count):
            data = b""
            while len(data) < count:
                block = peer.recv(count - len(data))
                if not block:
                    raise EOFError
                data += block
            return data
        while not self.stop.is_set():
            try:
                peer, _ = self.socket.accept()
            except (socket.timeout, OSError):
                continue
            with peer:
                peer.settimeout(5)
                try:
                    header = b""
                    while not header.endswith(b"\r\n\r\n"):
                        header += read(peer, 1)
                    key = next(line.split(b":", 1)[1].strip() for line in header.split(b"\r\n") if line.lower().startswith(b"sec-websocket-key:"))
                    accept = base64.b64encode(hashlib.sha1(key + b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest())
                    peer.sendall(b"HTTP/1.1 101 Switching Protocols\r\nSec-WebSocket-Accept: " + accept + b"\r\n\r\n")
                    while True:
                        head = read(peer, 2)
                        length = head[1] & 127
                        if length == 126:
                            length = struct.unpack("!H", read(peer, 2))[0]
                        elif length == 127:
                            length = struct.unpack("!Q", read(peer, 8))[0]
                        mask = read(peer, 4) if head[1] & 128 else None
                        data = read(peer, length)
                        if mask:
                            data = bytes(value ^ mask[i % 4] for i, value in enumerate(data))
                        request = json.loads(data)
                        if "id" not in request:
                            continue
                        method = request["method"]
                        result = {"data": list(self.statuses), "nextCursor": None} if method == "thread/loaded/list" else {}
                        if method == "thread/read":
                            result = {"thread": {"status": {"type": self.statuses[request["params"]["threadId"]]}}}
                        payload = json.dumps({"id": request["id"], "result": result}).encode()
                        framing = bytes([0x81, len(payload)]) if len(payload) < 126 else b"\x81\x7e" + struct.pack("!H", len(payload))
                        peer.sendall(framing + payload)
                except (EOFError, OSError):
                    pass

    def close(self):
        self.stop.set()
        self.socket.close()
        self.thread.join(timeout=6)


class Grants(unittest.TestCase):
    def test_held_spawn_reports_only_its_lapsed_grant_once(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / ".local/state/agents"
            state.mkdir(parents=True)
            org = state / "org.json"
            node = {"id": "lead", "session": "parent", "provider": "codex", "state": "active"}
            org.write_text(json.dumps({"nodes": [node]}))
            now = time.time()
            window = {"window": "week", "as_of": datetime.now(timezone.utc).isoformat(),
                      "used_pct": 50, "burn_pct_h": 0, "reset_h": 24, "resets_at": "reset"}
            control = {"at": now, "held": True, "workers": 1, "allowed_workers": 1, "windows": [window]}
            verdict = {"epoch": now, "stale_min": 30, "pace_control": True, "refusal": None,
                       "controls": {"codex": control}}
            (state / "usage-gate.json").write_text(json.dumps({"codex": verdict}))
            expired = {"account": "codex", "lead": "lead", "session": "parent", "max_workers": 4,
                       "until": now - 3600, "until_utc": datetime.fromtimestamp(now - 3600, timezone.utc).isoformat(),
                       "authority": "proxy", "at": now - 7200, "reason": "fixture admission"}
            (state / "pace-control.json").write_text(json.dumps({"grants": [expired]}))
            policy = root / "orchestration.toml"
            policy.write_text("")
            previous = dict(os.environ)
            try:
                os.environ.update(SPAWN_CAPACITY_HOME=str(root), XDG_STATE_HOME=str(root / ".local/state"),
                                  AGENTS_ORG_FILE=str(org), AGENTS_ORCHESTRATION=str(policy))
                spec = importlib.util.spec_from_file_location("expiry_capacity", HERE.parent / "hooks/lib/spawn_capacity.py")
                capacity = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(capacity)
                event = {"tool_name": "collaborationspawn_agent", "session_id": "foreign",
                         "cwd": str(root), "tool_input": {"task_name": "fix", "message": "original"}}
                inbox = state / "handoffs/proxy-inbox.md"
                self.assertTrue(capacity.evaluate(event)[0].startswith("Hold:"))
                self.assertFalse(inbox.exists())
                event["session_id"] = "parent"
                org.write_text(json.dumps({"nodes": [node | {"session": "replacement"}]}))
                self.assertTrue(capacity.evaluate(event)[0].startswith("Hold:"))
                self.assertFalse(inbox.exists())
                org.write_text(json.dumps({"nodes": [node]}))
                held = capacity.evaluate(event)
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(inbox.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                self.assertIn("lead", inbox.read_text())
                self.assertIn(expired["until_utc"], inbox.read_text())
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **expired}) + "\n")
                data = json.loads((state / "pace-control.json").read_text())
                data["grants"] = []
                (state / "pace-control.json").write_text(json.dumps(data))
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(inbox.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                renewed = expired | {"until": now + 3600, "at": now}
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **renewed}) + "\n")
                inbox.unlink()
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(inbox.exists())
                replacement = expired | {"at": now + 1, "task": "other"}
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **replacement}) + "\n")
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(inbox.exists())
                consumed = expired | {"at": now + 2, "uses": 1}
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **consumed}) + "\n")
                    log.write(json.dumps({"event": "grant-admission", "at": now + 3,
                                          "lead": "lead", "session": "parent", "account": "codex"}) + "\n")
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(inbox.exists())
                remaining = expired | {"at": now + 4, "uses": 1, "task": "fix"}
                (state / "pace-control.json").write_text(json.dumps({"grants": [remaining]}))
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **(remaining | {"uses": 2})}) + "\n")
                    log.write(json.dumps({"event": "grant-admission", "at": now + 5,
                                          "lead": "lead", "session": "parent", "account": "codex"}) + "\n")
                    log.write("[" * 10000 + "0" + "]" * 10000 + "\n")
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(inbox.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(inbox.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                exhausted = remaining | {"at": now + 6, "uses": 0}
                (state / "pace-control.json").write_text(json.dumps({"grants": [exhausted]}))
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **(exhausted | {"uses": 2})}) + "\n")
                inbox.unlink()
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(inbox.exists())
            finally:
                os.environ.clear()
                os.environ.update(previous)

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
            peer = ProviderPeer(root, {"parent": "active"})
            try:
                os.environ.update(HOME=str(root), XDG_STATE_HOME=str(root / ".local/state"),
                                  AGENTS_ORG_FILE=str(org), SPAWN_CAPACITY_HOME=str(root),
                                  SPAWN_CAPACITY_STATUS=str(probe), CODEX_BEHAVIOR_STATE=str(root / "behavior"))
                os.environ["NORTH_CODEX_CONVERSATION_HOME"] = str(peer.pool)
                os.environ["NORTH_CODEX_CONVERSATION_SQLITE_HOME"] = str(peer.pool / "sqlite")
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
                         "tool_use_id": "first",
                         "tool_input": {"task_name": "playtest", "message": "[routine:watchdog]"}}
                self.assertIsNone(capacity.evaluate(event)[0])
                capacity.confirm_spawn(event)
                event["tool_use_id"] = "second"
                self.assertIsNone(capacity.evaluate(event)[0])
                capacity.confirm_spawn(event)
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
                data = json.loads((state / "pace-control.json").read_text())
                data["grants"][0]["pending"][0]["id"] = "review"
                (state / "pace-control.json").write_text(json.dumps(data))
                pace_sampler.confirm(state, "parent", "review")
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
                peer.close()
                os.environ.clear()
                os.environ.update(previous)

    def test_refused_spawns_release_slots_without_consuming_uses(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            org = state / "org.json"
            org.write_text(json.dumps({"nodes": [
                {"id": "lead", "session": "parent", "provider": "codex", "state": "active"}]}))
            peer = ProviderPeer(root, {"parent": "active"})
            try:
                with patch.dict(os.environ, XDG_STATE_HOME=str(root), AGENTS_ORG_FILE=str(org),
                                NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                                NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite")):
                    expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                    pace_sampler.grant("codex", 2, expiry, "lead", "refused spawn regression", "review", 3)
                    gate = state / "usage-gate.json"
                    def reserve(identity):
                        return pace_sampler.reserve(gate, "codex", "codex", session="parent",
                                                    task="review", tool_use_id=identity)
                    now = time.time()
                    with patch.object(pace_sampler.time, "time", return_value=now):
                        self.assertIsNone(reserve("denied-one"))
                        self.assertIsNone(reserve("denied-two"))
                        self.assertIn("2/2", reserve("blocked"))
                    with patch.object(pace_sampler.time, "time", return_value=now + pace_sampler.RESERVATION_TTL_S + 1):
                        self.assertIsNone(reserve("started"))
                        self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0]["uses"], 3)
                        pace_sampler.confirm(state, "parent", "started")
                        pace_sampler.confirm(state, "parent", "started")
                        self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0]["uses"], 2)
                        self.assertIsNone(reserve("another"))
                        self.assertIn("2/2", reserve("blocked-again"))
                        rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                        self.assertEqual([r["tool_use_id"] for r in rows if r["event"] == "grant-admission"], ["started"])
            finally:
                peer.close()

    def test_grant_subtree_live_workers_and_pending_admissions(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            org = state / "org.json"
            org.write_text(json.dumps({"nodes": [
                {"id": "lead", "session": "parent", "provider": "codex", "state": "active"},
                {"id": "sub", "session": "subparent", "parent": "lead", "provider": "codex", "state": "active"},
                {"id": "other", "session": "otherparent", "provider": "codex", "state": "active"}]}))
            peer = ProviderPeer(root, {"parent": "active", "child": "active", "grandchild": "active",
                                      "subparent": "idle", "otherparent": "active", "unrelated": "active",
                                      "finished": "idle", "archived": "active"})
            with sqlite3.connect(peer.db) as db:
                db.executemany("INSERT INTO threads VALUES (?,?)", [(name, name == "archived") for name in peer.statuses if name != "parent"])
                db.executemany("INSERT INTO thread_spawn_edges VALUES (?,?)", [
                    ("parent", "child"), ("parent", "finished"), ("parent", "archived"),
                    ("subparent", "grandchild"), ("otherparent", "unrelated")])
            previous = dict(os.environ)
            try:
                os.environ.update(XDG_STATE_HOME=str(root), AGENTS_ORG_FILE=str(org),
                                  NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                                  NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite"))
                (state / "pace-control.json").write_text(json.dumps({"accounts": {"codex": {"workers": 99}}}))
                gate = state / "usage-gate.json"
                gate.write_text("{}")
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                pace_sampler.grant("codex", 3, expiry, "lead", "proxy subtree grant")
                self.assertIsNone(pace_sampler.reserve(gate, "codex", "codex", session="parent"))
                self.assertIn("3/3", pace_sampler.reserve(gate, "codex", "codex", session="parent"))
                with sqlite3.connect(peer.db) as db:
                    db.execute("INSERT INTO threads VALUES ('born',0)")
                    db.execute("INSERT INTO thread_spawn_edges VALUES ('parent','born')")
                peer.statuses["born"] = "active"
                self.assertIn("3/3", pace_sampler.reserve(gate, "codex", "codex", session="parent"))
                peer.statuses["child"] = "idle"
                command = [sys.executable, "-c", "import sys;sys.path.insert(0,sys.argv[1]);import pace_sampler;from pathlib import Path;print(pace_sampler.reserve(Path(sys.argv[2]),'codex','codex',session='parent') or 'ALLOW')", str(HERE), str(gate)]
                processes = [subprocess.Popen(command, stdout=subprocess.PIPE, text=True) for _ in range(4)]
                results = [p.communicate(timeout=10)[0].strip() for p in processes]
                self.assertEqual(results.count("ALLOW"), 1)
                self.assertEqual(json.loads((state / "pace-control.json").read_text())["accounts"]["codex"]["workers"], 99)
                org.write_text(json.dumps({"nodes": [
                    {"id": "lead", "session": "claude-parent", "provider": "claude", "account": "c", "state": "active"},
                    {"id": "sub", "session": "claude-sub", "parent": "lead", "provider": "claude", "account": "c", "state": "active"},
                    {"id": "other", "session": "unrelated-parent", "provider": "claude", "account": "c", "state": "active"}]}))
                profile = root / "claude"
                policy = root / "orchestration.toml"
                policy.write_text(f'[accounts.claude.profiles.c."{socket.gethostname()}"]\npath="{profile}"\n')
                vendor = root / "vendor"
                vendor.mkdir()
                command = vendor / "claude"
                command.write_text('#!/bin/sh\necho \'[{"sessionId":"claude-parent","status":"busy"},{"sessionId":"claude-sub","status":"busy"},{"sessionId":"unrelated-parent","status":"busy"}]\'\n')
                command.chmod(0o755)
                os.environ.update(AGENTS_ORCHESTRATION=str(policy), PATH=str(vendor) + os.pathsep + os.environ["PATH"])
                workers = profile / "projects/project/claude-parent/subagents"
                workers.mkdir(parents=True)
                (workers / "agent-live.jsonl").write_text(json.dumps({"type": "assistant", "message": {"content": [{"type": "tool_use"}]}}) + "\n")
                (workers / "agent-finished.jsonl").write_text(json.dumps({"type": "assistant", "message": {"stop_reason": "end_turn", "content": [{"type": "thinking"}, {"type": "text"}]}}) + "\n")
                unrelated = profile / "projects/project/unrelated-parent/subagents"
                unrelated.mkdir(parents=True)
                (unrelated / "agent-foreign.jsonl").write_text(json.dumps({"type": "user", "message": {"content": "running"}}) + "\n")
                pace_sampler.grant("claude:c", 3, expiry, "lead", "proxy Claude subtree grant")
                self.assertIsNone(pace_sampler.reserve(gate, "claude", "claude:c", session="claude-parent"))
                self.assertIn("3/3", pace_sampler.reserve(gate, "claude", "claude:c", session="claude-parent"))
            finally:
                peer.close()
                os.environ.clear()
                os.environ.update(previous)


if __name__ == "__main__":
    unittest.main()
