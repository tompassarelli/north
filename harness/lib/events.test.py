import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
T0 = 1_800_000_000


class Session:
    def __init__(self, directory):
        self.proc = subprocess.Popen(["sleep", "3600"])
        self.lines = []
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(directory / f"{self.proc.pid}.sock"))
        self.server.listen()
        threading.Thread(target=self.accept, daemon=True).start()

    def accept(self):
        while True:
            try:
                conn, _ = self.server.accept()
            except OSError:
                return
            with conn:
                data = b""
                while chunk := conn.recv(65536):
                    data += chunk
            self.lines += [json.loads(x)["message"]["content"] for x in data.decode().splitlines()]

    def kill(self):
        self.proc.kill()
        self.proc.wait()

    def close(self):
        self.server.close()
        if self.proc.poll() is None:
            self.kill()


class Org(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        socks = self.dir / "socks"
        socks.mkdir()
        self.lead, self.proxy = Session(socks), Session(socks)
        self.addCleanup(self.lead.close)
        self.addCleanup(self.proxy.close)
        self.codex = self.dir / "codex.log"
        self.restarts = self.dir / "agents.log"
        for name, log, code in (("codex-lead", self.codex, 0), ("agents", self.restarts, 0)):
            script = self.dir / name
            script.write_text(f'#!/usr/bin/env bash\nprintf "%s|%s\\n" "${{CODEX_LEAD_THREAD:-}}" "$*" >> {log}\nexit {code}\n')
            script.chmod(0o755)
        self.status = self.dir / "lead-status.md"
        self.org = {"version": 1, "proxy": {"pid": self.proxy.proc.pid}, "nodes": [
            {"id": "lead", "role": "lead", "domain": "north", "parent": "proxy", "provider": "claude", "session": "",
             "brief": "", "status_file": str(self.status), "depth": 1, "budget": 2, "state": "active",
             "pid": self.lead.proc.pid},
            {"id": "sub", "role": "sub-lead", "domain": "north", "parent": "lead", "provider": "codex",
             "session": "thread-1", "brief": "", "status_file": "", "depth": 2, "budget": 1, "state": "active"}]}
        (self.dir / "org.json").write_text(json.dumps(self.org))
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("AGENT")} | {
            "AGENTS_ORG_FILE": str(self.dir / "org.json"), "AGENTS_EVENTS_DB": str(self.dir / "events.db"),
            "AGENTS_CC_SOCKS": str(socks), "AGENTS_CODEX_SEND": str(self.dir / "codex-lead"),
            "AGENTS_BIN": str(self.dir / "agents"), "AGENTS_CODE_ROOT": str(self.dir / "code")}

    def agents(self, minutes, *argv, who=None):
        env = self.env | {"AGENTS_EVENTS_NOW": str(T0 + minutes * 60)} | ({"AGENT_ORG_NAME": who} if who else {})
        r = subprocess.run([str(ROOT / "bin/agents"), *argv], env=env, capture_output=True, text=True, timeout=60)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout

    def supervise(self, minutes):
        self.agents(minutes, "supervise", "--once")

    def git(self, *argv, cwd=None):
        return subprocess.run(["git", *argv], cwd=cwd, check=True, capture_output=True, text=True).stdout.strip()

    def test_stuck_wakes_the_live_parent_then_reaches_the_proxy_once_it_dies(self):
        self.agents(0, "event", "STUCK", "north#1", "--body", "need the schema", who="sub")
        self.assertIn("STUCK", self.status.read_text())
        self.supervise(0.5)
        self.assertEqual(len(self.lead.lines), 1)
        self.assertIn("north#1: need the schema", self.lead.lines[0])
        self.agents(1, "event", "ANSWER", "north#1", "--body", "use v2", who="lead")
        self.agents(2, "event", "STUCK", "north#2", "--body", "lease held", who="sub")
        self.lead.kill()
        self.supervise(2.5)
        self.supervise(21.9)
        self.assertEqual(self.proxy.lines, [])
        self.supervise(22)
        self.supervise(23)
        self.assertEqual(len(self.proxy.lines), 1)
        self.assertIn("STALL", self.proxy.lines[0])
        self.assertIn("north#2", self.proxy.lines[0])
        self.assertIn("north#2", (self.dir / "handoffs/proxy-inbox.md").read_text())
        self.assertEqual(self.codex.read_text().count("use v2"), 1)
        self.assertTrue(self.codex.read_text().startswith("thread-1|send ANSWER"))

    def test_land_ready_lane_pages_the_parent_then_the_proxy_until_main_has_it(self):
        origin, main = self.dir / "origin.git", self.dir / "code/north/main"
        self.git("init", "-q", "--bare", "-b", "main", str(origin))
        self.git("clone", "-q", str(origin), str(main))
        for n in (1, 2):
            self.git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", f"c{n}", cwd=main)
        landed, waiting = self.git("rev-parse", "HEAD~1", cwd=main), self.git("rev-parse", "HEAD", cwd=main)
        self.git("push", "-q", "origin", f"{landed}:refs/heads/main", cwd=main)
        self.agents(0, "event", "LAND-READY", "north#3", "--lane", "a", "--sha", landed, who="sub")
        self.agents(0, "event", "LAND-READY", "north#4", "--lane", "b", "--sha", waiting, who="sub")
        self.supervise(0)
        self.supervise(29.9)
        self.assertEqual(len(self.lead.lines), 2)
        self.supervise(30)
        self.assertEqual([("STALL" in x, "north#4" in x) for x in self.lead.lines[2:]], [(True, True)])
        self.supervise(59.9)
        self.assertEqual(self.proxy.lines, [])
        self.supervise(60)
        self.supervise(61)
        self.assertEqual([("STALL" in x, "north#4" in x) for x in self.proxy.lines], [(True, True)])
        self.assertEqual(len(self.lead.lines), 3)

    def test_recycle_ready_starts_one_successor_from_the_packet(self):
        packet = self.dir / "packet.md"
        packet.write_text("resume from here\n")
        self.agents(0, "event", "RECYCLE-READY", "north#5", "--packet", str(packet), who="sub")
        self.supervise(14.9)
        self.assertFalse(self.restarts.exists())
        self.supervise(15)
        self.supervise(16)
        self.assertEqual(self.restarts.read_text().splitlines(),
                         [f"|org set sub --brief {packet} --session ", "|lead restart sub"])
        self.assertIn("answered by supervisor", self.agents(17, "event", "list"))


if __name__ == "__main__":
    unittest.main()
