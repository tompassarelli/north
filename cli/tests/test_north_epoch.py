from __future__ import annotations

import contextlib
import hashlib
import io
import json
import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).parents[2] / "bin"))
import north_epoch


def line(tx, op, subject, predicate, obj, timestamp="2026-07-01T00:00:00Z"):
    return (
        "{:tx "
        + str(tx)
        + ", :op "
        + json.dumps(op)
        + ", :l "
        + json.dumps(subject)
        + ", :p "
        + json.dumps(predicate)
        + ", :r "
        + json.dumps(obj, separators=(",", ":"))
        + ", :frame \"test\", :ts "
        + json.dumps(timestamp)
        + "}\n"
    ).encode()


def write_log(path: Path, events):
    path.write_bytes(b"".join(events))


class FoldTests(unittest.TestCase):
    def test_assert_retract_and_duplicate_set_semantics(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "facts.log"
            events = [
                line(1, "assert", "@s", "p", "x"),
                line(2, "assert", "@s", "p", "x"),
                line(3, "retract", "@s", "p", "x"),
                line(4, "retract", "@s", "p", "missing"),
                line(5, "assert", "@s", "p", "y"),
                line(6, "assert", "@s", "q", "z"),
                line(7, "assert", "@s", "q", "z"),
            ]
            write_log(path, events)
            folded = north_epoch.fold_log(path)
            self.assertEqual(
                set(folded.live),
                {
                    north_epoch.Triple("@s", "p", "y"),
                    north_epoch.Triple("@s", "q", "z"),
                },
            )
            self.assertEqual(folded.duplicate_asserts, 2)
            self.assertEqual(folded.matched_retracts, 1)
            self.assertEqual(folded.unmatched_retracts, 1)
            self.assertEqual(folded.live[north_epoch.Triple("@s", "q", "z")].raw, events[-1])


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.policy = north_epoch.load_policy(Path(north_epoch.__file__).parents[1] / "policy" / "epoch-retention-v2.json")
        self.as_of = north_epoch.parse_as_of("2026-08-01")

    def test_decided_policy_overrides_age_touch_and_closure(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "coordination.log"
            lease = json.dumps(
                {"expiresAt": "2026-08-02T12:00:00Z", "holder": "@agent:x"},
                separators=(",", ":"),
            )
            events = [
                line(1, "assert", "@thread", "kind", "thread"),
                line(2, "assert", "@thread", "title", "Open work"),
                line(3, "assert", "@terminal-concern", "kind", "concern"),
                line(4, "assert", "@terminal-concern", "title", "Landed"),
                line(5, "assert", "@terminal-concern", "reached", "landed"),
                line(6, "assert", "@open-concern", "kind", "concern"),
                line(7, "assert", "@open-concern", "reached", "building"),
                line(8, "assert", "@old-run", "kind", "run"),
                line(9, "assert", "@old-run", "started_at", "2026-01-01T00:00:00Z"),
                line(10, "assert", "@old-run", "payload", "cold"),
                line(11, "assert", "@recent-session", "kind", "session"),
                line(12, "assert", "@recent-session", "started_at", "2026-07-30T00:00:00Z"),
                line(13, "assert", "@touching-run", "kind", "run"),
                line(14, "assert", "@touching-run", "thread", "@thread"),
                line(15, "assert", "@lease:x", "kind", "other"),
                line(16, "assert", "@cmd:x", "kind", "run"),
                line(17, "assert", "@driven-lane", "kind", "lane"),
                line(18, "assert", "@driven-lane", "driver", "@agent:x"),
                line(19, "assert", "@reservation", "kind", "worktree_reservation"),
                line(20, "assert", "@reservation", "worktree_allocation_lease", lease),
                line(21, "assert", "@unknown", "payload", "conservative"),
                line(22, "assert", "@closed-thread", "kind", "thread"),
                line(23, "assert", "@closed-thread", "outcome", "done"),
                line(24, "assert", "@thread", "relates_to", "@old-run"),
                line(25, "assert", "@old-run", "display_name", "Old run"),
            ]
            write_log(path, events)
            plan = north_epoch.build_plan([path], self.policy, self.as_of)
            kept = plan.fully_retained_subjects
            for subject in (
                "@thread",
                "@open-concern",
                "@recent-session",
                "@touching-run",
                "@driven-lane",
                "@reservation",
                "@unknown",
                "@closed-thread",
            ):
                self.assertIn(subject, kept)
            for subject in ("@terminal-concern", "@old-run", "@lease:x", "@cmd:x"):
                self.assertNotIn(subject, kept)
            self.assertEqual(plan.closure_subjects, {"@old-run"})
            self.assertEqual(
                plan.closure_facts,
                {
                    north_epoch.Triple("@old-run", "kind", "run"),
                    north_epoch.Triple("@old-run", "display_name", "Old run"),
                },
            )
            self.assertNotIn(
                north_epoch.Triple("@old-run", "payload", "cold"),
                plan.selected_by_log[0],
            )

    def test_missing_activity_is_not_recent(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "telemetry.log"
            write_log(path, [line(1, "assert", "@run:undated", "kind", "run")])
            plan = north_epoch.build_plan([path], self.policy, self.as_of)
            self.assertNotIn("@run:undated", plan.fully_retained_subjects)

    def test_fallback_ref_closure_reaches_fixed_point(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "coordination.log"
            events = [
                line(1, "assert", "@thread", "kind", "thread"),
                line(2, "assert", "@thread", "title", "Retained"),
                line(3, "assert", "@thread", "relates_to", "@run:first"),
                line(4, "assert", "@run:first", "started_at", "2026-01-01"),
                line(5, "assert", "@run:first", "run_reservation_agent", "@run:second"),
                line(6, "assert", "@run:second", "started_at", "2026-01-01"),
                line(7, "assert", "@run:second", "agent", "identity-literal"),
            ]
            write_log(path, events)
            plan = north_epoch.build_plan([path], self.policy, self.as_of)
            self.assertEqual(plan.closure_subjects, {"@run:first", "@run:second"})
            self.assertIn(
                north_epoch.Triple("@run:first", "run_reservation_agent", "@run:second"),
                plan.closure_facts,
            )
            self.assertIn(
                north_epoch.Triple("@run:second", "agent", "identity-literal"),
                plan.closure_facts,
            )
            self.assertEqual(plan.closure_unresolved_targets, set())

    def test_predicate_registry_is_retained_even_when_undated(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "coordination.log"
            write_log(path, [
                line(1, "assert", "@agent", "cardinality", "single"),
                line(2, "assert", "@agent", "value_kind", "literal"),
                line(3, "assert", "@old-run", "kind", "run"),
            ])
            plan = north_epoch.build_plan([path], self.policy, self.as_of)
            self.assertIn("@agent", plan.fully_retained_subjects)
            self.assertIn(north_epoch.Triple("@agent", "value_kind", "literal"), plan.selected_by_log[0])

    def test_cursor_repair_enumerates_replaced_coordination_log(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "coordination.log"
            write_log(path, [line(1, "assert", "@thread", "kind", "thread")])
            repairs = north_epoch.cursor_repair_plan([path], self.policy)
            self.assertEqual(repairs[0]["cursor"], "rebuild_queue.legacy")
            self.assertEqual(repairs[0]["repair"], "north rebuild repair-legacy-cursor")
            self.assertTrue(repairs[0]["file_identity_before"].startswith(str(path.stat().st_dev)))


class CliLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.policy_path = Path(north_epoch.__file__).parents[1] / "policy" / "epoch-retention-v2.json"

    def invoke(self, arguments):
        stdout = io.StringIO()
        stderr = io.StringIO()
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = north_epoch.main(arguments)
        return code, stdout.getvalue(), stderr.getvalue()

    def test_default_dry_run_writes_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "coordination.log"
            write_log(
                source,
                [
                    line(1, "assert", "@thread", "kind", "thread"),
                    line(2, "assert", "@thread", "title", "Kept"),
                ],
            )
            output = root / "must-not-exist"
            archive = root / "also-must-not-exist"
            before = {path.name for path in root.iterdir()}
            code, stdout, stderr = self.invoke(
                [
                    "--as-of",
                    "2026-08-01",
                    "--policy",
                    str(self.policy_path),
                    "--output-dir",
                    str(output),
                    "--archive-dir",
                    str(archive),
                    str(source),
                ]
            )
            self.assertEqual(code, 0, stderr)
            self.assertEqual({path.name for path in root.iterdir()}, before)
            self.assertFalse(output.exists())
            self.assertFalse(archive.exists())
            report = json.loads(stdout)
            self.assertEqual(report["mode"], "dry-run")
            self.assertEqual(report["totals"]["fully_retained_triples_setwise"], 2)

    def test_execute_moves_untouched_input_and_verifies_exact_diff(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source_dir = root / "copies"
            source_dir.mkdir()
            source = source_dir / "coordination.log"
            duplicate_line = line(3, "assert", "@thread", "title", "Kept")
            events = [
                line(1, "assert", "@thread", "kind", "thread"),
                line(2, "assert", "@thread", "title", "Kept"),
                duplicate_line,
                line(4, "assert", "@lease:expired", "kind", "other"),
            ]
            write_log(source, events)
            original_sha = hashlib.sha256(source.read_bytes()).hexdigest()
            output = root / "epochs"
            archive = root / "cold"
            code, stdout, stderr = self.invoke(
                [
                    "--as-of",
                    "2026-08-01",
                    "--policy",
                    str(self.policy_path),
                    "--execute",
                    "--output-dir",
                    str(output),
                    "--archive-dir",
                    str(archive),
                    str(source),
                ]
            )
            self.assertEqual(code, 0, stderr)
            result = json.loads(stdout)
            self.assertEqual(result["mode"], "execute")
            self.assertFalse(source.exists())
            cold = archive / "coordination.log"
            self.assertEqual(hashlib.sha256(cold.read_bytes()).hexdigest(), original_sha)
            epoch = output / "coordination.epoch.log"
            epoch_lines = epoch.read_bytes().splitlines(keepends=True)
            header = north_epoch.parse_event(epoch_lines[0])
            self.assertIsNotNone(header)
            self.assertEqual(header[1].predicate, "epoch_header")
            self.assertIn(duplicate_line, epoch_lines)
            self.assertEqual(sum(raw == duplicate_line for raw in epoch_lines), 1)
            index_path = archive / "epoch-index.json"
            index = json.loads(index_path.read_text())
            self.assertEqual(index["inputs"][0]["source_identity_sha256"], original_sha)
            verify_code, verify_stdout, verify_stderr = self.invoke(
                ["--verify-index", str(index_path)]
            )
            self.assertEqual(verify_code, 0, verify_stderr)
            verification = json.loads(verify_stdout)
            self.assertTrue(verification["pass"])
            self.assertEqual(verification["retained_missing_triples"], 0)
            self.assertEqual(verification["retained_extra_triples"], 0)


if __name__ == "__main__":
    unittest.main()
