#!/usr/bin/env python3
from pathlib import Path
import sqlite3
import sys
import unittest
from datetime import datetime, timedelta, timezone

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pace_control
import plan_usage


class ResetPace(unittest.TestCase):
    def test_reset_observation_does_not_turn_one_percent_into_seventy_per_hour(self):
        now = datetime.now(timezone.utc)
        reset = (now + timedelta(days=7, seconds=-60)).isoformat()
        with sqlite3.connect(":memory:") as db:
            db.execute("CREATE TABLE usage(ts,provider,account,name,resets_at,used_pct)")
            db.executemany("INSERT INTO usage VALUES (?,?,?,?,?,?)", [
                ((now - timedelta(seconds=52)).isoformat(), "codex", "", "week", reset, 30),
                ((now - timedelta(hours=1)).isoformat(), "codex", "", "week", (now - timedelta(seconds=1)).isoformat(), 99)])
            row = dict(ts=now.isoformat(), provider="codex", account="", name="week", resets_at=reset,
                       used_pct=31, window_min=7 * 24 * 60)
            self.assertIsNone(plan_usage.beta(db, row, now))
            baseline = dict(window="week", used_pct=30, burn_pct_h=1.1, resets_at=reset,
                            reset_h=168, age_s=0, as_of=(now - timedelta(seconds=52)).isoformat())
            old = pace_control.advance({}, [baseline], 1, now.timestamp() - 52)
            sample = dict(baseline, used_pct=31, burn_pct_h=None, as_of=now.isoformat())
            new = pace_control.advance(old, [sample], 1, now.timestamp())
            self.assertEqual(new["windows"][0]["burn_pct_h"], 1.1)
            later = now + timedelta(seconds=548)
            row["ts"] = later.isoformat()
            self.assertAlmostEqual(plan_usage.beta(db, row, later), 6)


if __name__ == "__main__":
    unittest.main()
