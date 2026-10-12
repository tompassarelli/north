#!/usr/bin/env python3
from contextlib import redirect_stdout
from importlib.machinery import SourceFileLoader
import io
from pathlib import Path
import subprocess
import tempfile
import unittest

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


if __name__ == "__main__":
    unittest.main()
