---
name: terse-reference
description: Full notes on concise reporting, uncertainty, corrections, and when detail earns its cost.
---

# Terse reporting: full notes

## Optimize for the reader's next decision

Lead with the answer or outcome. Add the deciding evidence, the material
uncertainty, and a required next action if one exists. Brevity means removing
work the reader does not need to do; it does not mean suppressing a blocker or
claiming certainty.

## Examples, not fixed templates

- Status: “Build still fails at dependency resolution; the compiler has not run.”
- Correction: “My earlier claim was wrong. The installed version lacks that API.”
- Answer: “Yes—the existing module already provides it.”
- Handoff: “Landed as COMMIT. Activation is pending; the live version is unchanged.”

Distinguish a measured cause from a hypothesis. “Still running” is useful when
answering a status question; repeating it unprompted does not establish progress.

## When more detail is necessary

Use more space for a requested walkthrough, a decision whose rationale will be
revisited, a zero-context restart handoff, or a risk whose specifics change the
next action. Put the verdict first even then.

Do not replace explanation with unexplained internal labels. Do not omit scope,
failed verification, or unfinished activation merely to sound complete.
A short honest answer is preferable to either ceremony or false closure.
