#!/usr/bin/env python3
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import events
import landing_health


class LandingHealth(unittest.TestCase):
    def test_recorded_supervisor_git_activity_and_inbox_journey(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            repository = directory / "repos/fixture/main"
            repository.mkdir(parents=True)
            environment = dict(os.environ, AGENTS_REPO_ROOT=str(directory / "repos"),
                               AGENTS_ORG_FILE=str(directory / "org.json"),
                               THREADS_DB=str(directory / "events.db"), PYTHONDONTWRITEBYTECODE="1")
            real_git = shutil.which("git")

            def git(*arguments):
                result = subprocess.run([real_git, "-C", str(repository), *arguments],
                                        env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return result.stdout.strip()

            git("init", "-q")
            git("config", "user.name", "Recorded fixture")
            git("config", "user.email", "fixture@example.invalid")
            git("remote", "add", "origin", "https://github.com/tompassarelli/fixture.git")
            (repository / ".gitattributes").write_text("generated.tsv merge=fixture-generated\n")
            (repository / ".safe-push").write_text('[generated "declared.tsv"]\n input = primary.ts\n')
            (repository / "primary.ts").write_text("primary source\n")
            (repository / "declared.tsv").write_text("seed\n")
            (repository / "generated.tsv").write_text("seed\n")
            (repository / "arbitrary.txt").write_text("seed\n")

            def commit():
                git("add", ".")
                git("-c", "core.hooksPath=/dev/null", "commit", "-qm", "recorded input")
                return git("rev-parse", "HEAD")

            base = commit()
            (repository / "generated.tsv").write_text("".join(f"row{index}\n" for index in range(801)))
            (repository / "arbitrary.txt").write_text("".join(f"unmarked{index}\n" for index in range(900)))
            (repository / "declared.tsv").write_text("".join(f"declared{index}\n" for index in range(801)))
            first = commit()
            (repository / "generated.tsv").write_text("".join(f"changed{index}\n" for index in range(801)))
            (repository / "arbitrary.txt").write_text("".join(f"other{index}\n" for index in range(900)))
            (repository / "declared.tsv").write_text("".join(f"updated{index}\n" for index in range(801)))
            second = commit()
            git("update-ref", "refs/remotes/origin/main", second)
            now = time.time()
            rows = [dict(id=1, ref="refs/heads/main", before=base, after=second,
                         timestamp=now - 1900)]
            for index in range(4):
                (repository / "lane.txt").write_text(str(index))
                lane = commit()
                git("update-ref", f"refs/remotes/origin/claude/land-{index}", lane)
                rows.append(dict(id=index + 2, ref=f"refs/heads/claude/land-{index}", before="0" * 40,
                                 after=lane, timestamp=now - 1800))
                git("reset", "--hard", second)
            fixture = directory / "activity.json"

            def save_rows():
                copied = [dict(row, timestamp=time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(row["timestamp"]))) for row in rows]
                fixture.write_text(json.dumps(copied))

            save_rows()
            tools = directory / "tools"
            tools.mkdir()
            (tools / "gh").write_text(f"#!/bin/sh\ncat '{fixture}'\n")
            (tools / "git").write_text(f"#!/bin/sh\nfor arg in \"$@\"; do [ \"$arg\" != fetch ] || exit 0; done\nexec '{real_git}' \"$@\"\n")
            for executable in tools.iterdir():
                executable.chmod(0o755)
            environment["PATH"] = str(tools) + ":" + environment["PATH"]
            status = directory / "lead-status.md"
            Path(environment["AGENTS_ORG_FILE"]).write_text(json.dumps(dict(version=1, nodes=[dict(
                id="fixture-lead", domain="fixture", role="lead", provider="claude", state="active",
                status_file=str(status), session="recorded-native-recipient")])) )

            def supervise():
                result = subprocess.run([str(ROOT / "bin/agents"), "supervise", "--once"],
                                        env=environment, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")

            def pending():
                result = subprocess.run([str(ROOT / "bin/threads"), "agent-events"],
                                        env=environment, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                return json.loads(result.stdout)

            def refresh():
                state = directory / "landing-health.json"
                saved = json.loads(state.read_text())
                saved["fixture"]["attempted_at"] = 0
                state.write_text(json.dumps(saved))

            supervise()
            captured = pending()
            self.assertEqual(len(captured), 6)
            self.assertEqual({row["to_node"] for row in captured}, {"fixture-lead", "proxy"})
            self.assertTrue(all(row["delivered_at"] and not row["last_error"] for row in captured))
            self.assertEqual(sum("LANDING-STALL" in row["body"] for row in captured), 2)
            self.assertEqual(sum("REGEN-CHURN" in row["body"] for row in captured), 4)
            self.assertTrue(all("arbitrary.txt" not in row["body"] for row in captured))
            for inbox in (directory / "lead-inbox.md", directory / "handoffs/proxy-inbox.md"):
                self.assertIn("LANDING-STALL", inbox.read_text())
                self.assertIn("REGEN-CHURN", inbox.read_text())
            refresh()
            supervise()
            self.assertEqual(len(pending()), 6)
            (repository / "healthy.txt").write_text("first\n")
            third = commit()
            (repository / "healthy.txt").write_text("second\n")
            fourth = commit()
            git("update-ref", "refs/remotes/origin/main", fourth)
            rows.append(dict(id=6, ref="refs/heads/main", before=second, after=fourth, timestamp=time.time()))
            save_rows()
            refresh()
            supervise()
            saved = json.loads((directory / "landing-health.json").read_text())["fixture"]
            self.assertFalse(saved["episodes"]["LANDING-STALL"]["active"])
            self.assertEqual(sum(at >= time.time() - 1800 for at in saved["observation"]["arrivals"].values()), 2)
            (tools / "gh").write_text("#!/bin/sh\necho 'seeded API unavailable' >&2\nexit 1\n")
            refresh()
            supervise()
            self.assertEqual(sum("LANDING-HEALTH-SOURCE" in row["body"] for row in pending()), 2)
            refresh()
            supervise()
            self.assertEqual(len(pending()), 8)
            observation = dict(lanes={str(index): 0 for index in range(4)}, arrivals={}, changes={})
            self.assertNotIn("LANDING-STALL", landing_health.anomalies(observation, 1799))
            self.assertIn("LANDING-STALL", landing_health.anomalies(observation, 1800))


if __name__ == "__main__":
    unittest.main()
