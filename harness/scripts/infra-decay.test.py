#!/usr/bin/env python3
from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("infra_decay", HERE / "infra-decay.py")
decay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(decay)


def fixture(name):
    snapshot = json.loads((HERE / "fixtures/infra-decay" / f"{name}.json").read_text())
    snapshot["org"] = json.loads((HERE / "fixtures/infra-decay/org.json").read_text())
    return snapshot


class InfraDecay(unittest.TestCase):
    def test_recorded_oct15_decay_and_quiet_day(self):
        snapshot = fixture("2026-10-15")
        rows = decay.evaluate(snapshot, decay.instant(snapshot["at"]))
        fired = {(r["check"], r["repo"]) for r in rows if r["firing"]}
        self.assertEqual(fired, {("workflow-jobs", "smashcraft"), ("runner-label", "smashcraft"),
            ("stale-file", "test-health.json"), ("dead-timer", "laptop-auto.timer"),
            ("record-churn", "smashcraft")})
        jobs = next(r for r in rows if r["check"] == "workflow-jobs")
        self.assertEqual(jobs["value"]["workflow"], "Background sweeps")
        self.assertEqual(jobs["value"]["successes"], 0)
        self.assertEqual(jobs["value"]["cancelled"], 24)
        self.assertEqual(jobs["owner"], "smashcraft-claude-lead")
        for row in rows:
            self.assertEqual(set(row), {"check", "repo", "value", "baseline", "threshold", "firing", "owner", "brief",
                                        "incident_key", "needs_tom", "release"})
            self.assertIn("Needs-tom: no", row["brief"])
            self.assertIs(row["needs_tom"], False)
            self.assertEqual(row["release"], "away")
            self.assertIn(row["owner"], {n["id"] for n in snapshot["org"]["nodes"]
                                       if n["role"] == "lead" and n["state"] == "active"})
        quiet = fixture("quiet")
        self.assertFalse(any(r["firing"] for r in decay.evaluate(quiet, decay.instant(quiet["at"]))))
        print("ok: recorded Oct 15 fires Background sweeps (24 cancelled), stale test-health, dead laptop-auto, record churn; quiet fires 0; all owners are registered active leads, needs_tom=false, release=away")

    def test_incidents_emit_once_across_days_and_rearm_only_after_observed_recovery(self):
        snapshot = fixture("quiet")
        snapshot["timers"][0]["next"] = None
        now = decay.instant(snapshot["at"])
        boxes, active = decay.repair_boxes(decay.evaluate(snapshot, now), [])
        key = next(row["incident_key"] for row in boxes if row["check"] == "dead-timer")
        self.assertEqual(len(boxes), 1)
        next_day = now + decay.timedelta(days=1)
        snapshot["files"][0]["modified"] = next_day.timestamp()
        boxes, active = decay.repair_boxes(decay.evaluate(snapshot, next_day), active)
        self.assertEqual(boxes, [])
        _, missing = decay.repair_boxes([], active)
        self.assertIn(key, missing)
        snapshot["timers"][0]["next"] = next_day.timestamp() + 60
        boxes, active = decay.repair_boxes(decay.evaluate(snapshot, next_day), active)
        self.assertEqual(boxes, [])
        self.assertNotIn(key, active)
        snapshot["timers"][0]["next"] = None
        boxes, active = decay.repair_boxes(decay.evaluate(snapshot, next_day), active)
        self.assertEqual([row["incident_key"] for row in boxes], [key])
        snapshot["timers"].append(dict(name="other.timer", enabled=True, next=None))
        boxes, _ = decay.repair_boxes(decay.evaluate(snapshot, next_day), active)
        self.assertEqual([row["repo"] for row in boxes], ["other.timer"])
        print("ok: one incident emits once across two days, missing signals stay armed, observed recovery re-arms; distinct subjects emit independently")

    def test_seeded_threshold_crossings_and_time_windows(self):
        snapshot = fixture("quiet")
        snapshot["changes"] = [dict(repo="fixture", tom_ask=True, total_minutes=minutes,
            landed_at=at, bottleneck_stage="build") for at, minutes in
            [("2026-10-14T03:00Z", 10), ("2026-10-14T04:00Z", 20), ("2026-10-14T05:00Z", 30),
             ("2026-10-15T01:00Z", 31), ("2026-10-15T02:00Z", 40), ("2026-10-15T03:00Z", 50),
             ("2026-10-16T01:00Z", 999)]]
        snapshot["workflows"] = [dict(repo="fixture", name="Autoland", budget_s=120, runs=[
            dict(createdAt="2026-10-14T01:00Z", startedAt="2026-10-14T01:00Z",
                 updatedAt="2026-10-14T01:02Z", status="completed", conclusion="success"),
            dict(createdAt="2026-10-15T01:00Z", startedAt="2026-10-15T01:00Z",
                 updatedAt="2026-10-15T01:03Z", status="completed", conclusion="success"),
            dict(createdAt="2026-10-15T02:59Z", status="queued", conclusion="")])]
        snapshot["files"][0]["modified"] -= 1
        snapshot["routines"] = {"test-health": "health:modules/health/default.gal"}
        now = decay.instant(snapshot["at"])
        rows = {r["check"]: r for r in decay.evaluate(snapshot, now)}
        self.assertEqual(rows["velocity"]["value"], dict(median_minutes=40, n=3))
        self.assertEqual(rows["velocity"]["baseline"], 20)
        self.assertIn("build", rows["velocity"]["brief"])
        self.assertEqual(rows["gate-suite-time"]["value"]["p50_s"], 180)
        self.assertEqual(rows["gate-suite-time"]["baseline"], 120)
        self.assertEqual(rows["stale-file"]["owner"], "health-maintainer")
        self.assertTrue(all(rows[key]["firing"] for key in ("velocity", "gate-suite-time", "workflow-jobs", "stale-file")))
        boundary = deepcopy(snapshot)
        boundary["changes"] = snapshot["changes"][:3] + [dict(c, total_minutes=30) for c in snapshot["changes"][3:6]]
        boundary["workflows"][0]["runs"][1]["updatedAt"] = "2026-10-15T01:02Z"
        boundary["workflows"][0]["runs"][2]["createdAt"] = "2026-10-15T03:00Z"
        boundary["files"][0]["modified"] += 1
        self.assertFalse(any(r["firing"] for r in decay.evaluate(boundary, now)))
        print("ok: velocity, suite, queue and stale thresholds cross; exact boundaries and future changes stay quiet")


if __name__ == "__main__":
    unittest.main()
