#!/usr/bin/env python3
from importlib.machinery import SourceFileLoader
from pathlib import Path
import unittest

lint = SourceFileLoader("issue_box_lint", str(Path(__file__).with_name("issue-box-lint"))).load_module()


class RecordedSmashcraftBoxes(unittest.TestCase):
    def test_smashcraft_12_oct_boxes_flag_only_mechanical_checks_routed_to_tom(self):
        body = "\n".join([
            "- [ ] Tom's play session on the current patch build (findings go into the next patch). Product proof (laptop, Tom).",
            "- [ ] Product proof (laptop, Tom's next launch): the controller helper builds on Tom's laptop for the current build (`bun wisp play` prints a helper path, not \"keyboard\").",
            "- [ ] Tom (note 7): \"the cursor should start on \"start match\" on the screen selection screen\". Entering stage select puts the pad focus and the mouse cursor on Start Match.",
            "- [ ] Tom tries it on his pad in his next playtest.",
            "- [ ] Tom's feel check in his next playtest.",
            "- [x] Tom tries the old pad build.",
            "- [ ] Frame native time is measured once per release candidate on Tom's machine, by one quiet laptop run under an exclusive lease.",
        ])
        self.assertEqual([box[:40] for box in lint.flagged(body)], [
            "Product proof (laptop, Tom's next launch",
            "Tom tries it on his pad in his next play",
        ])


if __name__ == "__main__":
    unittest.main()
