#!/usr/bin/env python3
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "socrates/lib"))
from chrome_tabs import close_candidates


class RecordedOwnership(unittest.TestCase):
    def test_dead_live_membership_boundary(self):
        record = {"owner_start": "10", "child_start": "20", "group_id": 30, "tabs": [40, 41]}
        self.assertEqual(close_candidates(record, None, None, 30, [40, 41, 99]), [40, 41])
        self.assertEqual(close_candidates(record, "10", None, 30, [40, 41]), [])
        self.assertEqual(close_candidates(record, None, "20", 30, [40, 41]), [])
        self.assertEqual(close_candidates(record, "11", "21", 30, [40, 99]), [40])
        self.assertEqual(close_candidates(record, None, None, 31, [40, 41]), [])


if __name__ == "__main__":
    unittest.main()
