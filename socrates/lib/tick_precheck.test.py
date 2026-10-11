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


if __name__ == '__main__':
    unittest.main()
