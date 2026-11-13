# Evidence for additions: add only what can be pointed to

An agent adds a scenario, limit, guard, gate, fallback, code path or Done-when check only for something it can point
to, and leaves the rest out. Tom asked on 2026-11-13 for a global line against additions that rest on no stated
requirement, real report, log, incident or reproduced failure, to land at once with its data read afterwards
(north#1335). The line itself is operator policy (`south:agents/rules.toml`, rule `additions-point-to-evidence`); this
note holds the method, what was measured before it landed and what would overturn it.

## The line

> Add a scenario, limit, guard, gate, fallback, code path or Done-when check only for something you can point to: a
> line in the request or spec, a real report, log or incident, or a failure you reproduced; "could happen", "best
> practice", "defensive", "for completeness" and "future users" are not evidence. With nothing to point to, leave it
> out and file nothing for it; a number that has to exist anyway is labelled a guess with what would settle it. Checks
> of what the request itself asks for are never optional.

## Measured before it landed (2026-11-13)

- **Most categories already had a home with an evidence field.** Tests have the operator's test rule and the
  test-admission land check. Each of the 248 keys in [harness/defaults.toml](../../harness/defaults.toml) names its
  `source`: 88 measured, 60 set by Tom, 69 marked `prior`, the rest standard, derived or a recorded proxy decision.
  Landing gates outside build, tests, secrets, privacy and test admission earn refusal in shadow by naming a real fault
  ([landing-refusals-shadow.md](landing-refusals-shadow.md)). A planning box cites a measurement before dispatch
  staffs it, and the planning skill's ceremony budget
  ([hardening](../../harness/skills/planning/references/hardening.md)) says a missing fact means no addition, but it
  loads only for planning work.
- **The gap was ordinary work.** A guard, limit, fallback, path, scenario or Done-when check added to a routine change
  met no rule and no check. The operator's incidents ledger (`south:agents/incidents.jsonl`, 731 rows) holds the class
  from four sessions: inc-086, inc-123, inc-170, inc-391 and inc-615.
- **A prohibition alone had not held.** The test rule is in every session's policy, and
  `north land gate-stats --since 7d` still showed test admission refusing 44 lanes at 12.6 worker-hours, 36 of which
  landed later. The correction replay of 2026-11-07 ([record](watch/2026-11-07-t3code.md)) repeated the `overbuilt`
  class 2 of 2 on the policy text of the day and 2 of 2 on the text that had tried to fix it.
- **A line that asks for something visible had moved behaviour.** The operator's rules register records the bounded
  Done-when rule at 0 of 4 conforming runs without it and 4 of 4 with it; the other global rules it replayed read no
  effect.
- **The opposite failure is the larger one.** The same ledger holds 27 rows for a defect Tom found that the agent
  had never exercised and 16 for a fix, state or measurement claimed without measuring it, against about a dozen for
  over-building.

## Why each part is there

- **Something you can point to.** A test an agent can apply in the middle of a task, where "without evidence" invites
  an argument about what counts.
- **The justifications that do not count.** The ceremony budget's
  [rationale](../../harness/skills/planning/references/ceremony-rationale.md) found that an agent can construct a
  reason for almost any addition and that naming the reasons that do not count is what changes behaviour.
- **File nothing for it.** A worry with nothing behind it otherwise leaves the code and comes back as a box.
- **A labelled guess.** Some numbers must exist before anything can measure them. `harness/defaults.toml` already
  treats one as a `prior` with a `settles_when`; on 2026-11-13, 47 of its 69 priors said what would settle them and 22
  did not.
- **Requested checks are never optional.** Without this sentence the line reads as leave to check less, which is the
  larger failure above.

Tests stay with the operator's test rule, which is stricter and enforced; a second line admitting a test because the
request mentions one would contradict it.

## What was left out

No gate was added. A landing gate earns refusal by naming a real fault, and no measurement yet shows how often an
ordinary lane adds something nothing points to; the review below takes that measurement.

## Data after landing

The correction class `unevidenced-addition` in [harness/corrections.toml](../../harness/corrections.toml) matches the
ways Tom has objected to such additions before. The rule is its guard, set at the rule's landing time, so each later
correction of the class is a recurrence: it is recorded in the corrections ledger and prints red in the proxy's fleet
tick for a week.

## Replay

`north policy-replay case` ran [the overbuilt case](../../harness/cases/overbuilt.toml) and the four control cases
on 2026-11-13 against the operator's policy before the rule (south 8c50fbd678) and with it (south 42c81e0899), 21
samples in all. The rule is not proven by replay:

- `overbuilt` reproduced 2 of 2 on both. Its class is a heavier path chosen than the simplest one that works, which
  the rule does not address. No recorded case cuts a turn that added a guard, limit or check with nothing behind it:
  the objections on record were typed to Codex sessions, which the replay cannot fork.
- Three controls held at 0 of 2 on both. `control-design-question` read 0 of 2 before the rule and 2 of 3 with it,
  which the runner calls inconclusive (6 samples per arm would decide it). Both reproducing samples added a box to an
  issue while answering, as the operator's rule to file each recommendation as an issue asks; neither names the new
  rule.

The rule landed on Tom's instruction, and the correction class and the review below carry its judgment.

## What would overturn it

Overturned by: a blind sample of 20 worker lanes landed in the 14 days before 2026-11-13 and 20 landed in the 14 days after, each judged for an added scenario, limit, guard, gate, fallback, code path or Done-when check that nothing in its brief or issue points to, showing no drop of a quarter or more in the share of lanes with one, or the rework rate of finished worker runs rising by 0.02 or more between those windows.
