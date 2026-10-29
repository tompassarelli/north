#!/usr/bin/env python3
import copy
import importlib.util
import os
from pathlib import Path
import random
import unittest

os.environ["NORTH_BIN"] = os.environ.get("NORTH_PACE_BIN") or str(Path(__file__).resolve().parents[2] / "target/debug/north")
spec = importlib.util.spec_from_file_location("routing_backtest", Path(__file__).with_name("routing-backtest.py"))
R = importlib.util.module_from_spec(spec)
spec.loader.exec_module(R)


def run(agent, start, end, tier="codex:sol-medium", text="repair parser"):
    fields = dict(category="feature", spec="measured", scope="module", surface="shell", verify="local-test")
    return dict(agent=agent, started=start, ended=end, tier=tier, follows=None, outcome="done", item="synthetic#1",
                expects_landing=0, landed=0, landed_at=None, minutes=1, cost_usd=999,
                brief_origin="codex.PostToolUse.spawn_agent", **fields,
                brief_text="\n".join(f"{k.capitalize()}: {v}" for k, v in fields.items()) + "\n" + text)


class RetrievalTests(unittest.TestCase):
    def test_future_observations_cannot_change_an_earlier_pick(self):
        rng = random.Random(73)
        rows = [run(f"prior{i}", "01", "02", text="repair parser " + str(rng.randrange(10))) for i in range(20)]
        costs = [dict(agent=r["agent"], ended="03", cost_usd=2) for r in rows]
        rows.append(run("target", "05", "09"))
        original = R.retrieval_decisions(rows, costs)[-1]
        self.assertEqual((original["rule"], original["k10"], original["k20"]), ("codex:sol-medium",) * 3)
        for i in range(40):
            changed = copy.deepcopy(rows)
            future = run("future", "06", "07", "claude:haiku", "repair parser " * rng.randrange(1, 100))
            future["follows"] = "prior0"
            changed.insert(rng.randrange(len(changed)), future)
            extra = [dict(agent="prior0", ended="08", cost_usd=rng.random() * 1000),
                     dict(agent="future", ended="08", cost_usd=.001)]
            target = R.retrieval_decisions(changed, costs + extra)[len(rows) - 1]
            self.assertEqual(target, original)
        unknown = R.retrieval_decisions(rows, [dict(c, ended="05") for c in costs])[-1]
        self.assertEqual(unknown["rule"], "codex:sol-medium")
        self.assertIsNone(unknown["k10"])
        bad = copy.deepcopy(rows)
        for r in bad[:-1]:
            r["tier"] = "unsupported-historical-tier"
        self.assertIsNone(R.retrieval_decisions(bad, costs)[-1]["k20"])
        landed = copy.deepcopy(rows)
        for r in landed[:-1]:
            r.update(expects_landing=1, landed=1, landed_at="08")
        self.assertIsNone(R.retrieval_decisions(landed, costs)[-1]["k10"])

    def test_neighbor_limits_and_observed_costs_control_the_pick(self):
        rows = [run(f"prior{i:02}", "01", "02", "codex:sol-medium" if i < 10 else "codex:sol-high") for i in range(20)]
        costs = [dict(agent=r["agent"], ended="03", cost_usd=2 if i < 10 else 1) for i, r in enumerate(rows)]
        rows.append(run("target", "05", "09"))
        pick = R.retrieval_decisions(rows, costs)[-1]
        self.assertEqual(pick["k10"], "codex:sol-medium")
        self.assertEqual(pick["k20"], "codex:sol-high")
        for r in rows[10:20]:
            r["outcome"] = "failed"
        self.assertEqual(R.retrieval_decisions(rows, costs)[-1]["k20"], "codex:sol-medium")
        rows[-1]["brief_origin"] = "report"
        self.assertEqual(len(R.retrieval_decisions(rows, costs)), 20)
        named = [run(f"named{i:02}", "01", "02", "gpt-6.1-sol medium") for i in range(20)]
        named.append(run("target", "05", "09", "gpt-6.1-sol medium"))
        pick = R.retrieval_decisions(named, [dict(agent=r["agent"], ended="03", cost_usd=2) for r in named])[-1]
        self.assertEqual((pick["actual"], pick["k10"], pick["k20"]), ("codex:sol-medium",) * 3)

    def test_unsupported_matches_cannot_claim_savings(self):
        rows = [dict(actual="codex:sol-medium", rule="codex:sol-medium", k10=None, waste=True, escalation=True)]
        result = R.retrieval_metrics(rows, "k10", 100, 7)
        self.assertEqual(result["arm"]["matched"], 0)
        self.assertIsNone(result["arm"]["waste"])
        self.assertIsNone(result["waste_ci95"])
        self.assertFalse(result["candidate_for_review"])
        rows[0]["k10"] = "codex:sol-medium"
        result = R.retrieval_metrics(rows, "k10", 100, 7)
        self.assertEqual(result["waste_ci95"], [0, 0])
        self.assertFalse(result["candidate_for_review"])
        self.assertIsNone(R.retrieval_metrics([], "k10", 100, 7)["waste_ci95"])


if __name__ == "__main__":
    unittest.main()
