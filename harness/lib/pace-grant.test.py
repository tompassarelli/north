#!/usr/bin/env python3
import importlib.util
import hashlib
import json
import os
from pathlib import Path
import subprocess
import socket
import sqlite3
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from datetime import datetime, timezone

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import pace_sampler


class CodexPool:
    def __init__(self, root):
        self.pool = root / "pool"
        (self.pool / "sqlite").mkdir(parents=True)
        self.db = self.pool / "sqlite/state_5.sqlite"
        with sqlite3.connect(self.db) as db:
            db.executescript("CREATE TABLE threads(id PRIMARY KEY, archived); CREATE TABLE thread_spawn_edges(parent_thread_id,child_thread_id);")
            db.execute("INSERT INTO threads VALUES ('parent',0)")


def fleet_stub(root, sessions=()):
    fleet, north = root / "fleet.json", root / "north"; fleet.write_text(json.dumps(sessions))
    north.write_text('#!/bin/sh\ncase "$1 $2" in "fleet list") cat "$NORTH_FLEET_FIXTURE";; "fleet send") printf "%s\n" "$*" >>"$NORTH_FLEET_SENDS";; *) exit 2;; esac\n'); north.chmod(0o755); return fleet, north


class Grants(unittest.TestCase):
    def test_held_spawn_reports_only_its_lapsed_grant_once(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / ".local/state/agents"
            state.mkdir(parents=True)
            fleet, north = fleet_stub(root)
            node = {"name": "parent", "conversation": "parent", "parent": None, "provider": "codex", "status": "working"}
            fleet.write_text(json.dumps([node]))
            now = time.time()
            window = {"window": "week", "as_of": datetime.now(timezone.utc).isoformat(),
                      "used_pct": 50, "burn_pct_h": 0, "reset_h": 24, "resets_at": "reset"}
            control = {"at": now, "held": True, "workers": 1, "allowed_workers": 1, "windows": [window]}
            verdict = {"epoch": now, "stale_min": 30, "pace_control": True, "refusal": None,
                       "controls": {"codex": control}}
            (state / "usage-gate.json").write_text(json.dumps({"codex": verdict}))
            expired = {"account": "codex", "lead": "parent", "session": "parent", "max_workers": 4,
                       "until": now - 3600, "until_utc": datetime.fromtimestamp(now - 3600, timezone.utc).isoformat(),
                       "authority": "proxy", "at": now - 7200, "reason": "fixture admission"}
            (state / "pace-control.json").write_text(json.dumps({"grants": [expired]}))
            policy = root / "orchestration.toml"
            policy.write_text("")
            previous = dict(os.environ)
            try:
                os.environ.update(SPAWN_CAPACITY_HOME=str(root), XDG_STATE_HOME=str(root / ".local/state"),
                                  NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"), AGENTS_ORCHESTRATION=str(policy))
                spec = importlib.util.spec_from_file_location("expiry_capacity", HERE.parent / "hooks/lib/spawn_capacity.py")
                capacity = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(capacity)
                event = {"tool_name": "collaborationspawn_agent", "session_id": "foreign",
                         "cwd": str(root), "tool_input": {"task_name": "fix", "message": "original"}}
                notices = root / "fleet-sends"
                self.assertTrue(capacity.evaluate(event)[0].startswith("Hold:"))
                self.assertFalse(notices.exists())
                event["session_id"] = "parent"
                fleet.write_text(json.dumps([node | {"conversation": "replacement"}]))
                self.assertTrue(capacity.evaluate(event)[0].startswith("Hold:"))
                self.assertFalse(notices.exists())
                fleet.write_text(json.dumps([node]))
                held = capacity.evaluate(event)
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(notices.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                self.assertIn("parent", notices.read_text())
                self.assertIn(expired["until_utc"], notices.read_text())
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **expired}) + "\n")
                data = json.loads((state / "pace-control.json").read_text())
                data["grants"] = []
                (state / "pace-control.json").write_text(json.dumps(data))
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(notices.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                renewed = expired | {"until": now + 3600, "at": now}
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **renewed}) + "\n")
                notices.unlink()
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(notices.exists())
                replacement = expired | {"at": now + 1, "task": "other"}
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **replacement}) + "\n")
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(notices.exists())
                consumed = expired | {"at": now + 2, "uses": 1}
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **consumed}) + "\n")
                    log.write(json.dumps({"event": "grant-admission", "at": now + 3,
                                          "lead": "parent", "session": "parent", "account": "codex"}) + "\n")
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(notices.exists())
                remaining = expired | {"at": now + 4, "uses": 1, "task": "fix"}
                (state / "pace-control.json").write_text(json.dumps({"grants": [remaining]}))
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **(remaining | {"uses": 2})}) + "\n")
                    log.write(json.dumps({"event": "grant-admission", "at": now + 5,
                                          "lead": "parent", "session": "parent", "account": "codex"}) + "\n")
                    log.write("[" * 10000 + "0" + "]" * 10000 + "\n")
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(notices.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                self.assertEqual(capacity.evaluate(event), held)
                self.assertEqual(notices.read_text().count("GRANT-EXPIRED-HOLDING"), 1)
                exhausted = remaining | {"at": now + 6, "uses": 0}
                (state / "pace-control.json").write_text(json.dumps({"grants": [exhausted]}))
                with (state / "pace-control.jsonl").open("a") as log:
                    log.write(json.dumps({"event": "proxy-grant", **(exhausted | {"uses": 2})}) + "\n")
                notices.unlink()
                self.assertEqual(capacity.evaluate(event), held)
                self.assertFalse(notices.exists())
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_proxy_grant_journey_preserves_scope_ceiling_money_and_expiry(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / ".local/state/agents"
            state.mkdir(parents=True)
            fleet, north = fleet_stub(root, [{"name": "parent", "conversation": "parent", "status": "working", "provider": "codex"}])
            control = {"at": time.time() - 600, "held": True, "workers": 0, "allowed_workers": 1, "windows": []}
            (state / "pace-control.json").write_text(json.dumps({"accounts": {"codex": control}}))
            gate = state / "usage-gate.json"
            verdict = {"epoch": time.time() - 600, "stale_min": 30, "pace_control": True,
                       "refusal": None, "controls": {"codex": control}}
            gate.write_text(json.dumps({"codex": verdict}))
            probe = root / "probe.json"
            probe.write_text(json.dumps({"committedBatchCpus": 0, "aggregateCpuLimit": 20, "protectedCpuSomeAvg10": 0}))
            previous = dict(os.environ)
            peer = CodexPool(root)
            try:
                os.environ.update(HOME=str(root), XDG_STATE_HOME=str(root / ".local/state"),
                                  NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"), SPAWN_CAPACITY_HOME=str(root),
                                  SPAWN_CAPACITY_STATUS=str(probe), CODEX_BEHAVIOR_STATE=str(root / "behavior"))
                os.environ["NORTH_CODEX_CONVERSATION_HOME"] = str(peer.pool)
                os.environ["NORTH_CODEX_CONVERSATION_SQLITE_HOME"] = str(peer.pool / "sqlite")
                os.environ.pop("AGENT_ORG_NAME", None)
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                result = subprocess.run([str(HERE.parents[1] / "bin/agents"), "pace", "grant", "codex", "2",
                                         "--until", expiry, "--for", "parent", "--reason", "proxy approved playtest"],
                                        capture_output=True, text=True, timeout=5)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["authority"], "proxy")
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "unrelated"))
                self.assertIsNone(pace_sampler.grant_for(state, "claude:c", "parent"))
                spec = importlib.util.spec_from_file_location("grant_capacity", HERE.parent / "hooks/lib/spawn_capacity.py")
                capacity = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(capacity)
                event = {"tool_name": "collaborationspawn_agent", "session_id": "parent", "cwd": str(root),
                         "tool_use_id": "first",
                         "tool_input": {"task_name": "playtest", "message": "[routine:watchdog]"}}
                self.assertIsNone(capacity.evaluate(event)[0])
                capacity.confirm_spawn(event)
                event["tool_use_id"] = "second"
                self.assertIsNone(capacity.evaluate(event)[0])
                capacity.confirm_spawn(event)
                self.assertIn("2/2", capacity.evaluate(event)[0])
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent", time.time() + 3601))
                fresh = control | {"at": time.time(), "windows": [{"window": "week", "age_s": 0,
                    "as_of": datetime.now(timezone.utc).isoformat(), "used_pct": 32, "burn_pct_h": 0,
                    "reset_h": 65, "projected_pct": 32, "resets_at": "reset"}]}
                gate.write_text(json.dumps({"codex": verdict | {"epoch": time.time(), "controls": {"codex": fresh},
                    "refusal": "No: a new codex worker could spend money: fixture."}}))
                self.assertIn("could spend money", capacity.evaluate(event)[0])
                gate.write_text(json.dumps({"codex": verdict | {"epoch": time.time(), "controls": {"codex": fresh}}}))
                pace_sampler.grant("codex", 3, expiry, "parent", "proxy one reviewer", "review", 1)
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent", task="playtest"))
                self.assertIsNone(pace_sampler.reserve(gate, "codex", "codex", session="parent", task="review"))
                data = json.loads((state / "pace-control.json").read_text())
                data["grants"][0]["pending"][0]["id"] = "review"
                (state / "pace-control.json").write_text(json.dumps(data))
                pace_sampler.confirm(state, "parent", "review")
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent", task="review"))
                data = json.loads((state / "pace-control.json").read_text())
                data["grants"][0]["until"] = time.time() - 1
                (state / "pace-control.json").write_text(json.dumps(data))
                self.assertIsNotNone(capacity.evaluate(event)[0])
                fleet.write_text(json.dumps([{"name": "parent", "conversation": "replacement", "status": "working", "provider": "codex"}]))
                self.assertIsNone(pace_sampler.grant_for(state, "codex", "parent"))
                rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                self.assertEqual(sum(r["event"] == "grant-admission" for r in rows), 3)
            finally:
                os.environ.clear()
                os.environ.update(previous)

    def test_refused_spawns_release_slots_without_consuming_uses(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            fleet, north = fleet_stub(root, [
                {"name": "parent", "conversation": "parent", "provider": "codex", "status": "working"}])
            peer = CodexPool(root)
            with patch.dict(os.environ, XDG_STATE_HOME=str(root), NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"),
                            NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                            NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite")):
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                pace_sampler.grant("codex", 2, expiry, "parent", "refused spawn regression", "review", 3)
                gate = state / "usage-gate.json"
                def reserve(identity):
                    return pace_sampler.reserve(gate, "codex", "codex", session="parent",
                                                task="review", tool_use_id=identity)
                now = time.time()
                with patch.object(pace_sampler.time, "time", return_value=now):
                    self.assertIsNone(reserve("denied-one"))
                    self.assertIsNone(reserve("denied-two"))
                    self.assertIn("2/2", reserve("blocked"))
                with patch.object(pace_sampler.time, "time", return_value=now + pace_sampler.RESERVATION_TTL_S + 1):
                    self.assertIsNone(reserve("started"))
                    self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0]["uses"], 3)
                    pace_sampler.confirm(state, "parent", "started")
                    pace_sampler.confirm(state, "parent", "started")
                    self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0]["uses"], 2)
                    pace_sampler.confirm(state, "parent", "denied-one", launched=False)
                    pace_sampler.confirm(state, "parent", "denied-two", launched=False)
                    self.assertIsNone(reserve("another"))
                    self.assertIn("2/2", reserve("blocked-again"))
                    rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                    self.assertEqual([r["tool_use_id"] for r in rows if r["event"] == "grant-admission"], ["started"])

    def test_unconfirmed_use_expires_and_late_confirmation_charges_once_with_audit(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            fleet, north = fleet_stub(root, [
                {"name": "parent", "conversation": "parent", "provider": "codex", "status": "working"}])
            peer = CodexPool(root)
            with patch.dict(os.environ, XDG_STATE_HOME=str(root), NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"),
                            NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                            NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite")):
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                pace_sampler.grant("codex", 4, expiry, "parent", "delayed confirmation", "review", 1)
                gate = state / "usage-gate.json"
                path = state / "pace-control.json"
                def reserve(identity):
                    return pace_sampler.reserve(gate, "codex", "codex", session="parent",
                                                task="review", tool_use_id=identity)
                now = time.time()
                with patch.object(pace_sampler.time, "time", return_value=now):
                    self.assertIsNone(reserve("first"))
                with sqlite3.connect(peer.db) as db:
                    db.execute("INSERT INTO threads VALUES ('child',0)")
                    db.execute("INSERT INTO thread_spawn_edges VALUES ('parent','child')")
                with patch.object(pace_sampler.time, "time", return_value=now + pace_sampler.RESERVATION_TTL_S + 1):
                    self.assertIn("all remaining uses reserved", reserve("second"))
                    self.assertEqual(json.loads(path.read_text())["grants"][0]["uses"], 1)
                with patch.object(pace_sampler.time, "time", return_value=now + pace_sampler.GRANT_RESERVATION_HOLD_S - 1):
                    self.assertIn("all remaining uses reserved", reserve("second"))
                with patch.object(pace_sampler.time, "time", return_value=now + pace_sampler.GRANT_RESERVATION_HOLD_S):
                    self.assertIsNone(reserve("second"))
                    for identity in ("first", "first"):
                        pace_sampler.confirm(state, "parent", identity)
                        self.assertEqual(json.loads(path.read_text())["grants"][0]["uses"], 0)
                    pace_sampler.confirm(state, "parent", "second")
                    pace_sampler.confirm(state, "parent", "second")
                    self.assertEqual(json.loads(path.read_text())["grants"][0]["uses"], -1)
                rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                self.assertEqual([r["tool_use_id"] for r in rows if r["event"] == "grant-admission"], ["first", "second"])
                late = [r for r in rows if r["event"] == "grant-late-confirm"]
                self.assertEqual([r["tool_use_id"] for r in late], ["first"])
                self.assertEqual(late[0]["reserved_at"], now)
                self.assertEqual(late[0]["uses"], 0)

    def test_late_confirmation_audit_failure_preserves_retryable_charge(self):
        with tempfile.TemporaryDirectory() as scratch:
            state = Path(scratch)
            path = state / "pace-control.json"
            now = time.time()
            original = {"grants": [{"account": "codex", "session": "parent", "lead": "parent", "uses": 0,
                         "pending": [{"id": "late", "at": now - pace_sampler.GRANT_RESERVATION_HOLD_S - 1,
                                      "confirmed": False}]}]}
            path.write_text(json.dumps(original))
            with patch.object(pace_sampler, "append", side_effect=OSError("audit write failed")):
                with self.assertRaisesRegex(OSError, "audit write failed"):
                    pace_sampler.confirm(state, "parent", "late")
            self.assertEqual(json.loads(path.read_text()), original)
            self.assertFalse((state / "pace-control.jsonl").exists())
            pace_sampler.confirm(state, "parent", "late")
            pace_sampler.confirm(state, "parent", "late")
            charged = json.loads(path.read_text())["grants"][0]
            self.assertEqual(charged["uses"], -1)
            self.assertTrue(charged["pending"][0]["confirmed"])
            rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
            self.assertEqual([r["event"] for r in rows], ["grant-late-confirm", "grant-admission"])
            self.assertEqual(rows[0]["uses"], -1)

    def test_codex_measure_only_refusal_reserves_nothing_before_corrected_retry(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / ".local/state/agents"
            state.mkdir(parents=True)
            fleet, north = fleet_stub(root, [
                {"name": "parent", "conversation": "parent", "provider": "codex", "status": "working"}])
            peer = CodexPool(root)
            with patch.dict(os.environ, XDG_STATE_HOME=str(root / ".local/state"), NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"),
                            SPAWN_CAPACITY_HOME=str(root), NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                            NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite"),
                            AGENT_NO_AUTHORING_HOOKS="0",
                            CODEX_BEHAVIOR_STATE=str(root / "behavior")):
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                pace_sampler.grant("codex", 2, expiry, "parent", "measure-only refusal", "review", 1)
                (state / "usage-gate.json").write_text(json.dumps({"codex": {"refusal": None}}))
                hook = HERE.parent / "hooks/codex-behavior-guard.sh"
                event = {"hook_event_name": "PreToolUse", "tool_name": "spawn_agent",
                         "session_id": "parent", "tool_use_id": "refused", "cwd": str(root),
                         "tool_input": {"task_name": "review", "message": "[routine:watchdog] measure-only"}}
                result = subprocess.run([str(hook)], input=json.dumps(event), text=True,
                                        capture_output=True, check=True, timeout=10)
                decision = json.loads(result.stdout)["hookSpecificOutput"]
                self.assertEqual(decision["permissionDecision"], "deny")
                self.assertIn("only measures", decision["permissionDecisionReason"])
                self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0].get("pending", []), [])
                event["tool_input"]["message"] += "; end with a PEER line"
                event["tool_use_id"] = "corrected"
                result = subprocess.run([str(hook)], input=json.dumps(event), text=True,
                                        capture_output=True, check=True, timeout=10)
                self.assertNotEqual(json.loads(result.stdout or "{}").get("hookSpecificOutput", {}).get("permissionDecision"), "deny")
                pending = json.loads((state / "pace-control.json").read_text())["grants"][0]["pending"]
                self.assertEqual([r["id"] for r in pending], ["corrected"])

    def test_live_and_expired_reservations_charge_each_confirmed_spawn_once(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            fleet, north = fleet_stub(root, [
                {"name": "parent", "conversation": "parent", "provider": "codex", "status": "working"}])
            peer = CodexPool(root)
            with patch.dict(os.environ, XDG_STATE_HOME=str(root), NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"),
                            NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                            NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite")):
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                gate = state / "usage-gate.json"
                def reserve(identity):
                    return pace_sampler.reserve(gate, "codex", "codex", session="parent",
                                                task="review", tool_use_id=identity)
                for uses in (1, 2):
                    with self.subTest(uses=uses):
                        pace_sampler.grant("codex", 4, expiry, "parent", "concurrent confirmation", "review", uses)
                        self.assertIsNone(reserve("first"))
                        child = f"child-{uses}"
                        with sqlite3.connect(peer.db) as db:
                            db.execute("INSERT INTO threads VALUES (?,0)", (child,))
                            db.execute("INSERT INTO thread_spawn_edges VALUES ('parent',?)", (child,))
                        second = reserve("second")
                        if uses == 1:
                            self.assertIsNotNone(second)
                        else:
                            self.assertIsNone(second)
                            pace_sampler.confirm(state, "parent", "second")
                        pace_sampler.confirm(state, "parent", "first")
                        pace_sampler.confirm(state, "parent", "first")
                        self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0]["uses"], 0)
                        self.assertIsNotNone(reserve("third"))
                pace_sampler.grant("codex", 4, expiry, "parent", "late confirmation", "review", 2)
                now = time.time()
                with patch.object(pace_sampler.time, "time", return_value=now):
                    self.assertIsNone(reserve("late"))
                with patch.object(pace_sampler.time, "time", return_value=now + pace_sampler.RESERVATION_TTL_S + 1):
                    self.assertIsNone(reserve("next"))
                    for identity in ("late", "next", "late", "next"):
                        pace_sampler.confirm(state, "parent", identity)
                    self.assertEqual(json.loads((state / "pace-control.json").read_text())["grants"][0]["uses"], 0)
                rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                self.assertEqual([r["tool_use_id"] for r in rows if r["event"] == "grant-admission"],
                                 ["first", "second", "first", "late", "next"])

    def test_numeric_pending_reservations_are_already_charged(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            fleet, north = fleet_stub(root, [
                {"name": "parent", "conversation": "parent", "provider": "codex", "status": "working"}])
            peer = CodexPool(root)
            with patch.dict(os.environ, XDG_STATE_HOME=str(root), NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"),
                            NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                            NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite")):
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                pace_sampler.grant("codex", 2, expiry, "parent", "existing numeric state", "review", 1)
                path = state / "pace-control.json"
                data = json.loads(path.read_text())
                data["grants"][0]["pending"] = [time.time()]
                path.write_text(json.dumps(data))
                pace_sampler.confirm(state, "parent", "unrelated")
                gate = state / "usage-gate.json"
                self.assertIsNone(pace_sampler.reserve(gate, "codex", "codex", session="parent",
                                                      task="review", tool_use_id="new"))
                self.assertEqual(json.loads(path.read_text())["grants"][0]["uses"], 1)
                self.assertIn("2/2", pace_sampler.reserve(gate, "codex", "codex", session="parent",
                                                        task="review", tool_use_id="blocked"))
                pace_sampler.confirm(state, "parent", "new")
                pace_sampler.confirm(state, "parent", "new")
                self.assertEqual(json.loads(path.read_text())["grants"][0]["uses"], 0)
                rows = [json.loads(line) for line in (state / "pace-control.jsonl").read_text().splitlines()]
                self.assertEqual([r["tool_use_id"] for r in rows if r["event"] == "grant-admission"], ["new"])

    def test_grant_subtree_live_workers_and_pending_admissions(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            state = root / "agents"
            state.mkdir()
            sessions = [
                {"name": "parent", "conversation": "parent", "provider": "codex", "status": "working"},
                {"name": "child", "conversation": "subparent", "parent": "parent", "provider": "codex", "status": "working"},
                {"name": "archived", "conversation": "archived", "parent": "parent", "provider": "codex", "status": "working"},
                {"name": "other", "conversation": "otherparent", "provider": "codex", "status": "working"}]
            fleet, north = fleet_stub(root, sessions)
            peer = CodexPool(root)
            threads = ["child", "grandchild", "subparent", "otherparent", "unrelated", "finished", "archived"]
            with sqlite3.connect(peer.db) as db:
                db.executemany("INSERT INTO threads VALUES (?,?)", [(name, name == "archived") for name in threads])
                db.executemany("INSERT INTO thread_spawn_edges VALUES (?,?)", [
                    ("parent", "child"), ("parent", "finished"), ("parent", "archived"),
                    ("subparent", "grandchild"), ("otherparent", "unrelated")])
            previous = dict(os.environ)
            try:
                os.environ.update(XDG_STATE_HOME=str(root), NORTH_BIN=str(north), NORTH_FLEET_FIXTURE=str(fleet), NORTH_FLEET_SENDS=str(root / "fleet-sends"),
                                  NORTH_CODEX_CONVERSATION_HOME=str(peer.pool),
                                  NORTH_CODEX_CONVERSATION_SQLITE_HOME=str(peer.pool / "sqlite"))
                (state / "pace-control.json").write_text(json.dumps({"accounts": {"codex": {"workers": 99}}}))
                gate = state / "usage-gate.json"
                gate.write_text("{}")
                expiry = datetime.fromtimestamp(time.time() + 3600, timezone.utc).isoformat()
                pace_sampler.grant("codex", 3, expiry, "parent", "proxy subtree grant")
                self.assertIsNone(pace_sampler.reserve(gate, "codex", "codex", session="parent"))
                self.assertIn("3/3", pace_sampler.reserve(gate, "codex", "codex", session="parent"))
                with sqlite3.connect(peer.db) as db:
                    db.execute("INSERT INTO threads VALUES ('born',0)")
                    db.execute("INSERT INTO thread_spawn_edges VALUES ('parent','born')")
                sessions.append({"name": "born", "conversation": "born", "parent": "parent", "provider": "codex", "status": "working"}); fleet.write_text(json.dumps(sessions))
                self.assertIn("3/3", pace_sampler.reserve(gate, "codex", "codex", session="parent"))
                sessions[1]["status"] = "idle"; fleet.write_text(json.dumps(sessions))
                command = [sys.executable, "-c", "import sys;sys.path.insert(0,sys.argv[1]);import pace_sampler;from pathlib import Path;print(pace_sampler.reserve(Path(sys.argv[2]),'codex','codex',session='parent') or 'ALLOW')", str(HERE), str(gate)]
                processes = [subprocess.Popen(command, stdout=subprocess.PIPE, text=True) for _ in range(4)]
                results = [p.communicate(timeout=10)[0].strip() for p in processes]
                self.assertEqual(results.count("ALLOW"), 1)
                self.assertEqual(json.loads((state / "pace-control.json").read_text())["accounts"]["codex"]["workers"], 99)
                fleet.write_text(json.dumps([
                    {"name": "parent", "conversation": "claude-parent", "provider": "claude", "account": "c", "status": "working"},
                    {"name": "child", "conversation": "claude-sub", "parent": "parent", "provider": "claude", "account": "c", "status": "working"}, {"name": "live-subagent", "conversation": "claude-live", "parent": "parent", "provider": "claude", "account": "c", "status": "working"},
                    {"name": "other", "conversation": "unrelated-parent", "provider": "claude", "account": "c", "status": "working"}]))
                profile = root / "claude"
                policy = root / "orchestration.toml"
                policy.write_text(f'[accounts.claude.profiles.c."{socket.gethostname()}"]\npath="{profile}"\n')
                vendor = root / "vendor"
                vendor.mkdir()
                command = vendor / "claude"
                command.write_text('#!/bin/sh\necho \'[{"sessionId":"claude-parent","status":"busy"},{"sessionId":"claude-sub","status":"busy"},{"sessionId":"unrelated-parent","status":"busy"}]\'\n')
                command.chmod(0o755)
                os.environ.update(AGENTS_ORCHESTRATION=str(policy), PATH=str(vendor) + os.pathsep + os.environ["PATH"])
                workers = profile / "projects/project/claude-parent/subagents"
                workers.mkdir(parents=True)
                (workers / "agent-live.jsonl").write_text(json.dumps({"type": "assistant", "message": {"content": [{"type": "tool_use"}]}}) + "\n")
                (workers / "agent-finished.jsonl").write_text(json.dumps({"type": "assistant", "message": {"stop_reason": "end_turn", "content": [{"type": "thinking"}, {"type": "text"}]}}) + "\n")
                unrelated = profile / "projects/project/unrelated-parent/subagents"
                unrelated.mkdir(parents=True)
                (unrelated / "agent-foreign.jsonl").write_text(json.dumps({"type": "user", "message": {"content": "running"}}) + "\n")
                pace_sampler.grant("claude:c", 3, expiry, "parent", "proxy Claude subtree grant")
                self.assertIsNone(pace_sampler.reserve(gate, "claude", "claude:c", session="claude-parent"))
                self.assertIn("3/3", pace_sampler.reserve(gate, "claude", "claude:c", session="claude-parent"))
            finally:
                os.environ.clear()
                os.environ.update(previous)


if __name__ == "__main__":
    unittest.main()
