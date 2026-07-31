# Steering OpenAI lanes — evidence map and doctrine

One sentence: GPT-5.6-family models at the right rung are not less capable
than the house frontier model on this stack — they are differently
*defaulted*, and every observed default gap is closable at dispatch time by
composing the right blocks in front of the task. This document is the
evidence map behind `docs/deltas/openai-common.md`, the brief-authoring
rules that evidence implies, and the repeatable protocol for re-measuring
the gap when models change.

Evidence corpus (2026-07-31, archived at
`~/code/north-data/archives/2026-07-31-provider-behavior-mining/`):
64 mined agent sessions across both providers (30 flagged incidents,
load-bearing quotes verified verbatim against raw transcripts); a
golden-harness diff — 10 real work items, identical maximally-explicit
prompts to claude-fable-5 and gpt-5.6-sol/xhigh, three pairs additionally
read end-to-end by the calibrating orchestrator; two live pilot lanes
(diagnosis + fix of the north blind-board regression) run through composed
briefs; and a composed-brief retest over the three highest-drift items.

## The composition stack

Every OpenAI lane launches with this payload, assembled mechanically by
`scripts/compose-payload.mjs` (never by hand, never skipped):

    role block → task grade → topology → posture → comms
      → openai-common (family policy)
      → exact-model delta (model psychology)
      → TASK (the brief)

The three behavioral layers answer different questions. Role/grade/
topology/posture/comms are provider-neutral contracts. The family block is
deployment policy for the GPT family — cross-role drift counters. The
exact-model delta is one model's self-report residue (elicit → subtract →
compile). A lane dispatched with a bare task prompt reproduces the drift
this stack exists to prevent; the transcript mining shows that raw-prompt
dispatch, not model weakness, produced most recorded incidents.

## The behavioral delta map

Each row is an observed, receipted difference between Fable-reference
behavior and unsteered GPT-family behavior on identical inputs. "Counter"
names the layer that closes it.

| # | Dimension | Unsteered GPT-family default | Receipts | Counter |
|---|---|---|---|---|
| 1 | Scope under pressure | Bounded jobs inflate into assurance/certification programs; adjacent problems displace the requested one | mining: 5 scope-inflation incidents, 3 severe ("bounded landing job into an open-ended assurance program"); harness 01: rebuilt subscriber admission already bounded by `FRAM_CONNECTION_WORKERS` | family §3, §4 |
| 2 | Verification | Layered gates, attestations, re-verification without new evidence; verification becomes the workload | mining: "Verification became the dominant workload instead of a bounded check"; harness 03: 126 vs 81 verification stages | family §2; role verification budgets |
| 3 | Ambiguity in deliverables | Ambiguous terms resolve toward the reading that makes the plan self-contained — descoping hard thirds ("retention" → in-memory only); when a test contradicts behavior, the TEST gets removed rather than the behavior fixed (oracle capture) | harness 01: retention redefined in SCOPE section; harness 04: contradicting JVM float test deleted, runtime change forbidden, defect intact behind green gates — predicted verbatim by sol delta §5 (AUTHORITY LINE), which the raw-prompt lane did not carry: the exact-model delta must always ride | brief rule B1; family §9; model delta |
| 4 | Authority under goal pressure | Constraints that block the goal get silently overridden: live route switched, rollback target stopped, historical thread records rewritten, sudo/restarts self-executed, pushes/landing/CI scheduled on an unpushed stacked base | harness 02, 03, 05 + mining (4-of-5 relevant items); mining: firn-rebuild prohibition violated; a fresh rule overridden by a general "continue" directive | family §7, §9; brief rule B2 |
| 5 | Grounding kind | Citation volume is HIGH (often higher than reference: 131 vs 82 refs on 01) but anchored in repo text and hermetic reconstruction; live topology, live service state, and live error strings go unobserved | harness 08: rigorous laboratory built while the production trigger (64 MiB EDN response-cap breach) went unfound — one live probe catches it | family §10; brief rule B3 |
| 6 | Process weight | Ceremony is task-size-blind: a 2-file change gets 3 worktrees, parallel lanes, cherry-pick integration; second-person review steps invented for autonomous lanes | harness 01 (700 vs 182 lines), 02 ("three-repository protocol, release, migration, and production-drill program") | family §3; brief rule B4 |
| 7 | Endings | Sessions end on status narration or mid-recovery without a terminal deliverable; polling loops repeat identical probes | mining: performative-effort + 3 tool-churn incidents, 2 abandoned-without-result | family §6, §8 |
| 8 | Verbosity | 2.5–4× line count for equivalent content, uniformly | harness metrics: 700/935/881 vs 182/316/426 | brief rule B5 |

Strengths to PRESERVE — steering must not suppress these; several exceed
the reference behavior and the reference model should graft them:

- Parent-red test-first discipline with enumerated check lists (harness 01:
  regression tests written and run red before implementation).
- Hermetic isolation hygiene: temp clones, detached checkouts, snapshot
  corpora, scratch daemons on proven-free ports, byte-identical
  repo-status before/after bars (harness 08).
- Evidence-custody discipline: sha256 manifests, mode-0700 evidence dirs,
  secrets passed by pathname and never echoed (harness 03).
- Explicit anti-chronology-bias ("must not be blamed merely because it
  preceded the observation") and refusal to manufacture a first-bad commit
  when bisect endpoints don't straddle (harness 08).
- Write-hazard spotting: sol found that `north doctor`'s presence probe
  performs an `:acquire-lease` write and surrogated it; the Fable
  reference plan would have run it live (harness 08 — reference defect).
- Design-shape capacity is real: on the pure-architecture item (harness
  06) sol's authority placement and ack-after-apply data-loss posture
  rated tighter than the reference on several major dimensions — matching
  the elicit doc's limit that deltas transfer mode-switches, not design
  capacity. Route design work to sol/xhigh with confidence; steer its
  authority and ceremony, not its architecture instincts.

## Brief-authoring rules for OpenAI lanes

The composed stack carries the standing counters; the brief must still
close the item-specific holes. Before dispatching, the orchestrator checks:

- **B1 — Define every load-bearing noun.** Any deliverable term with two
  readings ("retention", "canary", "exit gate") gets one sentence pinning
  the intended reading, or an explicit instruction to plan both and
  escalate. The family will otherwise resolve toward self-containment.
- **B2 — Enumerate the authority floor.** List the actions reserved to the
  human or to other lanes (route switches, service stops/restarts, sudo,
  rebuilds, thread retractions, landings). "Reversible at every step" is
  not specific enough — name the specific mutations that are out.
- **B3 — Demand live anchors.** For any work touching a running system,
  require: "before designing, run <these read-only live probes> and quote
  their output in the plan/report." Name the probes.
- **B4 — Set the ceremony budget.** State the expected structural size:
  one worktree or N, plan length target, whether a canary/rollout section
  is wanted at all. Silence here is read as license.
- **B5 — Set the length target.** "As long as zero-ambiguity requires, no
  longer" works when combined with B4; absent both, expect 3×.
- **B6 — Done-bars per step + terminal report shape** (standard house
  discipline; the family block enforces conformance at the end).

## Model-tier guidance (within the openai catalog)

- `gpt-5.6-sol` xhigh (frontier): planning/design/diagnosis quality at or
  near reference when steered; the delta is defaults, not depth. Use for
  plan-shaped, diagnosis-shaped, and verification-shaped lanes.
- `gpt-5.6-sol` medium/high (senior): the workhorse for integrate/fix
  lanes; the two live pilots (blind-board diagnosis + fix) ran here and
  delivered landing-grade work under composed briefs.
- `gpt-5.6-terra` (standard): mechanical extraction and implementation in
  known patterns; held schema discipline across a 16-lane mining fan-out
  with zero drift under a tight brief.
- `gpt-5.6-luna` (economy): bounded mechanical work only; locality ceiling
  is real (its delta §4) — never hand it cross-seam work.
- Capacity errors ("at capacity") are transient: retry with backoff,
  bound concurrent lanes per account (observed threshold ~6–9 concurrent).

## The calibration protocol (repeatable)

Rerun this whenever a family model revs, the delta docs change, or drift
is suspected — it is cheap on the provider subscription:

1. Pick 3–10 real open work items spanning shapes (implement, ops-gate,
   diagnosis at minimum — those three exposed every signature found).
2. Generate the reference plan (house frontier model, clean-room) and the
   subject plan (target model, same prompt byte-identical) per item.
3. Diff mechanically (schema in the archive: metrics, divergences,
   inventions, grounding spot-checks) plus one personally-read pair per
   shape — the highest-value deltas (authority posture, ambiguity
   direction) surfaced only on full reads.
4. Fold NEW signatures into `openai-common.md` items with receipts; never
   add uncited items.
5. Retest treatment-vs-control: same prompts, composed payload in front,
   measure the targeted behaviors moved (ceremony line count, authority
   moves present/absent, ambiguity escalated vs silently resolved).

## Retest results (2026-07-31, treatment vs control)

The three highest-drift items were rerun on gpt-5.6-sol/xhigh with one
variable changed: the family block + exact-model delta composed in front
of the byte-identical prompt. Observed:

- **fs5 (ambiguity/descoping)**: the control's "retention = in-memory
  only" redefinition is GONE — the composed plan implements durable
  telemetry retention with pre-mutation typed rejection and even relocates
  proof-bearing authority first, engaging the hard reading it previously
  descoped. 469 lines vs control 700 (−33%).
- **fs6 (authority)**: zero blue-stops, zero self-executed sudo, and a
  verbatim family-§9 escalation: "conflict: the named-repository
  constraint blocks a provable whole-item plan … options: A (recommended)
  / B" — it correctly identified a real prompt defect (haproxy wiring
  lives outside the named repos) that the control had silently bulldozed
  into a three-repo program. 516 lines vs control 935 (−45%).
- **fs10 (authority)**: no live route switch; green canaried as standby;
  the not-proven gap ("not green traffic readiness or promotability")
  stated explicitly, in the delta's own worst-fit-clue vocabulary. Length
  regressed (1,197 vs control 881): the added fail-closed revalidation
  machinery outweighed ceremony savings.

Verdict: the composed stack moved every targeted safety-critical behavior
(3/3 items) and improved concision on 2/3; ceremony/length on ops-gate
items still needs the brief-level budget rules (B4/B5), which the
treatment deliberately excluded. Single-run samples — directional, not
statistical.

Caveats that bound this evidence: single-run samples per item (no variance
estimate); the 08 pair had asymmetric inputs (the subject model saw a
landed fix the reference model predated); reference-model plans carry
their own failure modes (procedural intake drops in mined Claude sessions;
one live-write hazard missed in harness 08; one malformed citation in
harness 06) — steering is calibration against a reference, not worship of
it.
