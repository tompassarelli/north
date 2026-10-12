import json
from pathlib import Path
import re
import tempfile
import unittest

import tick_precheck


class NorthLeadTicks(unittest.TestCase):
    def test_recorded_12_oct_north_lead_ticks(self):
        base = json.loads(Path(__file__).with_name('tick-precheck-2026-10-12.json').read_text())['outputs']
        code, lines, keys = tick_precheck.decide(base, [], 48)
        self.assertEqual(code, 10)

        def shift(text):
            return re.sub(r'(?<=[ :\[,=(])(\d+)(?=[.,m %)\]}/\s])', lambda m: str(int(m.group(1)) + 7), text)

        volatile = {name: shift(base[name]) for name in ('sweep', 'capacity')}
        volatile['unowned'] = re.sub(r'(?m)^([^\t]+\t[^\t]+\t[^\t]+\t)(\d+)', lambda m: m.group(1) + str(int(m.group(2)) + 1), base['unowned'])
        self.assertNotEqual(volatile['unowned'], base['unowned'])
        self.assertNotEqual(volatile['capacity'], base['capacity'])
        self.assertNotEqual(volatile['sweep'], base['sweep'])
        code, lines, keys2 = tick_precheck.decide(volatile, keys, 49)
        self.assertEqual((code, lines), (0, [f'quiet tick=49: priority:now unowned=0 blocked=3 owned=51 flags={len(keys)}']))
        self.assertEqual(keys2, keys)

        report = json.loads(base['capacity'].splitlines()[0])
        report['incidents'] = ['- 2026-10-12T01:30:00Z farm-disk-low: farm VM vast has 18 GB free on /, below 30 GB']
        flagged = {
            'unowned': base['unowned'] + 'north#31\tpriority:now\tUNOWNED\t3\tTick precheck for every lead\n',
            'sweep': base['sweep'] + '0badc0ffee123456  worker  ctx=140k  ran=61m  idle=2m  OVERTIME\n',
            'capacity': json.dumps(report) + '\n' + base['capacity'].splitlines()[1] + '\n',
        }
        code, lines, _ = tick_precheck.decide(flagged, keys, 50)
        self.assertEqual(code, 10)
        self.assertEqual(lines, [
            'unowned: north#31\tpriority:now\tUNOWNED\t3\tTick precheck for every lead',
            'sweep: 0badc0ffee123456  worker  ctx=140k  ran=61m  idle=2m  OVERTIME',
            'capacity: - 2026-10-12T01:30:00Z farm-disk-low: farm VM vast has 18 GB free on /, below 30 GB',
        ])


class StopProjection(unittest.TestCase):
    def test_each_stop_event_wakes_once(self):
        graceful = "STOP worker: graceful cap=88 reading=88.99 in_flight=32 at=2026-10-12T15:00:00+00:00"
        hard = "STOP worker: hard cap=88 reading=89.01 in_flight=32 at=2026-10-12T15:01:00+00:00"
        code, lines, keys = tick_precheck.decide({"sweep": graceful}, [], 1)
        self.assertEqual((code, lines), (10, ["sweep: " + graceful]))
        self.assertEqual(tick_precheck.decide({"sweep": graceful}, keys, 2)[0], 0)
        code, lines, _ = tick_precheck.decide({"sweep": graceful + "\n" + hard}, keys, 3)
        self.assertEqual((code, lines), (10, ["sweep: " + hard]))


class UntakenQueueJobs(unittest.TestCase):
    def test_recorded_12_oct_jobs_queued_behind_job_131_flag_once(self):
        listing = (
            'id   placement  item       state   queued                wait  finished  job_run                                cmd\n'
            '131  nexus      -          taken   2026-10-12T09:30:00Z  32m   -         nexus 2026-10-12T10:02:00Z rc=-        galileo-mutants run --rev 5c1e --shard 0/2 /home/tom/galileo-mutants-0.jsonl\n'
            '132  nexus      -          queued  2026-10-12T09:45:00Z  100m  -         -                                      galileo-full-suite 7a2d --skip differential_1\n'
            '133  nexus      -          queued  2026-10-12T10:55:00Z  30m   -         -                                      galileo-full-suite 7a2d --only differential_1\n'
            '134  follows    north#48   queued  2026-10-12T08:00:00Z  205m  -         -                                      follows north#47 on claude:opus-medium\n'
            '135  pool       -          dropped 2026-10-12T09:50:00Z  20m   2026-10-12T10:10:00Z  -                      galileo-full-suite 61b0\n'
        )
        code, lines, keys = tick_precheck.decide({'queue': listing}, [], 60, 'proxy', [])
        self.assertEqual((code, keys), (10, ['queue untaken 132']))
        self.assertEqual(lines, ['queue: ' + listing.splitlines()[2]])
        code, lines, _ = tick_precheck.decide({'queue': listing.replace('100m', '101m')}, keys, 61, 'proxy', [])
        self.assertEqual(code, 0)
        self.assertEqual(tick_precheck.decide({'queue': listing}, [], 60, 'north-lead', [])[2], [])


class Ownership(unittest.TestCase):
    def test_each_flag_reaches_only_its_owning_lead(self):
        nodes = [
            {'id': 'north-lead', 'role': 'lead', 'domain': 'north', 'parent': 'proxy', 'session': 'deb6397b'},
            {'id': 'muove-lead', 'role': 'lead', 'domain': 'galileo', 'parent': 'proxy', 'session': '4805ab27'},
            {'id': 'muove-codex-lead', 'role': 'sub-lead', 'domain': 'galileo', 'parent': 'muove-lead', 'session': '01a1258b'},
            {'id': 'nexus-lead', 'role': 'lead', 'domain': 'nexus', 'parent': 'proxy', 'session': '3a329eb1', 'repos': ['smashcraft-client']},
        ]
        report = {'minutes': 30, 'incidents': [], 'flags': [
            'heavy lease claude-4.13 median 7.2 cores over its 6 declared org=muove-codex-lead',
            'moderate lease galileo-8.13.0 median 2.4 cores over its 2 declared org=north-lead',
            'cpu PSI above 40% in 31/60 samples (max 62.1)',
        ]}
        outputs = {
            'capacity': json.dumps(report) + '\n',
            'flow': 'galileo: pending 4, landed 0/h over 3h, oldest landing job 41m, stall stuck, actions 3/40 busy\n'
                    'north: pending 0, landed 2/h over 3h, oldest landing job none, stall healthy, actions 3/40 busy\n',
        }
        seen = {who: tick_precheck.decide(outputs, [], 3, tick_precheck.viewer_of(nodes, session), nodes)[1]
                for who, session in (('north', 'deb6397b'), ('muove', '4805ab27'), ('proxy', '95589bba'))}
        self.assertEqual(seen['north'], ['capacity: moderate lease galileo-8.13.0 median 2.4 cores over its 2 declared org=north-lead'])
        self.assertEqual(seen['muove'], [
            'capacity: heavy lease claude-4.13 median 7.2 cores over its 6 declared org=muove-codex-lead',
            'flow: galileo: pending 4, landed 0/h over 3h, oldest landing job 41m, stall stuck, actions 3/40 busy',
        ])
        self.assertEqual(seen['proxy'], ['capacity: cpu PSI above 40% in 31/60 samples (max 62.1)'])

    def test_repo_rows_reach_only_that_repos_lead_and_the_proxy(self):
        nodes = [
            {'id': 'north-lead', 'role': 'lead', 'domain': 'north', 'parent': 'proxy', 'session': 'deb6397b'},
            {'id': 'smashcraft-lead', 'role': 'lead', 'domain': 'smashcraft', 'parent': 'proxy', 'session': '136bfaa7'},
        ]
        outputs = {
            'unowned': 'smashcraft#520\tpriority:now\tUNOWNED\t1\tPeon: Hitboxes reach past the drawn axe on 11 normals\n'
                       'priority:now unowned=1 blocked=30 owned=22\n',
            'sweep': 'WAIT actions: tompassarelli/smashcraft has 2 queued runs, the oldest queued 12 minutes; cut duplicate workflows or batch the pushes.\n',
            'ledger': 'debugging-unknown-cause  medium  -  29  17  58%  51%  40  13  16  0  0.4  2/28  24k  14  2/21\n',
        }
        seen = {who: [line.split(':', 1)[0] for line in
                      tick_precheck.decide(outputs, [], 3, tick_precheck.viewer_of(nodes, session), nodes)[1]
                      if not line.startswith('priority:now') and not line.startswith('quiet')]
                for who, session in (('north', 'deb6397b'), ('smashcraft', '136bfaa7'), ('proxy', '95589bba'))}
        self.assertEqual(seen['north'], [])
        self.assertEqual(sorted(seen['smashcraft']), ['sweep', 'unowned'])
        self.assertIn('unowned', seen['proxy'])


class FinishedSubLeads(unittest.TestCase):
    def test_only_a_finished_line_naming_the_child_finishes_it(self):
        status = '/s/handoffs/codex-lead-status.md'
        peer_file = '/s/handoffs/codex-lead-peer.md'
        nodes = [{'id': 'policy-codex-lead', 'role': 'sub-lead', 'parent': 'policy-lead', 'state': 'active',
                  'status_file': status, 'started': '2026-10-12T13:05:00+0800'},
                 {'id': 'other', 'parent': 'north-lead', 'state': 'active', 'status_file': status}]
        done_at = 1791786810
        live = 'DONE P1 (muove-lead): card v1 landed; K1 (card v2) and Z2 may start once its review lands\n'
        for last, expected in ((live, [('policy-codex-lead', 'done')]),
                               ('FINISHED policy-codex-lead\n', [('policy-codex-lead', 'finish')]),
                               ('FINISHED other-name\n', []),
                               ('FINISHED policy-codex-lead-2\n', [])):
            files = {status: (done_at - 900, 'tick=40\n'), peer_file: (done_at, 'PEER 06:31 X4: checkpoint\n' + last + '\n')}
            self.assertEqual([c[:2] for c in tick_precheck.done_children('policy-lead', nodes, files)], expected, last)
        files = {peer_file: (done_at, 'FINISHED policy-codex-lead\nPEER 06:34 X6: new box\n')}
        self.assertEqual(tick_precheck.done_children('policy-lead', nodes, files), [])

    def test_a_reported_done_line_wakes_the_parent_once(self):
        with tempfile.TemporaryDirectory() as tmp:
            peer = Path(tmp) / 'codex-lead-peer.md'
            nodes = [{'id': 'policy-lead', 'role': 'lead', 'session': 'parent-session'},
                     {'id': 'policy-codex-lead', 'role': 'sub-lead', 'parent': 'policy-lead', 'state': 'active',
                      'status_file': str(Path(tmp) / 'codex-lead-status.md')}]
            keys, codes = [], []
            for last in ('DONE P1 (muove-lead): card v1 landed', 'DONE P1 (muove-lead): card v1 landed',
                         'DONE K1: card v2 landed'):
                peer.write_text(f'PEER 06:31 X4: checkpoint\n{last}\n')
                finished, done = tick_precheck.finish_done('parent-session', nodes)
                code, lines, keys = tick_precheck.decide({}, keys, 40, 'policy-lead', nodes, done)
                codes.append(code)
            self.assertEqual(codes, [10, 0, 10])


if __name__ == '__main__':
    unittest.main()
