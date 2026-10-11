#!/usr/bin/env python3
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "socrates/lib"))
from chrome_tabs import receive

FAKE_CLAUDE = r'''#!/usr/bin/env python3
import json, os, subprocess, sys, time
TOOLS = [{"name": n, "inputSchema": {"type": "object"}} for n in
         ("tabs_context_mcp", "tabs_create_mcp", "tabs_close_mcp", "browser_batch")]
if "--claude-in-chrome-mcp" in sys.argv:
    for line in sys.stdin:
        request = json.loads(line)
        result = {"tools": TOOLS} if request["method"] == "tools/list" else {}
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
    sys.exit(0)
server = json.loads(sys.argv[sys.argv.index("--mcp-config") + 1])["mcpServers"]["claude-in-chrome"]
mcp = subprocess.Popen([server["command"], *server["args"]], stdin=subprocess.PIPE,
                       stdout=subprocess.PIPE, text=True)
transcript = [("initialize", {}), ("tools/list", {}),
              ("tools/call", {"name": "tabs_context_mcp", "arguments": {"createIfEmpty": True}}),
              ("tools/call", {"name": "tabs_create_mcp", "arguments": {}}),
              ("tools/call", {"name": "tabs_create_mcp", "arguments": {}})]
for index, (method, params) in enumerate(transcript):
    mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": index, "method": method, "params": params}) + "\n")
    mcp.stdin.flush()
    assert "result" in json.loads(mcp.stdout.readline())
open(os.environ["FAKE_READY"], "w").close()
if os.environ["FAKE_END"] != "exit":
    time.sleep(600)
'''


class FakeExtension:
    def __init__(self, path):
        self.group, self.next_id, self.closed = None, 40, []
        self.tabs = {5: None}
        self.server = socket.socket(socket.AF_UNIX)
        self.server.bind(str(path))
        self.server.listen()
        threading.Thread(target=self.serve, daemon=True).start()

    def text(self, value):
        return {"content": [{"type": "text", "text": value}]}

    def answer(self, tool, args):
        if tool == "tabs_context_mcp":
            if self.group is None and args.get("createIfEmpty"):
                self.group, self.tabs[self.next_id], self.next_id = 7, 7, self.next_id + 1
            ids = [tab for tab, group in self.tabs.items() if group == self.group is not None]
            return self.text(json.dumps({"availableTabs": [{"tabId": tab} for tab in ids],
                                         "tabGroupId": self.group}))
        if tool == "tabs_create_mcp":
            tab, self.next_id = self.next_id, self.next_id + 1
            self.tabs[tab] = self.group
            if tab == 42:
                self.tabs[99] = self.group
            return self.text(f"Created new tab. Tab ID: {tab}")
        if tool == "tabs_close_mcp":
            self.closed.append(args["tabId"])
            self.tabs.pop(args["tabId"], None)
            return self.text("Closed")
        raise ValueError(tool)

    def serve(self):
        while True:
            client, _ = self.server.accept()
            with client:
                request = json.loads(receive(client, struct.unpack("<I", receive(client, 4))[0]))
                params = request["params"]
                reply = json.dumps({"tool_use_id": params["tool_use_id"],
                                    "result": self.answer(params["tool"], params["args"])}).encode()
                client.sendall(struct.pack("<I", len(reply)) + reply)


class RecordedSessionReaping(unittest.TestCase):
    def test_recorded_tabs_close_after_exit_sigterm_and_sigkill(self):
        for end in ("exit", "sigterm", "sigkill"):
            with self.subTest(end=end), tempfile.TemporaryDirectory() as scratch:
                scratch = Path(scratch)
                user = f"chrome-tabs-test-{os.getpid()}-{end}"
                bridge = Path("/tmp") / f"claude-mcp-browser-bridge-{user}"
                bridge.mkdir(mode=0o700)
                bin_dir = scratch / "bin"
                bin_dir.mkdir()
                (bin_dir / "claude").write_text(FAKE_CLAUDE)
                (bin_dir / "claude").chmod(0o755)
                state = scratch / "state/agents/chrome-tabs"
                try:
                    extension = FakeExtension(bridge / "1.sock")
                    ready = scratch / "ready"
                    env = {**os.environ, "USER": user, "XDG_STATE_HOME": str(scratch / "state"),
                           "PATH": f"{bin_dir}:{os.environ['PATH']}", "FAKE_READY": str(ready),
                           "FAKE_END": end}
                    launcher = subprocess.Popen([str(HERE / "chrome-tabs"), "run", "--", "claude", "--chrome"],
                                                env=env, start_new_session=True)
                    deadline = time.monotonic() + 20
                    while not ready.exists() and time.monotonic() < deadline:
                        time.sleep(0.05)
                    self.assertTrue(ready.exists())
                    if end == "sigterm":
                        launcher.send_signal(signal.SIGTERM)
                    elif end == "sigkill":
                        os.killpg(launcher.pid, signal.SIGKILL)
                    launcher.wait(timeout=10)
                    deadline = time.monotonic() + 10
                    while list(state.glob("*.json")) and time.monotonic() < deadline:
                        time.sleep(0.1)
                    self.assertEqual(sorted(extension.closed), [40, 41, 42])
                    self.assertEqual(list(state.glob("*.json")), [])
                finally:
                    for record in state.glob("*.json"):
                        subprocess.run(["systemctl", "--user", "stop", f"agent-chrome-tabs-{record.stem}"])
                    for path in bridge.iterdir():
                        path.unlink()
                    bridge.rmdir()


if __name__ == "__main__":
    unittest.main()
