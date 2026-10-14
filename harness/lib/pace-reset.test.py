#!/usr/bin/env python3
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pace_control
import plan_usage


class ResetPace(unittest.TestCase):
    def test_exhausted_week_stays_held_without_a_burn_estimate(self):
        now = datetime.now(timezone.utc)
        for used in (97, 99):
            window = dict(window="week", used_pct=used, burn_pct_h=None, reset_h=12,
                          resets_at=(now + timedelta(hours=12)).isoformat(), age_s=0, as_of=now.isoformat())
            control = pace_control.advance({}, [window], 0, now.timestamp())
            self.assertTrue(control["held"], control)
            with tempfile.TemporaryDirectory() as scratch:
                policy = Path(scratch) / "orchestration.toml"
                policy.write_text("")
                previous = os.environ.get("AGENTS_ORCHESTRATION")
                try:
                    os.environ["AGENTS_ORCHESTRATION"] = str(policy)
                    self.assertIn("week", pace_control.refusal("claude:a", control, now.timestamp()))
                finally:
                    if previous is None:
                        os.environ.pop("AGENTS_ORCHESTRATION")
                    else:
                        os.environ["AGENTS_ORCHESTRATION"] = previous

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

    def test_soft_hold_resume_flaps_wait_but_a_hard_projection_holds_immediately(self):
        now = datetime.now(timezone.utc).timestamp()

        def sample(at, used=30, burn=0.1):
            return [dict(window="week", used_pct=used, burn_pct_h=burn, reset_h=24,
                         resets_at="unchanged", age_s=0, as_of=datetime.fromtimestamp(at, timezone.utc).isoformat())]

        control = pace_control.advance({}, sample(now), 1, now)
        for minutes in range(1, 10):
            at = now + minutes * 60
            control = pace_control.advance(control, sample(at), None if minutes % 2 else 1, at)
            self.assertFalse(control["held"], control)
        at = now + 10 * 60
        control = pace_control.advance(control, sample(at, used=99, burn=10), 1, at)
        self.assertTrue(control["held"])
        control = pace_control.advance(control, sample(at + 60, used=30), 1, at + 60)
        self.assertTrue(control["held"])


if __name__ == "__main__":
    unittest.main()
