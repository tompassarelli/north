import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "harness/lib"))
import org


class ProxyLaunchTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = root = Path(self.temporary.name)
        home, profile, fake = root / "home", root / "profile", root / "bin"
        self.home, self.profile = home, profile
        for directory in (home, profile, fake):
            directory.mkdir()
        (home / ".claude.json").write_text(json.dumps({"projects": {
            str(home): {"hasTrustDialogAccepted": True}}}))
        (profile / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {
            "accessToken": "fixture-token"}}))
        (profile / "settings.json").write_text(json.dumps({"skipDangerousModePermissionPrompt": True}))
        self.config = profile / ".claude.json"
        self.config.write_text(json.dumps({"oauthAccount": {"accountUuid": "fixture-account"}, "projects": {}}))
        label = hashlib.sha256(b"fixture-account").hexdigest()[:8]
        policy = root / "orchestration.toml"
        policy.write_text(f'[accounts.claude]\nlabels = {{ "{label}" = "a" }}\n'
                          f'profiles = {{ a = {{ "{socket.gethostname()}" = {{ path = "{profile}" }} }} }}\n')
        self.brief = root / "brief.md"
        self.brief.write_text("First proxy turn fixture.")
        stubs = {
            "spawn-quiet": '''#!/usr/bin/env python3
import os,subprocess,sys
from pathlib import Path
args=sys.argv[sys.argv.index("-e")+1:]
if os.environ.get("FIXTURE_DEAD")=="1":
    os.execvp(args[0],args)
process=subprocess.Popen(args,start_new_session=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
Path(os.environ["LAUNCHER_PID"]).write_text(str(process.pid))
''',
            "tmux": '#!/usr/bin/env bash\nexit 1\n',
            "claude": '''#!/usr/bin/env python3
import json,os,signal,sys
from pathlib import Path
profile=Path(os.environ["CLAUDE_CONFIG_DIR"])
cwd=os.getcwd()
config=json.loads((profile/".claude.json").read_text())
if not config["projects"].get(cwd,{}).get("hasTrustDialogAccepted"):
    sys.exit("TRUST PROMPT")
if os.environ.get("FIXTURE_GATES")=="1" and (not config.get("hasCompletedOnboarding") or not config.get("theme")):
    sys.exit("ONBOARDING PROMPT")
session=sys.argv[sys.argv.index("--session-id")+1]
path=profile/"projects"/"fixture"/(session+".jsonl")
path.parent.mkdir(parents=True,exist_ok=True)
rows=[{"type":"assistant","message":{"content":"first reply"}}]
if os.environ["FIXTURE_COMPLETE"]=="1":
    rows.append({"type":"system","subtype":"turn_duration","durationMs":1})
path.write_text("".join(json.dumps(row)+"\\n" for row in rows))
Path(os.environ["CAPTURE"]).write_text(json.dumps({"session":session,"cwd":cwd,"role":os.environ["AGENT_ROLE"]}))
if os.environ.get("FIXTURE_DEAD")!="1":
    signal.pause()
''',
        }
        for name, contents in stubs.items():
            path = fake / name
            path.write_text(contents)
            path.chmod(0o755)
        self.env = dict(os.environ, HOME=str(home), CLAUDE_CONFIG_DIR=str(profile),
                        AGENTS_ORCHESTRATION=str(policy), AGENTS_ORG_FILE=str(root / "org.json"),
                        AGENTS_SESSION_HOST="ghostty", CAPTURE=str(root / "capture.json"),
                        LAUNCHER_PID=str(root / "launcher.pid"), FIXTURE_COMPLETE="1",
                        PATH=f"{fake}:{ROOT / 'bin'}:{os.environ['PATH']}")
        for key in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN"):
            self.env.pop(key, None)
        self.addCleanup(self.close_launcher)

    def close_launcher(self):
        path = self.root / "launcher.pid"
        if path.exists():
            try:
                os.killpg(int(path.read_text()), signal.SIGKILL)
            except ProcessLookupError:
                pass
            path.unlink()

    def start(self, env=None, account=True, extra=()):
        return subprocess.run([str(ROOT / "bin/agents"), "org", "start", "proxy",
                               "--brief", str(self.brief), "--cwd", str(self.home), "--timeout", "0.3",
                               *(["--account", "a"] if account else []), *extra],
                              env=self.env | (env or {}), capture_output=True, text=True, timeout=5)

    def test_proxy_reaches_first_turn_without_a_trust_prompt_and_rejects_stalled_startup(self):
        root, home, profile = self.root, self.home, self.profile
        for complete in (True, False):
            with self.subTest(completed=complete):
                result = self.start({"FIXTURE_COMPLETE": str(int(complete))})
                self.assertNotIn("TRUST PROMPT", result.stderr)
                capture = json.loads((root / "capture.json").read_text())
                self.assertEqual((capture["cwd"], capture["role"]), (str(home), "proxy"))
                if complete:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("first turn completed", result.stdout)
                    proxy = json.loads((root / "org.json").read_text())["proxy"]
                    self.assertEqual((proxy["session"], proxy["account"]), (capture["session"], "a"))
                else:
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn("timed out waiting for its first completed turn", result.stderr)
                    self.assertNotIn("proxy started", result.stdout)
                self.close_launcher()

    def test_proxy_prepares_ui_gates_and_refuses_unaccepted_permissions_or_missing_auth(self):
        result = self.start({"FIXTURE_GATES": "1"})
        self.assertEqual(result.returncode, 0, result.stderr)
        config = json.loads(self.config.read_text())
        self.assertTrue(config["hasCompletedOnboarding"])
        self.assertEqual(config["theme"], "dark")
        self.assertNotIn("bypassPermissionsModeAccepted", config)
        self.close_launcher()
        self.config.unlink()
        result = self.start({"FIXTURE_GATES": "1"}, account=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.close_launcher()
        credentials = self.profile / ".credentials.json"
        settings = self.profile / "settings.json"
        for gate in ("authentication", "bypassPermissions", "API key confirmation", "auto"):
            with self.subTest(gate=gate):
                credentials.write_text(json.dumps({"claudeAiOauth": {"accessToken": "fixture-token"}}))
                settings.write_text(json.dumps({"skipDangerousModePermissionPrompt": gate != "bypassPermissions"}))
                if gate == "authentication":
                    credentials.write_text("{}")
                env = {"ANTHROPIC_API_KEY": "fixture-unapproved-api-key"} if gate == "API key confirmation" else {}
                result = self.start(env, account=False,
                                    extra=("--", "--permission-mode", "auto") if gate == "auto" else ())
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(gate, result.stderr)
                self.assertFalse((self.root / "launcher.pid").exists())
                self.assertNotIn("proxy started", result.stdout)
                self.assertNotIn("fixture-unapproved-api-key", result.stderr)
        self.assertNotIn("bypassPermissionsModeAccepted", json.loads(self.config.read_text()))

    def test_trust_preserves_concurrent_config_updates_under_the_runtime_lock(self):
        original = Path.read_text
        raced = False
        locked = False

        def racing_read(path, *args, **kwargs):
            nonlocal raced, locked
            data = original(path, *args, **kwargs)
            if path == self.config and not raced:
                raced = True
                locked = path.with_name(path.name + ".lock").is_dir()
                current = json.loads(data)
                current["runningSessionValue"] = "new-write"
                current["projects"]["concurrent-project"] = {"hasTrustDialogAccepted": True}
                path.write_text(json.dumps(current))
            return data

        with patch.object(Path, "home", return_value=self.home), patch.object(Path, "read_text", racing_read):
            org.trust(str(self.home / "code/project"), self.profile)
        config = json.loads(self.config.read_text())
        self.assertEqual(config["runningSessionValue"], "new-write")
        self.assertTrue(config["projects"]["concurrent-project"]["hasTrustDialogAccepted"])
        self.assertTrue(config["projects"][str(self.home / "code/project")]["hasTrustDialogAccepted"])
        self.assertTrue(locked)
        self.assertFalse(self.config.with_name(self.config.name + ".lock").exists())

    def test_proxy_refuses_a_completed_turn_from_an_exited_session(self):
        result = self.start({"FIXTURE_DEAD": "1"})
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("exited", result.stderr)
        self.assertNotIn("proxy started", result.stdout)
        proxy = json.loads((self.root / "org.json").read_text())["proxy"]
        self.assertTrue(proxy.get("died"))


if __name__ == "__main__":
    unittest.main()
