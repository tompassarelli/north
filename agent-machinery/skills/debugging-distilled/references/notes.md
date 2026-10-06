# Debugging notes

Long-form half of `debugging-distilled`. Read for re-distillation or a named
detail; the distilled guide is the operating surface.

## Why the skill exists

The operator asked for a durable rule after repeatedly reminding agents to
research and isolate instead of freewheeling. The bootstrap carries the short
rule so it applies without the skill loaded; this skill carries the procedure.

## Originating incident (2026-10-06)

A Warcraft III map's imported assets failed to load on one launch path and
loaded on another. An agent spent hours relaunching with different
combinations, often changing two variables per attempt, and judged results by
screenshot and OCR. It did not:

1. search the web for the exact error or symptom;
2. isolate variables with one decisive experiment;
3. trace the good and bad launches (strace, `WINEDEBUG`, socket capture, logs)
   and diff them to the first divergence;
4. use the project's documented menu WebSocket, which already exposed the
   state it was screenshotting.

Each omission maps to one section of the distilled guide.

## Deliberate choices

- The trigger is any non-trivial bug or unexplained behavior, not reverse
  engineering. Tracing and binary tools are one observation section.
- The search pass is bounded (a handful of queries) so it stays cheap; it is
  not a prior-art survey. `prior-art-distilled` owns design-choice research.
- "Two failed fixes" matches the bootstrap's existing two-failure stop and
  inserts research plus a differential trace before the third attempt.
- No hook enforces this: whether an attempt was a guess is not mechanically
  decidable.
