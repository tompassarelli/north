import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import flow
import goal_eta

FIXTURE = json.loads(Path(__file__).with_name("fixtures").joinpath("goal-eta-2026-10-12.json").read_text())
THREADS = goal_eta.threads_module()


LEDGER = [dict(zip(FIXTURE["ledger_fields"], row)) for row in FIXTURE["ledger"]]


def off(finish, actual):
    return abs((goal_eta.stamp(finish) - goal_eta.stamp(actual)).total_seconds()) / 60


class Replay(unittest.TestCase):
    def test_goals_of_12_oct_replayed_from_their_start(self):
        results = {g["goal"]: (g, goal_eta.replay(g, LEDGER, THREADS)) for g in FIXTURE["goals"]}
        recorded = {"policy-lead harness goal": (450, "2026-10-12T12:54:00Z", 978, 26, 22),
                    "north#35": (62, "2026-10-12T08:07:00Z", 310, 5, 22)}
        for name, (goal, result) in results.items():
            self.assertEqual((result["minutes"], result["finish"], result["serial_minutes"], result["boxes"], result["width"]),
                             recorded[name], name)
            self.assertLessEqual(goal_eta.stamp(result["finish"]), goal_eta.stamp(goal["target"]), name)
            self.assertLess(goal_eta.stamp(result["finish"]), goal_eta.stamp(goal["hand"]), name)
            print(f"{name}: start {goal['start']}, predicted {result['finish']}, lane M {goal['m_predicted']}, actual "
                  f"{goal['actual']} ({goal['actual_note']}), serial hand-sum {result['serial_finish']}, hand {goal['hand']}")
        goal, result = results["north#35"]
        self.assertLess(off(result["finish"], goal["actual"]), off(goal["m_predicted"], goal["actual"]))
        _, result = results["policy-lead harness goal"]
        lower = max(sum(b["eta"] for b in path) for path in chains(result["boxes_detail"]))
        self.assertEqual(result["minutes"], lower)

    def test_projected_miss_compares_the_goal_eta_with_the_target(self):
        goal = FIXTURE["goals"][0]
        result = goal_eta.replay(goal, LEDGER, THREADS)
        rows = [{"at": goal["start"], "kind": "goal_eta", "cell": "north", "n": result["boxes"], "value": result["minutes"],
                 "source": json.dumps({"predicted_finish": result["finish"], "boxes": result["boxes"], "width": result["width"],
                                       "target": target})} for target in (goal["target"], "2026-10-12T07:00:00Z")]
        base = {"repo": "north", "pending": 0, "landed_per_hour": 6, "push_to_land_minutes": 0, "push_to_land_p90_minutes": 0,
                "closed_per_hour": 0.0, "needed_per_hour": 14 / 10.6, "land_to_close_minutes": None, "land_ratio_3h": None,
                "failed_unowned": 0, "oldest_lane_wait_minutes": None}
        with tempfile.TemporaryDirectory() as scratch, patch.dict(os.environ, {"AGENTS_OPERATOR_DIR": scratch}):
            lines = {}
            for target in (goal["target"], "2026-10-12T07:00:00Z"):
                epoch = flow.timestamp(target)
                finish = flow.goal_finish("north", epoch, [r for r in rows if json.loads(r["source"])["target"] == target])
                lines[target] = flow.format_row({**base, "target": epoch, "goal_finish": finish})
        self.assertNotIn("Blocked: decision for Tom:", lines[goal["target"]])
        self.assertIn("Blocked: decision for Tom: north projected miss 2026-10-12T07:00:00Z", lines["2026-10-12T07:00:00Z"])


class Scale(unittest.TestCase):
    def test_goal_forecast_scales_boxes_by_the_median_not_the_coverage_quantile(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch) / "ledger.db"
            db = goal_eta.sqlite3.connect(path)
            db.execute("CREATE TABLE runs (agent TEXT)")
            db.execute("CREATE TABLE calibration (at TEXT, kind TEXT, cell TEXT, n INT, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT)")
            at = goal_eta.datetime.now(goal_eta.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
            db.execute("INSERT INTO calibration VALUES (?, 'box', 'all', 563, 0.31, 0.78, 1.99, 1.99, 'measured')", (at,))
            db.commit()
            db.close()
            _, _, (scale, label) = goal_eta.snapshot(THREADS, path)
        self.assertEqual(scale, 0.78, label)


def chains(boxes):
    def walk(key):
        children = [c for c, b in boxes.items() if key in b["deps"]]
        return [[boxes[key], *rest] for c in children for rest in walk(c)] or [[boxes[key]]]
    return [path for key, box in boxes.items() if not box["deps"] for path in walk(key)]


if __name__ == "__main__":
    unittest.main()
