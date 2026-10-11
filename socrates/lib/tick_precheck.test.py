import json
from pathlib import Path
import re
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


if __name__ == '__main__':
    unittest.main()
