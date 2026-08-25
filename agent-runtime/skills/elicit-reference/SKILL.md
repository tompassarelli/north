---
name: elicit-reference
description: Detailed prompt, subtraction examples, and compilation shape for model-delta calibration. Use only when elicit-distilled calls for procedural detail.
---

# Elicit reference

## Introspection prompt shape

Ask the target model to describe, in first person:

- its real software-engineering behavior and style;
- what it prioritizes and deliberately omits;
- how it changes across a small fix, vague large task, debugging, and greenfield work;
- where it struggles; and
- what internal or observable signs tell it that it is out of depth.

Request roughly 150–250 lines of Markdown written to the chosen path. End the prompt by embedding the contamination requirement from `elicit-distilled` as an explicit strict instruction.

## Subtraction examples

| Self-report evidence | Classification | Delta form |
| --- | --- | --- |
| “I lose track across long dependency chains” | named limit | Write one contract line per layer hop. |
| “I get quieter when unsure” | internal tell | Give uncertainty more words, not fewer. |
| “I cannot run code” in a tool-enabled runtime | stale belief | Run the code instead of predicting it. |
| A preached check repeatedly dropped under momentum | known-but-skipped | Add a written one-line checkpoint. |

## Compiled shape

Use a short trust-the-canon preamble, then numbered items grouped by work phase. Phrase each item with words drawn from the self-report and make each demand one written answer. Use the completion limits and destinations from `elicit-distilled`.
