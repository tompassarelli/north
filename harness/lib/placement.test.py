import copy
import json
import os
from pathlib import Path
import tempfile
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
        host = patch.dict(os.environ, RESIDENT_CAP_HOST="gex45")
        host.start()
        self.addCleanup(host.stop)

    def control(self, name):
        return self.gate[name.split(":", 1)[0]]["controls"][name]

    def place(self, candidates=False):
        before = copy.deepcopy((self.gate, self.cfg, self.spawner))
        with patch.object(placement, "host_load", side_effect=AssertionError("live host probe")), \
             patch.object(placement, "laptop_state", side_effect=AssertionError("live laptop probe")), \
             patch.object(placement, "credit", side_effect=AssertionError("live credit probe")):
            result = placement.place_spawn(self.brief, self.spawner, self.gate, self.cfg, self.at)
        self.assertEqual(before, (self.gate, self.cfg, self.spawner))
        if not candidates and "candidates" in result:
            result = result["candidates"][0]
            if result["kind"] == "native":
                return {"kind": "native"}
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
        self.assertEqual((self.place()["account"], self.place()["host"]), ("codex", "gex45"))
        self.control("claude:c")["held_reason"] = "floor"
        self.assertEqual(self.place()["host"], "gex45")

    def test_unsupported_launch_hosts_queue_with_the_placement_reason(self):
        self.control("claude:a")["held_reason"] = "workers"
        for host in ("remote", "anthropic-cloud"):
            with self.subTest(host=host):
                self.cfg["hosts"][host] = {"resident_cap": 24}
                self.cfg["placement"]["order"] = [host]
                self.cfg["placement"]["probe"].update({host: (True, 3), "credit": (True, "prepaid")})
                result = self.place()
                self.assertEqual(result["kind"], "queued")
                self.assertIn(host, result["reason"])
                self.assertIn("prepaid" if host == "anthropic-cloud" else "3/24", result["reason"])

    def test_controller_open_accounts_ignore_old_pace_admission(self):
        self.spawner.update(provider="codex", account="codex", tier="codex:sol-medium")
        self.control("codex")["held_reason"] = "floor"
        self.cfg["success"] = {"claude:haiku|tooling": 99}
        with patch.object(routing, "price", side_effect=AssertionError("old price admission")), \
             patch.object(routing, "pace_open", side_effect=AssertionError("old pace admission")):
            candidates = self.place(candidates=True)["candidates"]
        self.assertEqual([c["account"] for c in candidates], ["claude:c", "claude:a"])
        self.assertEqual(candidates[0]["tier"], "claude:opus-medium")

    def test_candidates_keep_own_account_first_until_an_account_drains(self):
        candidates = self.place(candidates=True)["candidates"]
        self.assertEqual([c["account"] for c in candidates], ["claude:a", "claude:c", "codex"])
        self.assertEqual(candidates[0]["kind"], "native")
        week = next(w for w in self.control("codex")["windows"] if w["window"] == "week")
        self.control("codex")["target_burn_pct_h"] = week["smoothed_pct_h"] + 1
        candidates = self.place(candidates=True)["candidates"]
        self.assertEqual([c["account"] for c in candidates], ["codex", "claude:c", "claude:a"])

    def admission_fixture(self):
        for name in ("claude:a", "claude:c", "codex"):
            control = self.control(name)
            control.update(held=False, workers=0, allowed_workers=1)
            for window in control["windows"]:
                window["burn_pct_h"] = 0

    def admit(self, candidates, path):
        with patch.dict(os.environ, AGENTS_ORCHESTRATION=str(Path(__file__).parent / "fixtures/orchestration.toml")), \
             patch.object(placement.pace_sampler.time, "time", return_value=self.at.timestamp()):
            return placement.admit(candidates, path, now=self.at)

    def test_admission_reserves_second_account_after_first_fills_and_queues_when_full(self):
        candidates = self.place(candidates=True)["candidates"][:2]
        self.admission_fixture()
        self.control("claude:a")["workers"] = 1
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "usage-gate.json"
            path.write_text(json.dumps(self.gate))
            result = self.admit(candidates, path)
            self.assertEqual((result["kind"], result["account"]), ("launch", "claude:c"))
            reserved = json.loads(path.read_text())["claude"]["controls"]
            self.assertEqual([reserved[name]["workers"] for name in ("claude:a", "claude:c")], [1, 1])
            self.assertEqual(self.admit(candidates, path)["kind"], "queued")
            for name in ("claude:a", "claude:c"):
                self.control(name)["held_reason"] = "floor"
            path.write_text(json.dumps(self.gate))
            self.assertEqual(self.admit(candidates, path),
                             {"kind": "refused", "reason": "every eligible account at its floor"})

    def test_projected_spend_skips_account_without_consuming_its_worker_slot(self):
        candidates = self.place(candidates=True)["candidates"][:2]
        self.admission_fixture()
        self.gate["claude"]["accounts"] = {"claude:a": {"used_pct": 89, "cap_pct": 90},
                                            "claude:c": {"used_pct": 80, "cap_pct": 90}}
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "usage-gate.json"
            path.write_text(json.dumps(self.gate))
            with patch.object(placement.usage, "ledger_reservation", return_value={"expected": 2, "existing": 3}):
                result = self.admit(candidates, path)
            self.assertEqual(result["account"], "claude:c")
            reserved = json.loads(path.read_text())["claude"]["controls"]
            self.assertEqual([reserved[name]["workers"] for name in ("claude:a", "claude:c")], [0, 1])

    def test_production_reads_local_and_laptop_state_without_a_probe_fixture(self):
        del self.cfg["placement"]["probe"]
        for local in ("gex45", "whiterabbit"):
            with self.subTest(local=local), patch.dict(os.environ, RESIDENT_CAP_HOST=local), \
                 patch.object(placement.resident_cap, "count", return_value=3), \
                 patch.object(placement, "laptop_state", return_value="unavailable"), \
                 patch.object(placement.subprocess, "run", side_effect=AssertionError("remote probe")):
                result = placement.place_spawn(self.brief, self.spawner, self.gate, self.cfg, self.at)
            self.assertEqual((result["kind"], result["account"], result["host"]), ("native", "claude:a", local))
        self.brief += "Verify: native\n"
        with patch.object(placement, "laptop_state", return_value="available"):
            result = placement.place_spawn(self.brief, self.spawner, self.gate, self.cfg, self.at)
        self.assertEqual((result["kind"], result["host"]), ("launch", "laptop"))

    def test_unsupported_host_skips_that_candidate_and_keeps_supported_ones(self):
        self.spawner.update(provider="codex", account="codex", tier="codex:sol-medium")
        self.cfg["placement"].update(order=["anthropic-cloud", "gex45", "laptop"])
        self.cfg["placement"]["probe"]["credit"] = (True, "cloud credit: prepaid")
        result = self.place(candidates=True)
        self.assertEqual((result["kind"], result["account"]), ("native", "codex"))
        self.assertEqual([c["account"] for c in result["candidates"]], ["codex"])

    def test_admission_queues_on_a_temporarily_held_eligible_account(self):
        self.control("claude:c")["held_reason"] = "short-window"
        self.control("codex")["held_reason"] = "floor"
        result = self.place(candidates=True)
        self.assertEqual([c["account"] for c in result["candidates"]], ["claude:a"])
        self.admission_fixture()
        self.control("claude:a")["held_reason"] = "floor"
        reset = next(w["resets_at"] for w in self.control("claude:c")["windows"] if w["window"] == "5h")
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "usage-gate.json"
            path.write_text(json.dumps(self.gate))
            with patch.dict(os.environ, AGENTS_ORCHESTRATION=str(Path(__file__).parent / "fixtures/orchestration.toml")), \
                 patch.object(placement.pace_sampler.time, "time", return_value=self.at.timestamp()):
                admitted = placement.admit(result["candidates"], path, eligible=result["eligible"], now=self.at)
        self.assertEqual((admitted["kind"], admitted["eta"]), ("queued", reset))

    def test_reservation_rechecks_controller_hold_inside_the_lock(self):
        candidates = self.place(candidates=True)["candidates"][:1]
        self.admission_fixture()
        reserve = placement.pace_sampler.reserve
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "usage-gate.json"
            path.write_text(json.dumps(self.gate))
            def flip(*args, **kwargs):
                self.control("claude:a")["held_reason"] = "blocked"
                path.write_text(json.dumps(self.gate))
                return reserve(*args, **kwargs)
            with patch.object(placement.pace_sampler, "reserve", side_effect=flip):
                result = self.admit(candidates, path)
            self.assertEqual(result["kind"], "queued")
            self.assertIn("blocked", result["reason"])
            self.assertEqual(json.loads(path.read_text())["claude"]["controls"]["claude:a"]["workers"], 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
