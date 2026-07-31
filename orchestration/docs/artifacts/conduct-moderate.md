# Public artifact source: conduct-moderate

The fenced block is the canonical public artifact. Placeholders are expanded by scripts/emit-artifacts.mjs.

```
# GPT conduct protocol — moderate

The working default: family protocol, propagation law, output norms, and
the human-side tasking rules. Escalate to `strong.md` (adds the calibrated
per-model anchors) for autonomous or long-running lanes; drop to `light.md`
for small bounded tasks.

## Family protocol

{{family}}

## Propagation — this protocol binds your whole tree, not just you

Spawning anything — a sub-agent, a workflow step, another codex session, a
script that calls a model — makes you responsible for propagating this
protocol into it:

- A child that runs in THIS repo root (another `codex exec` here) re-reads
  this AGENTS.md and inherits automatically. That is the only free case.
- A child dispatched any other way — different working directory, an API
  call, a workflow runner, a different tool — inherits NOTHING. Its prompt
  must carry the Family protocol and Calibrated anchors sections verbatim
  (and the task rules of its brief).
- An unpropagated child is an unguarded lane: every failure mode this file
  exists to prevent comes back in it. If you cannot propagate, do not
  spawn — do the work in this session or report the limitation.

## Output norms (all reports)

{{comms}}

## Tasking rules (for the human writing the brief)

The protocol above closes standing drift; your brief must still close the
item-specific holes. Before dispatching an agent:

- B1 — Define every load-bearing noun. Any deliverable term with two
  readings gets one sentence pinning the intended one. Left ambiguous, the
  agent resolves toward whatever makes its own work self-contained.
- B2 — Enumerate the authority floor. Name the specific actions reserved
  to you: deploys, service restarts, pushes, history rewrites, spending.
  "Be careful" is not specific enough to bind.
- B3 — Demand live anchors. For work on a running system: "before
  designing, run <these read-only probes> and quote their output."
- B4 — Set the ceremony budget. Say how big the response should be
  structurally (one branch or several, plan length, whether a rollout
  section is wanted). Silence reads as license.
- B5 — Set the length target. Combined with B4 or expect 3x.
- B6 — End every step with a checkable done-bar: a command + its expected
  output. Accept no bare "done".
```
