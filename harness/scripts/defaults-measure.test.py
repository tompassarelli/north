#!/usr/bin/env python3
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone


HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "defaults-measure.py"
spec = importlib.util.spec_from_file_location("defaults_measure", SCRIPT)
M = importlib.util.module_from_spec(spec)
spec.loader.exec_module(M)


def cli(db, *args, script=SCRIPT):
    return subprocess.run([sys.executable, str(script), str(db), *args], capture_output=True, text=True)


def fixture(path):
    db = sqlite3.connect(path)
    db.execute("""CREATE TABLE runs (
        agent TEXT PRIMARY KEY, item TEXT, follows TEXT, tier TEXT, category TEXT,
        started TEXT, ended TEXT, minutes INTEGER, eta_min INTEGER, outcome TEXT,
        spec TEXT, scope TEXT, surface TEXT, verify TEXT, role TEXT,
        expects_landing INTEGER, landed INTEGER, landed_at TEXT, cost_usd REAL)""")
    start = datetime(2026, 9, 28, tzinfo=timezone.utc)
    for i in range(120):
        at = start + timedelta(hours=i * 3)
        ended = (at + timedelta(minutes=30)).isoformat().replace("+00:00", "Z")
        db.execute("INSERT INTO runs VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
                   (f"synthetic-{i}", "fixture#1", None, "codex:sol-high", "tooling",
                    at.isoformat().replace("+00:00", "Z"), ended, 5 + i % 7 * 5, 20,
                    "done", "measured", "module", "shell" if i % 3 else "api", "local-test", "worker",
                    int(i in (45, 85)), 0 if i in (45, 85) else 1, None, .1 + i % 3))
    db.execute("CREATE TABLE calibration (at TEXT, value REAL)")
    db.execute("CREATE TABLE usage (ts TEXT, used_pct REAL)")
    db.commit()
    db.close()


class TrainingBoundary(unittest.TestCase):
    def test_future_evidence_cannot_change_read_only_training_proposals(self):
        with tempfile.TemporaryDirectory(prefix="defaults-proposals-") as tmp:
            tmp = Path(tmp)
            path = tmp / "training.db"
            fixture(path)
            args = ("--propose", "--train-until", "2026-10-12", "--since", "2026-10-05", "--json")
            before = path.read_bytes()
            result = cli(path, *args)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stderr, "")
            self.assertEqual(path.read_bytes(), before)
            self.assertEqual(cli(path, *args).stdout, result.stdout)
            rows = {r["default"]: r for r in map(json.loads, result.stdout.splitlines())}
            from defaults import registry
            self.assertEqual(set(rows), {k for k, v in registry().items() if v["source"].startswith("measured:")})
            for name, row in rows.items():
                self.assertEqual(row["current"], registry()[name]["value"])
                self.assertTrue({"report", "proposed", "n", "score_current", "score_proposed", "train_until",
                                 "since", "objective", "direction", "eligible", "reason"} <= row.keys())
                json.dumps(row, allow_nan=False)
            self.assertFalse(rows["success_threshold"]["eligible"])
            self.assertIn("production replay required", rows["success_threshold"]["reason"])
            self.assertIn("untimestamped costs unavailable", rows["success_threshold"]["reason"])
            self.assertTrue(rows["eta_min_samples"]["eligible"])
            self.assertIsNone(rows["box_chain_gap_min"]["score_current"])
            self.assertEqual(rows["box_chain_gap_min"]["proposed"], rows["box_chain_gap_min"]["current"])

            cutoff, since = M.iso_stamp("2026-10-12"), M.iso_stamp("2026-10-05")
            training = M.training_db(path, cutoff)
            snapshot = tmp / "snapshot.db"
            with sqlite3.connect(snapshot) as out:
                training.backup(out)
            for name, fn, field in (("success_threshold", lambda: M.sweep_tier(training, since, "success_threshold"), "rate"),
                                    ("eta_min_samples", lambda: M.sweep_eta_min_samples(training, since), "median_abs_log_err"),
                                    ("eta_coverage", lambda: M.sweep_eta_coverage(training, since, M.T.DEFAULTS["overdue_ratio"]), "cost")):
                current = next(r for r in fn() if r["value"] == rows[name]["current"])
                self.assertEqual(rows[name]["score_current"], current[field])
            pairs = M.eta_pairs(training, M.T.DEFAULTS["eta_min_samples"], since)
            all_pairs = M.eta_pairs(training, M.T.DEFAULTS["eta_min_samples"], "")
            self.assertEqual(rows["eta_min_samples"]["n"], len(pairs))
            self.assertTrue(all(r["ended"] >= since for r, _ in pairs))
            self.assertEqual(pairs[0][1], next(eta for r, eta in all_pairs if r["ended"] == pairs[0][0]["ended"]))
            self.assertLess(len(pairs), len(all_pairs))
            with self.assertRaises(sqlite3.OperationalError):
                training.execute("DELETE FROM runs")
            training.close()

            with sqlite3.connect(path) as db:
                for i, at in enumerate(("2026-10-12T00:00:00Z", "2026-10-12T05:00:00+05:00",
                                        "2026-10-12T00:00:00.000001Z", "2026-10-14T00:00:00Z")):
                    db.execute("INSERT INTO runs SELECT ?, item, ?, tier, category, ?, ?, 10000, eta_min, 'not done',"
                               " spec, scope, surface, verify, role, 0, 0, NULL, 1000 FROM runs WHERE agent = 'synthetic-50'",
                               (f"future-{i}", None if i % 2 == 0 else "synthetic-70", at, at))
                db.execute("UPDATE runs SET landed = 1, landed_at = '2026-10-12T00:00:00Z' WHERE agent = 'synthetic-45'")
                db.execute("UPDATE runs SET landed = 1, landed_at = NULL WHERE agent = 'synthetic-85'")
                db.execute("UPDATE runs SET cost_usd = 999")
                db.execute("INSERT INTO calibration VALUES ('2026-10-12T00:00:00Z', 999)")
                db.execute("INSERT INTO usage VALUES ('2026-10-12T00:00:00Z', 100)")
            held_out_bytes = path.read_bytes()
            after = cli(path, *args)
            self.assertEqual(after.returncode, 0, after.stderr)
            self.assertEqual(after.stdout, result.stdout, "held-out/Follows/cost evidence leaked into training")
            self.assertEqual(path.read_bytes(), held_out_bytes)

            isolated = tmp / "isolated"
            (isolated / "harness/scripts").mkdir(parents=True)
            shutil.copytree(HERE.parent / "lib", isolated / "harness/lib", ignore=shutil.ignore_patterns("__pycache__"))
            (isolated / "bin").mkdir()
            shutil.copy2(M.ROOT / "bin/threads", isolated / "bin/threads")
            for name in ("defaults-measure.py", "policy_stats.py"):
                shutil.copy2(HERE / name, isolated / "harness/scripts" / name)
            defaults = (M.ROOT / "harness/defaults.toml").read_text()
            current = registry()["eta_min_samples"]["value"]
            off_grid_value = next(v for v in range(1, 20) if v not in (1, 2, 3, 5, 8, current))
            original = f"[eta_min_samples]\nvalue = {current}"
            self.assertEqual(defaults.count(original), 1)
            (isolated / "harness/defaults.toml").write_text(defaults.replace(original, f"[eta_min_samples]\nvalue = {off_grid_value}"))
            off_grid = cli(snapshot, "eta_min_samples", *args, script=isolated / "harness/scripts/defaults-measure.py")
            self.assertEqual(off_grid.returncode, 0, off_grid.stderr)
            off_grid_row = json.loads(off_grid.stdout)
            self.assertEqual(off_grid_row["current"], off_grid_value)
            with M.open_db(snapshot) as db:
                exact = M.sweep_eta_min_samples(db, since, (off_grid_value,))[0]
            self.assertEqual(off_grid_row["score_current"], exact["median_abs_log_err"])

            empty = tmp / "empty.db"
            fixture(empty)
            with sqlite3.connect(empty) as db:
                db.execute("DELETE FROM runs")
            empty_result = cli(empty, *args)
            self.assertEqual(empty_result.returncode, 0, empty_result.stderr)
            for row in map(json.loads, empty_result.stdout.splitlines()):
                self.assertIsNone(row["score_current"])
                self.assertIsNone(row["score_proposed"])
                self.assertEqual(row["current"], row["proposed"])
                self.assertFalse(row["eligible"])
            for flags in (("--propose", "--json"), ("--propose", "--train-until", "invalid", "--json"),
                          ("--propose", "--train-until", "2026-10-01", "--since", "2026-10-05", "--json")):
                invalid = cli(path, *flags)
                self.assertNotEqual(invalid.returncode, 0)
                self.assertEqual(invalid.stdout, "")
            print("training fixture sha256=" + hashlib.sha256(held_out_bytes).hexdigest())


if __name__ == "__main__":
    unittest.main()
