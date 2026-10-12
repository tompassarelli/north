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
sys.path.insert(0, str(HERE.parent / "harness/lib"))
from chrome_tabs import SAFETY_PENDING, call, create, receive

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
server = json.loads(sys.argv[sys.argv.index("--mcp-config") + 1])["mcpServers"]["chrome-tabs"]
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
    def __init__(self, path, tabs, sibling):
        self.group, self.next_id, self.closed, self.calls = None, 40, [], []
        self.tabs, self.sibling, self.errors = tabs, sibling, {}
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
            if tab == 42 and self.sibling:
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
                self.calls.append(params["tool"])
                queued = self.errors.get(params["tool"])
                outcome = ({"error": queued.pop(0)} if queued else
                           {"result": self.answer(params["tool"], params["args"])})
                reply = json.dumps({"tool_use_id": params["tool_use_id"], **outcome}).encode()
                client.sendall(struct.pack("<I", len(reply)) + reply)


FAKE_CHROME = "#!/bin/sh\ntouch {marker}\n"


class RecordedSessionReaping(unittest.TestCase):
    def session(self, end, tabs=None, sibling=True, chrome_absent=False):
        with tempfile.TemporaryDirectory() as scratch:
            scratch = Path(scratch)
            user = f"chrome-tabs-test-{os.getpid()}-{end}-{chrome_absent}"
            bridge = Path("/tmp") / f"claude-mcp-browser-bridge-{user}"
            bridge.mkdir(mode=0o700)
            bin_dir = scratch / "bin"
            bin_dir.mkdir()
            started = scratch / "chrome-started"
            for name, text in (("claude", FAKE_CLAUDE), ("google-chrome-stable", FAKE_CHROME.format(marker=started))):
                (bin_dir / name).write_text(text)
                (bin_dir / name).chmod(0o755)
            state = scratch / "state/agents/chrome-tabs"
            tabs = {5: None} if tabs is None else tabs
            try:
                extension = None if chrome_absent else FakeExtension(bridge / "1.sock", tabs, sibling)
                ready = scratch / "ready"
                env = {**os.environ, "USER": user, "XDG_STATE_HOME": str(scratch / "state"),
                       "PATH": f"{bin_dir}:{os.environ['PATH']}", "FAKE_READY": str(ready),
                       "FAKE_END": end}
                launcher = subprocess.Popen([str(HERE / "chrome-tabs"), "run", "--", "claude", "--chrome"],
                                            env=env, start_new_session=True)
                deadline = time.monotonic() + 20
                while extension is None and time.monotonic() < deadline:
                    if started.exists():
                        extension = FakeExtension(bridge / "1.sock", tabs, sibling)
                    time.sleep(0.05)
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
                self.assertEqual(list(state.glob("*.json")), [])
                return extension
            finally:
                for record in state.glob("*.json"):
                    subprocess.run(["systemctl", "--user", "stop", f"agent-chrome-tabs-{record.stem}"])
                for path in bridge.iterdir():
                    path.unlink()
                bridge.rmdir()

    def test_recorded_tabs_close_after_exit_sigterm_and_sigkill(self):
        for end in ("exit", "sigterm", "sigkill"):
            with self.subTest(end=end):
                extension = self.session(end)
                self.assertEqual(sorted(extension.closed), [40, 41, 42])
                self.assertNotIn(43, extension.tabs)

    def test_session_owning_every_tab_in_the_only_window_leaves_a_blank_tab(self):
        extension = self.session("exit", tabs={}, sibling=False)
        if os.environ.get("CHROME_TABS_DRY_RUN_LOG"):
            Path(os.environ["CHROME_TABS_DRY_RUN_LOG"]).write_text("\n".join(extension.calls) + "\n")
        self.assertEqual(sorted(extension.closed), [40, 41, 42])
        self.assertEqual(list(extension.tabs), [43])
        calls = extension.calls
        self.assertLess(len(calls) - 1 - calls[::-1].index("tabs_create_mcp"), calls.index("tabs_close_mcp"))

    def test_run_starts_chrome_when_no_native_host_exists(self):
        self.assertEqual(sorted(self.session("exit", chrome_absent=True).closed), [40, 41, 42])


class NativeHostErrors(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        socket_path = Path(scratch.name) / "1.sock"
        self.extension = FakeExtension(socket_path, {}, False)
        self.path = Path(scratch.name) / "session.json"
        create(self.path, {"group_key": "chat:test", "native_socket": str(socket_path),
                           "native_inode": socket_path.stat().st_ino, "tabs": [], "group_id": None})
        self.slept = []

    def navigate(self):
        return call(self.path, "navigate", {"url": "https://example.com"}, self.slept.append)

    def test_safety_check_waits_with_backoff_until_chrome_answers(self):
        self.extension.errors["navigate"] = [{"content": SAFETY_PENDING + ". Blocking as a precaution"}] * 3
        self.extension.answer = lambda tool, args: self.extension.text("Navigated")
        self.assertEqual(self.navigate()["content"][0]["text"], "Navigated")
        self.assertEqual(self.slept, [1, 2, 4])

    def test_safety_check_reports_clearly_after_about_two_minutes(self):
        self.extension.errors["navigate"] = [{"content": SAFETY_PENDING + ". Blocking as a precaution"}] * 20
        result = self.navigate()
        self.assertTrue(result["isError"])
        self.assertIn("120s", result["content"][0]["text"])
        self.assertEqual(sum(self.slept), 120)
        self.assertEqual(len(self.extension.errors["navigate"]), 11)


if __name__ == "__main__":
    unittest.main()
