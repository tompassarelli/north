# The method — why gaffer works this way

## Routing: shape, not difficulty

Initial role triage follows the deliverable SHAPE — execute / implement /
integrate / design / direct / scout / analyze / review / verify / judge /
research-science — because shape is a better prior than difficulty-as-felt.
Role does not choose the other axes: task grade, domain requirements,
topology, semantic tier, deliberation, and posture remain explicit. A
hard-but-local testable bug is still *implement*; a one-line naming decision
that shapes an API is *design*. Blast radius may route capability up;
importance alone never does.

Two empirical laws sharpen the routing:

- **Minimum-sufficient floor.** Repository or stack layer is not a capability
  proxy. Foundational implementation-only work may remain `economy` or
  `standard` when it owns no invariant decision and has a bounded seam plus an
  objective oracle. Foundational invariant-decision ownership routes at least
  `senior`; system-shaping/open-solution ownership or system synthesis routes
  `frontier`. The selection assessment makes that boundary explicit instead of
  promoting every foundational edit.
- **Shingle law.** A provider catalog exposes only useful
  model×deliberation rungs and assigns each exact rung to only one semantic
  tier. When an upper rung is dominated by the next route's lower rung, the
  catalog omits the overlap; a provider's strongest model may span adjacent
  tiers only through disjoint deliberation levels. Routing therefore follows
  one continuous semantic ramp: harder work climbs the route rather than
  cranking deliberation against a low ceiling. Concrete, current examples are
  generated from the catalogs in
  [`docs/provider-matrix.md`](provider-matrix.md); they are evidence for the
  mapping, not shared doctrine.

## Minimum-sufficient selection assessment

Role still describes the deliverable, while a versioned sidecar derives the
minimum route from seven independent signals:

| Signal | Question |
|---|---|
| `decisionOwnership` | Is the worker executing, making a bounded choice, owning a cross-boundary invariant, shaping a system, or exploring an open solution class? |
| `seamScope` | Are there no seams, established seams, consequential seams, or system-wide seams? |
| `errorExposure` | Is a plausible error contained/reversible, material/recoverable, or high/hard to reverse? |
| `oracleStrength` | Is the outcome objectively checkable locally/end-to-end, only partially observed, or judgment-only? |
| `foundationalImpact` | Is foundational code untouched, implementation-only, or does the lane own an invariant decision? |
| `dependencyShape` | Is work atomic, a fixed workflow, parallel breadth, dynamic decomposition, or tightly coupled sequential work? |
| `reasoningShape` | Is reasoning deterministic, bounded branching, multi-hypothesis, system synthesis, or exceptional? |

`scripts/selection-assessment.mjs` recomputes `minimumTier`,
`minimumReasoning`, and stable `ruleCodes` from those signals. A selected route
below either minimum is invalid. A route above either minimum needs one of four
explicit exception codes plus detail: `explicit-human-floor`,
`recent-lower-tier-failure`, `calibration-experiment`, or `unmodeled-risk`.
`max` is valid only for `reasoningShape=exceptional` with a concrete nonempty
`exceptionalDeliberation`; that field is rejected below max so stale ceremony
cannot accumulate. Free-text stock-template `overrideReason` remains distinct
human provenance. The assessment is a sidecar, never a ninth request field.

## Review is an artifact-evaluation shape

Review takes one supplied artifact or change and evaluates it across multiple
criteria. Its output is evidence-backed, prioritized findings plus an
`accept`, `changes-required`, or `cannot-assess` disposition. That makes it
different from analysis (understanding a mechanism), verification (deciding
one claim), judging (ranking multiple alternatives), design (choosing or
redesigning a shape), and integration (applying a change).

The generalized contract is grounded in primary code-review practice:
Google's reviewer guidance treats review as a whole-change, multi-criterion
assessment across design, functionality, complexity, tests, documentation,
and context
([what to look for](https://google.github.io/eng-practices/review/reviewer/looking-for.html));
its standard prioritizes evidence and system health rather than taste or
perfection
([review standard](https://google.github.io/eng-practices/review/reviewer/standard.html));
and GitHub gives review an explicit approve/request-changes/comment disposition
([pull-request reviews](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/about-pull-request-reviews)).
Gaffer generalizes beyond code to any one supplied artifact while preserving
read-only authority.

## Payloads: consumer-calibrated deltas

Generic "best practices" prompts waste budget restating what a model
already does and dilute the items it actually needs. A gaffer delta is
built by **elicit → subtract → compile** (see the elicit skill):

1. Elicit the model's own introspective self-report, contamination-guarded.
2. Subtract everything it already self-reports — that needs no teaching.
3. Compile what remains in the model's own vocabulary: named limits become
   procedure, named tells become triggers, stale self-models get corrected,
   know-but-skip habits get written-checkpoint enforcers.

The self-reports that produced the shipped deltas are in
`docs/self-reports/` — they double as worked examples of step 1.

## What this does and doesn't transfer

Procedure transfers **mode-switches**: verification discipline (run, don't
predict), escalation tripwires (first failed fix → report, don't thrash),
reporting protocol (observed/inferred/assumed provenance). Procedure does
NOT transfer **capacity**: holding many constraints in parallel, noticing
the anomaly nobody flagged, distant-analogy reframes. That residue is why
the ramp exists — when a task needs capacity, route up the model tier; the
delta makes whichever tier you land on behave at its best.

## Authoring norms — briefs, payloads, skills

The discipline that makes a good spawn payload makes a good brief and a good
skill. Three levers, adapted from Matt Pocock's *writing great skills*
(github.com/mattpocock/skills, MIT).

**Done-bars.** Every step of a brief or skill ends on a *completion
criterion* — a checkable bar the worker judges itself against, never a vague
"understanding reached". State it as a command plus its expected output, or a
grep plus the hit count it must return. Two properties earn it: it is
*checkable* (the worker tells done from not-done) and, where it matters,
*exhaustive* ("every modified file accounted for", not "produce a change
list"). A bar defends against **premature completion** — attention slipping
to *being done* before the work is — so name that failure when the bar is
non-obvious: what a rushed worker skips, and what count catches it. A bare
"done" is never accepted: a self-contained worker supplies evidence against
its bars, while an emergent aggregate receives an independently staffed,
context-carrying verifier report with a verdict, probe, and observed result.
The coordinator still owns the final judgment: it drives the assembled result
end-to-end and records bounded independent non-authoring verification probes at
the load-bearing child seams. Such a probe may create disposable
test/build/cache state needed for observation. That is integration due
diligence, not a rerun of each worker's full local suite and never permission to
edit, implement, or repair a worker's piece.

**Leading words.** One strong pretrained concept anchors a whole region of
behaviour in the fewest tokens, by recruiting priors the model already holds
(*tight loop*, *red-capable*, *driven end-to-end*, *minimum-sufficient floor*). It works
twice: in the body it anchors execution — the worker reaches for the same
behaviour every time the word appears — and in the invocation it anchors
firing, the same word in prompts, docs, and code linking them to the intent.
Hunt restatements and collapse them into the word: a triad spelled out at
three sites is one token begging to be minted. The no-op test grades each
line — does it change behaviour versus the model's default? A line that does
not (*be thorough* when the model is already thorough-ish) dies or earns a
stronger word (*relentless*). Prefer positive phrasing: a prohibition names
the elephant into the frame, so state the target behaviour and the banned one
is never spoken.

**Information hierarchy.** Payload sits on three tiers, ranked by how
immediately every reader needs it: the inline step (in the brief or SKILL.md
body — what every path executes), the in-file reference (a rule or table
consulted on demand), and the linked file behind a named pointer (loaded only
when the pointer fires). **Progressive disclosure** is the move down the
ladder — push reference out of the top so it stays legible. Branching is the
test: inline what every path needs, disclose what only some paths reach. The
pointer's *wording*, not its target, decides whether the worker reaches the
material — a must-have behind a weak pointer is a variance bug; sharpen the
wording before inlining.
