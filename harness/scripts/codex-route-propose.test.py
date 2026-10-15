import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "harness/scripts/codex-route-propose.py"
spec = importlib.util.spec_from_file_location("harness_learn", ROOT / "harness/scripts/harness-learn.py")
learn = importlib.util.module_from_spec(spec)
spec.loader.exec_module(learn)

NOW = datetime.now(timezone.utc).replace(microsecond=0)
ADOPTED = NOW - timedelta(days=7)
SECTIONS = {"tooling": "codex", "docs-policy": "claude"}


def iso(moment):
    return moment.strftime("%Y-%m-%dT%H:%M:%SZ")


def toml(values, note="fixture"):
    return "".join(f'[codex_route_{c}]\nvalue = "{v}"\nunit = "engine for {c} boxes: codex or claude"\nsource = "{note}"\n\n'
                   for c, v in values.items())


class CodexRoutePropose(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="codex-route-test-")
        self.root = Path(self.directory.name)
        self.repo, self.state, self.db = self.root / "repo", self.root / "state", self.root / "threads.db"
        self.env = dict(os.environ, THREADS_DB=str(self.db), XDG_STATE_HOME=str(self.root / "xdg"))
        for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
            self.env.pop(key, None)
        (self.repo / "harness").mkdir(parents=True)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Route fixture")
        self.git("config", "user.email", "route@example.invalid")
        self.commit({c: "claude" if v == "codex" else "codex" for c, v in SECTIONS.items()}, NOW - timedelta(days=30))
        self.commit(SECTIONS, ADOPTED)
        db = sqlite3.connect(self.db)
        db.execute("CREATE TABLE runs(agent TEXT PRIMARY KEY, item TEXT, follows TEXT, tier TEXT NOT NULL, category TEXT NOT NULL,"
                   " ended TEXT NOT NULL, outcome TEXT NOT NULL, expects_landing INTEGER, landed INTEGER,"
                   " role TEXT NOT NULL DEFAULT 'worker')")
        db.commit()
        db.close()
        self.serial = 0

    def tearDown(self):
        self.directory.cleanup()

    def git(self, *argv, when=None):
        env = dict(self.env, **({"GIT_AUTHOR_DATE": iso(when), "GIT_COMMITTER_DATE": iso(when)} if when else {}))
        done = subprocess.run(["git", *argv], cwd=self.repo, env=env, text=True, capture_output=True)
        self.assertEqual(done.returncode, 0, done.stderr)

    def commit(self, values, when, note="fixture"):
        (self.repo / "harness/defaults.toml").write_text(toml(values, note))
        self.git("add", ".")
        self.git("commit", "-m", f"routes at {iso(when)}", when=when)

    def runs(self, category, engine, ok, total, ended=NOW - timedelta(days=2)):
        db = sqlite3.connect(self.db)
        for i in range(total):
            self.serial += 1
            db.execute("INSERT INTO runs(agent, item, tier, category, ended, outcome, expects_landing, landed)"
                       " VALUES (?, ?, ?, ?, ?, ?, 1, 1)",
                       (f"w{self.serial}", f"north#{self.serial}", "gpt-6.1-sol high" if engine == "codex" else "high",
                        category, iso(ended), "done" if i < ok else "not done"))
        db.commit()
        db.close()

    def propose(self, category):
        done = subprocess.run([sys.executable, str(SCRIPT), "--repo", str(self.repo), "--state", str(self.state)],
                              env=self.env, text=True, capture_output=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        rows = [json.loads(line) for line in (self.state / f"proposals/codex-route-{category}.jsonl").read_text().splitlines()]
        self.assertEqual(len(rows), 1)
        return rows[0]

    def test_codex_wins_a_claude_routed_class(self):
        self.runs("docs-policy", "codex", 50, 60)
        self.runs("docs-policy", "claude", 30, 60)
        row = self.propose("docs-policy")
        self.assertEqual((row["dimension"], row["default"], row["current"], row["proposed"], row["n"], row["eligible"]),
                         ("codex-route-docs-policy", "codex_route_docs-policy", "claude", "codex", 60, True))
        self.assertEqual((row["since"], row["train_until"]), (iso(ADOPTED), iso(ADOPTED)))
        self.assertAlmostEqual(row["score_proposed"], 50 / 60 - 30 / 60 + 0.1, places=3)
        self.assertGreater(row["ci_low"], 0)
        self.assertLess(row["ci_low"], row["score_proposed"])
        self.assertGreater(row["ci_high"], row["score_proposed"])
        self.assertIn("codex 50/60", row["reason"])
        self.assertIn("claude 30/60", row["reason"])
        self.assertEqual(learn.judgment(row, 20, NOW + timedelta(minutes=1))[0], "won")
        print("PASS claude-routed class: codex 50/60 vs claude 30/60 proposes codex with ci_low > 0, judged won")

    def test_small_cell_waits(self):
        self.runs("docs-policy", "codex", 5, 5)
        self.runs("docs-policy", "claude", 0, 8)
        row = self.propose("docs-policy")
        self.assertEqual(row["n"], 5)
        self.assertEqual(learn.judgment(row, 20, NOW + timedelta(minutes=1)), ("waited", "insufficient evidence"))
        print("PASS 5 codex vs 8 claude runs: n=5, harness-learn waits")

    def test_runs_before_adoption_are_excluded(self):
        self.commit(SECTIONS, NOW - timedelta(days=5), note="source edited after adoption")
        self.runs("docs-policy", "codex", 50, 60)
        self.runs("docs-policy", "claude", 30, 60)
        self.runs("docs-policy", "claude", 40, 40, ended=ADOPTED - timedelta(days=1))
        self.runs("docs-policy", "codex", 0, 40, ended=ADOPTED - timedelta(days=1))
        row = self.propose("docs-policy")
        self.assertEqual((row["train_until"], row["n"]), (iso(ADOPTED), 60))
        self.assertIn("codex 50/60", row["reason"])
        self.assertIn("claude 30/60", row["reason"])
        print("PASS runs ended before the value-line commit are excluded; a source-only edit keeps train_until")

    def test_claude_wins_a_codex_routed_class(self):
        self.runs("tooling", "codex", 140, 200)
        self.runs("tooling", "claude", 180, 200)
        row = self.propose("tooling")
        self.assertEqual((row["current"], row["proposed"], row["n"]), ("codex", "claude", 200))
        self.assertAlmostEqual(row["score_proposed"], 0.9 - 0.7 - 0.1, places=3)
        self.assertGreater(row["ci_low"], 0)
        self.assertEqual(learn.judgment(row, 20, NOW + timedelta(minutes=1))[0], "won")
        print("PASS codex-routed class: claude 20 points better proposes claude with ci_low > 0 after the margin")


if __name__ == "__main__":
    unittest.main()
