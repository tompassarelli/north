#!/usr/bin/env python3
from contextlib import redirect_stdout
from importlib.machinery import SourceFileLoader
import io
import json
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
from datetime import datetime, timedelta

provenance = SourceFileLoader("provenance", str(Path(__file__).with_name("provenance"))).load_module()


class RecordedGalileoRows(unittest.TestCase):
    def test_galileo_plan_row_bar_is_found_and_a_measurement_is_not(self):
        self.assertEqual([bar[:3] for bar in provenance.bars(
            "| 9.11p | query view | the 1,000-row query keystroke stays under 50 ms | open |")], [("under", "50", "ms")])
        self.assertEqual(provenance.bars("| 9.11p | query view | measured 57.8 ms on the 1,000-row query | done |"), [])


class RecordedDebtLedger(unittest.TestCase):
    def test_due_lists_a_passed_revisit_and_omits_paid_and_future_debt(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / "galileo" / "main"
            repo.mkdir(parents=True)

            def git(*args):
                return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                                      cwd=repo, capture_output=True, text=True, check=True).stdout.strip()

            git("init", "-q", "-b", "main")
            git("commit", "--allow-empty", "-qm", "query view",
                "-m", "Target-debt: keystroke under 50 ms; measured 57.8 ms; owner Galileo; revisit 2026-01-01")
            git("commit", "--allow-empty", "-qm", "scroll view",
                "-m", "Target-debt: scroll under 16 ms; measured 19 ms; owner Galileo; revisit 2026-02-01")
            paid = git("rev-parse", "HEAD")
            git("commit", "--allow-empty", "-qm", "index scroll rows",
                "-m", f"Target-paid: {paid[:12]} measured 12 ms")
            git("commit", "--allow-empty", "-qm", "export",
                "-m", "Target-debt: export under 2 s; measured 3 s; owner Galileo; revisit 2999-01-01")
            git("update-ref", "refs/remotes/origin/main", "HEAD")
            out = io.StringIO()
            with redirect_stdout(out):
                provenance.main(["debt", "--due", "--repo", str(repo)])
            self.assertEqual([row.split("\t")[2:] for row in out.getvalue().splitlines()],
                             [["keystroke under 50 ms", "57.8 ms", "Galileo", "2026-01-01"]])


class RecordedSteeringAudit(unittest.TestCase):
    def test_steering_records_refuse_missing_grounding_and_rank_revisit_evidence(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent.parent) as tmp:
            repo = Path(tmp)
            command = str(Path(__file__).with_name("provenance").resolve())

            def git(*args):
                return subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                                      cwd=repo, capture_output=True, text=True, check=True).stdout.strip()

            def run(*args):
                return subprocess.run([command, *args], cwd=repo, capture_output=True, text=True)

            git("init", "-q", "-b", "main")
            (repo / "AGENTS.md").write_text("# Policy\n- Preserve main checkouts.\n")
            (repo / "old/hooks").mkdir(parents=True)
            (repo / "old/hooks/retained.sh").write_text("#!/bin/sh\nexit 3\n")
            git("add", "AGENTS.md", "old")
            git("commit", "-qm", "base")
            base = git("rev-parse", "HEAD")
            (repo / "AGENTS.md").write_text("# Policy\n- Preserve main checkouts.\n- Never force-push.\n")
            (repo / "harness/hooks").mkdir(parents=True)
            (repo / "harness/hooks/steering.sh").write_text("#!/bin/sh\nexit 2\n")
            (repo / "catalog-config.json").write_text('{"kind":"hook","name":"steering"}\n')
            (repo / "feature.py").write_text("raise RuntimeError('ordinary product code')\n")
            git("add", "AGENTS.md", "harness", "catalog-config.json", "feature.py")
            git("commit", "-qm", "unrecorded steering")
            missing = run("check", base, "HEAD")
            self.assertEqual(missing.returncode, 1, missing.stderr)
            self.assertEqual({line.split("\t")[0] for line in missing.stdout.splitlines()},
                             {"AGENTS.md", "harness/hooks/steering.sh", "catalog-config.json"})
            invalid = "Provenance: AGENTS.md harness/hooks/steering.sh catalog-config.json; origin experiment; rationale not recorded"
            git("commit", "--amend", "-qm", "incomplete steering record", "-m", invalid)
            self.assertEqual(run("check", base, "HEAD").returncode, 1)
            grounded = ("Provenance: AGENTS.md harness/hooks/steering.sh catalog-config.json; origin incident; "
                        "pointer north#40; rationale docs/rationale/targets.md#soft-targets; revisit 2999-01-01")
            git("commit", "--amend", "-qm", "malformed rationale", "-m",
                grounded.replace("docs/rationale/targets.md#soft-targets", "unrecorded guess"))
            self.assertEqual(run("check", base, "HEAD").returncode, 1)
            git("commit", "--amend", "-qm", "grounded steering", "-m", grounded)
            self.assertEqual(run("check", base, "HEAD").returncode, 0)
            (repo / "AGENTS.md").write_text("# Renamed heading\n- Never force-push.\n- Preserve main checkouts.\n")
            git("commit", "-qam", "move existing policy")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 0)
            (repo / "AGENTS.md").write_text("# Policy\nOrdinary introductory prose.\n```\n- Example command.\n```\n")
            (repo / "harness/SKILL.md").write_text("# Moved policy\n1. Never\n   force-push.\n2. Preserve main\n   checkouts.\n")
            (repo / "product/hooks").mkdir(parents=True)
            (repo / "product/hooks/example.sh").write_text("#!/bin/sh\nexit 0\n")
            (repo / "harness/policy-owners.toml").write_text('guard = [{ unit = "steering", command = "steering.sh" }]\n')
            (repo / "old/hooks/retained.sh").write_text("#!/bin/sh\nprintf ''\nexit 3\n")
            git("mv", "old/hooks/retained.sh", "harness/hooks/retained.sh")
            git("add", "AGENTS.md", "harness", "product")
            git("commit", "-qm", "move and rewrap rules with ordinary prose and fenced examples")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 0)
            (repo / "harness/SKILL.md").write_text("# Policy\n1. Never force-push.\n2. Preserve main checkouts.\n")
            git("commit", "-qam", "unwrap existing rules")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 0)
            with (repo / "harness/SKILL.md").open("a") as f:
                f.write("3. Refuse unreviewed policy.\n")
            git("commit", "-qam", "net-new numbered rule")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 1)
            git("commit", "--amend", "-qm", "restore policy", "-m", f"This reverts commit {base}.")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 0)
            git("commit", "--amend", "-qm", "quote sheriff trailer", "-m", f"Quoted Sheriff-Reverts: {base}")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 1)
            git("commit", "--amend", "-qm", "sheriff restores policy", "-m", f"Sheriff-Reverts: {base}")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 0)
            (repo / "PLAN.md").write_text("| Box | Done when |\n| query | keystroke under 50 ms |\n")
            git("add", "PLAN.md")
            git("commit", "-qm", "unrecorded bar")
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 1)
            soft = ("Provenance: keystroke under 50 ms; origin none; pointer recorded galileo row text in tests; "
                    "rationale not recorded; revisit 2999-01-01; soft")
            git("commit", "--amend", "-qm", "soft bar", "-m", soft)
            self.assertEqual(run("check", "HEAD^", "HEAD").returncode, 0)
            source = Path(__file__).resolve().parent.parent / "harness/lib/routing-scenarios/measured.json"
            now = datetime.fromisoformat(json.loads(source.read_text())["now"].replace("Z", "+00:00"))
            released = (now - timedelta(days=1)).isoformat()
            old = (now - timedelta(days=2)).isoformat()
            db = repo / "models.db"
            with sqlite3.connect(db) as c:
                c.execute("CREATE TABLE model_events(ts TEXT, model TEXT, kind TEXT, at TEXT, p REAL, source TEXT, detail TEXT)")
                c.execute("INSERT INTO model_events VALUES (?, ?, 'released', ?, NULL, 'fixture', '{}')",
                          (now.isoformat(), "openai:gpt-6.2-sol", released))
                c.execute("INSERT INTO model_events VALUES (?, ?, 'released', NULL, NULL, 'fixture', '{}')",
                          (now.isoformat(), "openai:gpt-6.1-sol"))
            for claim, n, at, model, revisit in (
                    ("fired", 2, now.isoformat(), "gpt-6.2-sol", "2026-01-01"),
                    ("single", 1, now.isoformat(), "gpt-6.2-sol", "2999-01-01"),
                    ("older", 2, old, "gpt-6.2-sol", "2999-01-01"),
                    ("unknown-release", 2, old, "gpt-6.1-sol", "2999-01-01")):
                pointer = f"{claim}.json"
                if claim == "single":
                    pointer = f"{claim}.jsonl"
                    (repo / pointer).write_text(json.dumps({"trial": 0, "pass": True, "model": model}) + "\n")
                elif claim == "older":
                    pointer = f"{claim}.toml"
                    (repo / pointer).write_text(f'n = {n}\ndate = "{at}"\nmodel = "{model}"\n')
                else:
                    (repo / pointer).write_text(json.dumps({"n": n, "date": at, "model": model}))
                git("commit", "--allow-empty", "-qm", claim, "-m",
                    f"Provenance: {claim}; origin experiment; pointer {pointer} plus rerun; "
                    f"rationale docs/rationale/targets.md#soft-targets; revisit {revisit}")
            git("commit", "--allow-empty", "-qm", "unknown origin", "-m",
                "Provenance: unknown; origin unknown; pointer north#40; revisit 2999-01-01")
            git("commit", "--allow-empty", "-qm", "event", "-m",
                "Provenance: event; origin incident; pointer north#40; rationale docs/rationale/targets.md#soft-targets; revisit measured event")
            git("update-ref", "refs/remotes/origin/main", "HEAD")
            output = run("audit", "--repo", str(repo), "--db", str(db), "--event", "measured event")
            self.assertEqual(output.returncode, 0, output.stderr)
            rows = [dict(zip(output.stdout.splitlines()[0].split("\t"), row.split("\t")))
                    for row in output.stdout.splitlines()[1:]]
            self.assertEqual([int(r["priority"]) for r in rows], [1, 1, 2, 2, 3, 4, 5, 5])
            by_claim = {r["claim"]: r for r in rows}
            self.assertEqual(by_claim["unknown"]["origin"], "none recorded")
            self.assertEqual(by_claim["unknown"]["rationale"], "rationale not recorded")
            self.assertEqual(by_claim["single"]["runs"], "1")
            self.assertEqual(by_claim["older"]["release_date"], released)
            self.assertEqual(by_claim["unknown-release"]["release_date"], "")
            without_event = run("audit", "--repo", str(repo), "--db", str(db))
            self.assertIn("\t5\t\tevent\t", without_event.stdout)


if __name__ == "__main__":
    unittest.main()
