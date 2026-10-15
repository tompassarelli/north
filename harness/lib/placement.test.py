import copy
import json
from pathlib import Path
import tomllib
import unittest
from unittest.mock import patch

import placement
import routing


class SpawnPlacement(unittest.TestCase):
    def setUp(self):
        here = Path(__file__).resolve().parent
        self.gate = json.loads((here / "fixtures/placement-gate.json").read_text())
        self.cfg = tomllib.loads((here / "fixtures/orchestration.toml").read_text())
        self.cfg["hosts"] = {"gex45": {"resident_cap": 24}}
        self.cfg["placement"] = {"order": ["gex45", "laptop"],
                                 "probe": {"gex45": (True, 3), "laptop": "available"}}
        self.at = routing.moment(self.gate["claude"]["ts"])
        self.spawner = {"provider": "claude", "account": "claude:a", "tier": "claude:opus-medium"}
        self.brief = "Item: north#1\nCategory: tooling\nDifficulty: 40\nNeeds: code\n"

    def control(self, name):
        return self.gate[name.split(":", 1)[0]]["controls"][name]

    def place(self):
        before = copy.deepcopy((self.gate, self.cfg, self.spawner))
        with patch.object(placement, "host_load", side_effect=AssertionError("live host probe")), \
             patch.object(placement, "laptop_state", side_effect=AssertionError("live laptop probe")), \
             patch.object(placement, "credit", side_effect=AssertionError("live credit probe")):
            result = placement.place_spawn(self.brief, self.spawner, self.gate, self.cfg, self.at)
        self.assertEqual(before, (self.gate, self.cfg, self.spawner))
        return result

    def test_worker_hold_moves_to_same_provider_without_changing_requested_tier(self):
        self.control("claude:a")["held_reason"] = "workers"
        self.assertEqual(self.place(), {"kind": "launch", "provider": "claude", "account": "claude:c",
                         "tier": "claude:opus-medium", "model": "claude-opus-5-5", "effort": "medium", "host": "gex45"})

    def test_provider_floor_uses_routing_band_and_measured_success(self):
        for name in ("claude:a", "claude:c"):
            self.control(name)["held_reason"] = "floor"
        result = self.place()
        self.assertEqual((result["provider"], result["account"], result["tier"]),
                         ("codex", "codex", "codex:sol-xhigh"))
        self.cfg["success"] = {"codex:sol-medium|tooling": 95}
        self.assertEqual(self.place()["tier"], "codex:sol-xhigh")
        self.spawner["tier"] = "claude:haiku"
        self.cfg["success"] = {"codex:sol-medium|tooling": 0, "codex:sol-high|tooling": 95}
        self.assertEqual(self.place()["tier"], "codex:sol-high")

    def test_every_floor_is_the_only_refusal(self):
        for name in ("claude:a", "claude:c", "codex"):
            self.control(name)["held_reason"] = "floor"
        self.assertEqual(self.place(), {"kind": "refused", "reason": "every eligible account at its floor"})
        self.control("codex")["held_reason"] = "blocked"
        self.assertEqual(self.place()["kind"], "queued")

    def test_short_window_queues_until_its_reset(self):
        for name in ("claude:a", "codex"):
            self.control(name)["held_reason"] = "floor"
        self.control("claude:c")["held_reason"] = "short-window"
        reset = next(w["resets_at"] for w in self.control("claude:c")["windows"] if w["window"] == "5h")
        result = self.place()
        self.assertEqual((result["kind"], result["eta"]), ("queued", reset))

    def test_underburn_moves_native_work_and_otherwise_keeps_native(self):
        week = next(w for w in self.control("claude:c")["windows"] if w["window"] == "week")
        self.control("claude:c")["target_burn_pct_h"] = week["smoothed_pct_h"] + 1
        self.assertEqual(self.place()["account"], "claude:c")
        self.control("claude:c")["target_burn_pct_h"] = week["smoothed_pct_h"]
        self.assertEqual(self.place(), {"kind": "native"})

    def test_stale_and_worker_holds_queue_without_refusal(self):
        for name in ("claude:a", "claude:c", "codex"):
            self.control(name)["at"] -= 180
        result = self.place()
        self.assertEqual(result["kind"], "queued")
        self.assertGreater(routing.moment(result["eta"]), self.at)
        for name in ("claude:a", "claude:c", "codex"):
            self.control(name)["at"] += 180
            self.control(name)["held_reason"] = "workers"
        self.assertEqual(self.place()["kind"], "queued")

    def test_needs_restrict_hosts_and_full_hosts_queue(self):
        self.control("claude:a")["held_reason"] = "workers"
        self.brief += "Verify: native\n"
        self.assertEqual(self.place()["host"], "laptop")
        self.cfg["placement"]["probe"]["laptop"] = "unavailable"
        self.assertEqual(self.place()["kind"], "queued")
        self.brief = self.brief.replace("Verify: native", "Verify: local-test")
        self.cfg["placement"]["probe"]["gex45"] = (True, 24)
        self.assertEqual(self.place()["kind"], "queued")

    def test_auto_route_and_provider_mode_exclude_accounts(self):
        self.control("claude:a")["held_reason"] = "workers"
        self.control("claude:c")["auto_route"] = False
        self.cfg["mode"] = "claude-only"
        self.assertEqual(self.place()["kind"], "queued")

    def test_missing_account_reading_cannot_turn_other_floors_into_refusal(self):
        for name in ("claude:a", "claude:c"):
            self.control(name)["held_reason"] = "floor"
        self.gate["codex"]["controls"] = {}
        self.assertEqual(self.place()["kind"], "queued")

    def test_cloud_hosts_match_provider_and_private_work_stays_off_cloud(self):
        self.control("claude:a")["held_reason"] = "workers"
        self.cfg["placement"].update(order=["anthropic-cloud", "gex45", "laptop"],
                                      private_repos=["north"])
        self.cfg["placement"]["probe"]["credit"] = (True, "cloud credit: prepaid")
        self.assertEqual(self.place()["host"], "gex45")
        self.cfg["placement"]["private_repos"] = []
        self.assertEqual(self.place()["host"], "anthropic-cloud")
        self.control("claude:c")["held_reason"] = "floor"
        self.assertEqual(self.place()["host"], "gex45")


if __name__ == "__main__":
    unittest.main(verbosity=2)
