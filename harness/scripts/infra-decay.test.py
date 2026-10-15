#!/usr/bin/env python3
from copy import deepcopy
from contextlib import contextmanager
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("infra_decay", HERE / "infra-decay.py")
decay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(decay)


def fixture(name):
    snapshot = json.loads((HERE / "fixtures/infra-decay" / f"{name}.json").read_text())
    snapshot["org"] = json.loads((HERE / "fixtures/infra-decay/org.json").read_text())
    return snapshot


@contextmanager
def collected_fixture():
    source = fixture("collection")
    now = decay.instant(source["at"])
    with tempfile.TemporaryDirectory(prefix="infra-decay-") as directory:
        home = Path(directory)
        state = home / ".local/state/agents"
        state.mkdir(parents=True)
        for name, contents in {
                "org.json": source["org"], "test-health.json": {}, "landing-health.json": {},
                "iteration-ledger/changes.json": {"projects": {"smashcraft": {}}},
                "worker-ledger.sqlite": {}}.items():
            path = state / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(contents))
            os.utime(path, (now.timestamp(), now.timestamp()))
        os.utime(state / "worker-ledger.sqlite", (decay.instant(source["retired_ledger_at"]).timestamp(),) * 2)
        mirror = home / ".local/state/threads/snapshot.db"
        mirror.parent.mkdir(parents=True)
        mirror.touch()
        os.utime(mirror, (now.timestamp(),) * 2)
        (home / "code/smashcraft/main").mkdir(parents=True)

        def run(args, **kwargs):
            if args[:4] == ["systemctl", "--user", "list-timers", "--all"]:
                output = source["timers"]
            elif args[:3] == ["systemctl", "--user", "list-unit-files"]:
                output = source["enabled"]
            elif args[:3] == ["systemctl", "--user", "show"]:
                output = source["timer_states" if args[3].endswith(".timer") else "service_states"]
            elif args[0] == "git" and args[3:5] == ["remote", "get-url"]:
                output = "https://github.com/fixture/smashcraft.git\n"
            elif args[0] == "git" and args[3] == "log":
                output = ""
            elif args[:2] == ["gh", "api"]:
                endpoint = args[-1]
                if endpoint.endswith("/actions/workflows?per_page=100"):
                    output = [{"workflows": source["workflows"]}]
                else:
                    if "/actions/runs?status=" in endpoint:
                        status = endpoint.split("status=", 1)[1].split("&", 1)[0]
                        output = [{"workflow_runs": [dict(w["outstanding"], workflow_id=w["id"])
                                                     for w in source["workflows"]
                                                     if w.get("outstanding", {}).get("status") == status]}]
                    else:
                        workflow = next(w for w in source["workflows"] if f'/workflows/{w["id"]}/runs?' in endpoint)
                        assert "created=%3E%3D" in endpoint and "status=" not in endpoint
                        page = int(endpoint.rsplit("page=", 1)[1])
                        output = {"workflow_runs": [dict(workflow["run"], id=workflow["id"] * 10000 + i)
                            for i in range((page - 1) * 100, min(page * 100, workflow["count"]))]}
            else:
                raise AssertionError(f"unrecognized fixture command: {args}")
            return subprocess.CompletedProcess(args, 0, output if isinstance(output, str) else json.dumps(output), "")

        with patch.dict(os.environ, {"XDG_STATE_HOME": str(home / ".local/state")}), patch.object(decay.subprocess, "run", side_effect=run):
            yield decay.collect(now, home), now, mirror


class InfraDecay(unittest.TestCase):
    def test_current_norththreads_mirror_does_not_report_retired_ledger_decay(self):
        with collected_fixture() as (snapshot, now, mirror):
            rows = [r for r in decay.evaluate(snapshot, now) if r["check"] == "stale-file"]
            ledger = next(r for r in rows if r["repo"] == "threads/snapshot.db")
            self.assertFalse(ledger["firing"])
            self.assertFalse(any(r["repo"] == "worker-ledger.sqlite" for r in rows))
            os.utime(mirror, (now.timestamp() - 1801,) * 2)
            snapshot["files"][-1]["modified"] = mirror.stat().st_mtime
            self.assertTrue(next(r for r in decay.evaluate(snapshot, now)
                                 if r["repo"] == "threads/snapshot.db")["firing"])

    def test_running_timer_and_activating_service_stay_healthy_while_dead_timer_fires(self):
        with collected_fixture() as (snapshot, now, _):
            timers = {r["repo"]: r for r in decay.evaluate(snapshot, now) if r["check"] == "dead-timer"}
            self.assertFalse(timers["worker-ledger.timer"]["firing"])
            self.assertTrue(timers["laptop-auto.timer"]["firing"])
            worker = next(t for t in snapshot["timers"] if t["name"] == "worker-ledger.timer")
            worker["sub"] = "waiting"
            self.assertFalse(next(r for r in decay.evaluate(snapshot, now)
                                  if r["repo"] == "worker-ledger.timer")["firing"])
            worker.update(active="activating", service_active=["inactive"])
            self.assertFalse(next(r for r in decay.evaluate(snapshot, now)
                                  if r["repo"] == "worker-ledger.timer")["firing"])
            worker.update(active="failed", service_active=["failed"])
            self.assertTrue(next(r for r in decay.evaluate(snapshot, now)
                                 if r["repo"] == "worker-ledger.timer")["firing"])

    def test_three_day_queue_stays_armed_outside_completed_run_window(self):
        with collected_fixture() as (snapshot, now, _):
            snapshot.update(files=[], timers=[], commits={})
            snapshot["workflows"] = [dict(snapshot["workflows"][0], runs=[
                next(r for r in snapshot["workflows"][0]["runs"] if r["status"] == "queued")])]
            rows = decay.evaluate(snapshot, now)
            jobs = next(r for r in rows if r["check"] == "workflow-jobs")
            self.assertEqual(jobs["value"]["runs"], 0)
            self.assertEqual(jobs["value"]["oldest_queue_minutes"], 4320)
            self.assertTrue(jobs["firing"])
            self.assertEqual(decay.repair_boxes(rows, [jobs["incident_key"]]), ([], [jobs["incident_key"]]))

    def test_per_workflow_collection_keeps_sweeps_behind_1100_newer_helper_runs(self):
        with collected_fixture() as (snapshot, now, _):
            workflows = {w["name"]: w for w in snapshot["workflows"]}
            self.assertEqual(len(workflows["Autoland heartbeat"]["runs"]), 1100)
            jobs = next(r for r in decay.evaluate(snapshot, now)
                        if r["check"] == "workflow-jobs" and r["value"]["workflow"] == "Background sweeps")
            self.assertEqual(jobs["value"]["cancelled"], 6)
            self.assertTrue(jobs["firing"])

    def test_queue_states_are_read_once_per_repo_not_per_workflow(self):
        calls = []
        original = decay.command
        with patch.object(decay, "command", side_effect=lambda args: calls.append(args[-1]) or original(args)):
            with collected_fixture() as (snapshot, now, _):
                pass
        queue = sorted(c for c in calls if "status=" in c)
        self.assertEqual(queue, [f"repos/fixture/smashcraft/actions/runs?status={state}&per_page=100"
                                 for state in sorted(decay.QUEUE_STATES)])
        per_workflow = [c for c in calls if "/workflows/" in c and "/runs?" in c]
        self.assertTrue(per_workflow and all("created=%3E%3D2026-10-13T05%3A00%3A00Z" in c for c in per_workflow))

    def test_unowned_workflow_routes_to_registered_test_audit_lead(self):
        snapshot = fixture("quiet")
        snapshot["workflows"][0].update(repo="wisp", runs=[dict(
            createdAt="2026-10-12T05:00Z", status="queued")])
        row = next(r for r in decay.evaluate(snapshot, decay.instant(snapshot["at"]))
                   if r["check"] == "workflow-jobs")
        self.assertTrue(row["firing"])
        self.assertEqual(row["owner"], "test-audit-lead")

    def test_only_gate_suite_identities_measure_runtime_excluding_heartbeat_and_sweep_helpers(self):
        snapshot = fixture("quiet")
        snapshot["workflows"] = [dict(repo="fixture", name=name, runs=fixture("collection")["gate_runs"])
                                 for name in ("Autoland", "landing-gate", "Farm test", "Autoland heartbeat",
                                              "Autoland sweep", "Background sweeps")]
        rows = [r for r in decay.evaluate(snapshot, decay.instant(snapshot["at"])) if r["check"] == "gate-suite-time"]
        self.assertEqual({r["value"]["workflow"] for r in rows}, {"Autoland", "landing-gate", "Farm test"})
        self.assertTrue(all(r["firing"] for r in rows))

    def test_record_as_a_leading_verb_is_not_a_record_refresh(self):
        self.assertFalse(decay.record_refresh("Record independent review of playtest pool custody"))
        self.assertFalse(decay.record_refresh("docs: record independent landing incident review"))
        self.assertTrue(decay.record_refresh("Refresh Classic effect record for platform lane"))
        self.assertTrue(decay.record_refresh("Regenerate reports after cue change"))

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
