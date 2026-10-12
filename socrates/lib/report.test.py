from datetime import datetime, timezone
import json
from pathlib import Path
import sqlite3
import tempfile
import unittest

import calibration
import report
from stall_recovery import recovery

FIXTURE = Path(__file__).with_name('report-fixture')


class Golden(unittest.TestCase):
    def test_report_renders_the_fixture_state(self):
        now = datetime(2026, 10, 12, 8, 0, tzinfo=timezone.utc)
        with tempfile.TemporaryDirectory() as tmp:
            db = Path(tmp) / 'snapshot.db'
            with sqlite3.connect(db) as conn:
                conn.execute('CREATE TABLE calibration(at TEXT, kind TEXT, cell TEXT, n INT, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT)')
                conn.executemany('INSERT INTO calibration VALUES (?,?,?,?,?,?,?,?,?)', json.loads((FIXTURE / 'calibration.json').read_text()))
                conn.execute('CREATE TABLE runs(agent TEXT, arm TEXT, follows TEXT, outcome TEXT, expects_landing INT, landed INT, role TEXT)')
                conn.executemany("INSERT INTO runs VALUES (?,?,?,?,?,?,'worker')", json.loads((FIXTURE / 'runs.json').read_text()))
            lines = report.render(calibration.rows(db), recovery(FIXTURE / 'worker-sweep', '2026-10-05'),
                                  report.jsonl(FIXTURE / 'spawn-gate-denials.jsonl'), report.jsonl(FIXTURE / 'goal-scores.jsonl'), now,
                                  report.arm_runs(db))
        self.assertEqual('\n'.join(lines) + '\n', (FIXTURE / 'expected.txt').read_text())

    def test_empty_state_says_so(self):
        now = datetime(2026, 10, 12, 8, 0, tzinfo=timezone.utc)
        self.assertEqual(report.render([], [], [], [], now), [
            'calibration: no calibration yet',
            'escalation arms: escalate 0/0 [0%-100%], cheap-fix 0/0 [0%-100%], split 0/0 [0%-100%] (needs 15 runs per arm)',
            'spawn-gate denials 7d: 0 (capacity 0, flow 0, pace 0)',
            'goal scores: none scored yet',
            'goal ETA: none predicted yet',
        ])


if __name__ == '__main__':
    unittest.main()
