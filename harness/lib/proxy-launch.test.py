import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ProxyLaunchTest(unittest.TestCase):
    def test_proxy_reaches_first_turn_without_a_trust_prompt_and_rejects_stalled_startup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home, profile, fake = root / "home", root / "profile", root / "bin"
            for directory in (home, profile, fake):
                directory.mkdir()
            (home / ".claude.json").write_text(json.dumps({"projects": {
                str(home): {"hasTrustDialogAccepted": True}}}))
            (profile / ".credentials.json").write_text("{}")
            label = hashlib.sha256(b"fixture-account").hexdigest()[:8]
            policy = root / "orchestration.toml"
            policy.write_text(f'[accounts.claude]\nlabels = {{ "{label}" = "a" }}\n'
                              f'profiles = {{ a = {{ "{socket.gethostname()}" = {{ path = "{profile}" }} }} }}\n')
            brief = root / "brief.md"
            brief.write_text("First proxy turn fixture.")
            stubs = {
                "spawn-quiet": '#!/usr/bin/env python3\nimport os,sys\n'
                               'args=sys.argv[sys.argv.index("-e")+1:]\nos.execvp(args[0],args)\n',
                "tmux": '#!/usr/bin/env bash\nexit 1\n',
                "claude": '''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
profile=Path(os.environ["CLAUDE_CONFIG_DIR"])
cwd=os.getcwd()
config=json.loads((profile/".claude.json").read_text())
if not config["projects"].get(cwd,{}).get("hasTrustDialogAccepted"):
    sys.exit("TRUST PROMPT")
session=sys.argv[sys.argv.index("--session-id")+1]
path=profile/"projects"/"fixture"/(session+".jsonl")
path.parent.mkdir(parents=True,exist_ok=True)
rows=[{"type":"assistant","message":{"content":"first reply"}}]
if os.environ["FIXTURE_COMPLETE"]=="1":
    rows.append({"type":"system","subtype":"turn_duration","durationMs":1})
path.write_text("".join(json.dumps(row)+"\\n" for row in rows))
Path(os.environ["CAPTURE"]).write_text(json.dumps({"session":session,"cwd":cwd,"role":os.environ["AGENT_ROLE"]}))
''',
            }
            for name, contents in stubs.items():
                path = fake / name
                path.write_text(contents)
                path.chmod(0o755)
            env = dict(os.environ, HOME=str(home), CLAUDE_CONFIG_DIR=str(profile),
                       AGENTS_ORCHESTRATION=str(policy), AGENTS_ORG_FILE=str(root / "org.json"),
                       AGENTS_SESSION_HOST="ghostty", CAPTURE=str(root / "capture.json"),
                       PATH=f"{fake}:{ROOT / 'bin'}:{os.environ['PATH']}")
            for complete in (True, False):
                with self.subTest(completed=complete):
                    (profile / ".claude.json").write_text(json.dumps({"oauthAccount": {
                        "accountUuid": "fixture-account"}, "projects": {}}))
                    result = subprocess.run([str(ROOT / "bin/agents"), "org", "start", "proxy",
                                             "--brief", str(brief), "--account", "a", "--cwd", str(home),
                                             "--timeout", "0.1"],
                                            env=env | {"FIXTURE_COMPLETE": str(int(complete))},
                                            capture_output=True, text=True, timeout=5)
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


if __name__ == "__main__":
    unittest.main()
