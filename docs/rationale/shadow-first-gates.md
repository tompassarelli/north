# Shadow first: a new gate watches before it refuses

Decided 2026-11-07 under north#706. On the night of 2026-11-06 several harness changes went straight to enforcing (the
privacy judge, the spend rule, the compaction checkpoint and the sheriff rules), and correct work was refused before
anyone had seen what the new rule would refuse. A gate's false refusals cost a worker's whole run and read as the
worker's failure; in shadow they cost nothing and are read before they can.

## The rule

A gate is anything that can refuse a launch, a push, a landing or a GitHub write. A new one lands with
`mode = "shadow"` in harness/gates.toml and asks `north_base::gates::decide(ID, refusal)` for each decision: in shadow
the call logs the decision, whether it would refuse and why to `$XDG_STATE_HOME/north/gates/ID.jsonl` and lets the
action through; once the gate enforces, it returns the refusal. A gate switches to `mode = "enforce"` only in a lane
that adds its `[gate.review]`: the reviewer who read its would-be refusals, the span read (`from`, `to`, UTC), its
`decisions`, how many it `would_refuse` and how many of those were `wrong`. The review must cover at least 24 h or 20
decisions and find no wrong refusal; a wrong one goes back to the gate's code and the shadow starts again.

## What holds it

`north land scan gate-shadow BASE..HEAD`, which safe-push runs with its other final scans, refuses a lane that

- adds a Rust file under a gate directory (`crates/north-guard/src/guard/`, `crates/north-landing/src/land/scans/`,
  `crates/north-diff-gates/src/`, `crates/north-safe-push/src/gates/`, tests and fixtures aside) that no gate entry
  names, or
- adds a gate as `enforce`, or switches one from `shadow` to `enforce`, without a review that meets the bar.

`a_lane_adding_an_enforcing_gate_with_no_shadow_record_is_refused_and_its_shadow_twin_is_not` plants both lanes.

The scan is itself a new gate, so it ships in shadow (`gate-shadow` in the registry): until its own review it logs
what it would refuse and lets every lane through. Its review is a box on north#706.

## Limits

The gates that landed before north#706 are not in the registry and keep enforcing; a change to one of them is not
held. The directories catch a new gate file, not a new refusal added inside an existing file or a gate outside them
(a hook in the main crate, a GitHub write check); those are held by the review of the lane that adds them, until a
case shows the directory list needs to grow.
