# mattpocock-skills, second review (2026-11-04)

Cycle record for the followed source [mattpocock-skills](../../references.md#mattpocock-skills), made by the method in
[watching.md](../../../harness/skills/agent-policy/references/watching.md). The first take, on 2026-07-10, adapted
three authoring norms into north in `684e72bfc` and kept no cycle record; `agent-machinery/PROVENANCE.md` holds its
credit.

## What we looked at

- `mattpocock/skills`, followed at the whole repository. Fresh blobless clone into an empty scratch directory, read
  as data. Head `4ae2317` (2026-10-31). License at `4ae2317`: MIT, `LICENSE` reads "Copyright (c) 2026 Matt Pocock".
  No file or passage was copied; everything below is a paraphrase of mechanisms.
- Diff from `2ee14df` (2026-07-10) to `4ae2317`: 269 commits, 189 files, +5,321 and -2,135 lines. `SKILL.md` files go
  from 38 to 39: engineering 17 to 20, productivity 5 to 7, in-progress 6 to 8, and the deprecated (4) and personal
  (2) buckets were emptied. 187 commit bodies carry a Claude co-author trailer.
- North at origin/main `f7cae20e2`, its worker ledger, transcripts of the last 7 days and the south policy and
  incident files named below, all read on 2026-11-04.

What the source changed, by mechanism:

- `writing-great-skills` became `writing-for-agents` (`17f22a3`, 2026-07-23): one reference for any document an
  agent reads, not only skills, and its 201-line glossary was deleted. It keeps the three levers north took in July
  (completion criteria, leading words, the information hierarchy) and adds context pointers (a pointer's wording
  decides when the agent reaches its target), two loads (always-loaded context against what the human must
  remember), the environment as a source of truth that a document only caches (`d4e8664`), sediment, and a no-op
  test that is settled by running the document rather than by debate.
- `retro` (added 2026-08-24, promoted 2026-09-24): reads a finished session and proposes changes to the agent's
  environment under seven headings (navigation, automated checks, coding standards, global AGENTS.md, tool economy,
  no-ops, information access). A mechanical violation gets a deterministic check, not a written rule (`0243b6e`),
  and coding standards belong to the reviewer, which works from a diff, not to the implementer, which carries the
  most context.
- `chief-of-staff` (in progress, 2026-10-05 to 2026-10-31): a long-goal lead that works only through fresh
  subagents, keeps a wiki of dated, commit-cited claims ("write history, not the present", `5beb35b`), and runs
  `retro` on about one in five of its subagents' sessions, the failed ones first, acting on the findings itself
  (`5ae4816`).
- `software-factory` (in progress, `4ae2317`): a vocabulary for a system of agents that turns tickets into merged
  code. Every trigger is an accelerator that needs brakes (checks, automated review, human review); triggers start
  throttled; rework loops are capped; a ratchet task takes one file off an exception list per day; a retro task
  reads one merged PR in ten and proposes instruction changes; fix runs never chain.
- `implement-spec` (promoted 2026-09-24): tickets form a task graph with a frontier; one fresh implementer per ticket
  in its own worktree, a merger, one code review of the integration branch, and one fresh implementer fixing all of
  its findings; an exploration subagent leaves notes outside the repository for later implementers.
- `7739eae` (2026-10-29): every skill that dispatches a subagent says to start it with a fresh context window,
  because some harnesses fork the parent conversation by default.
- `code-review`: two axes, Standards and Spec, in two parallel fresh subagents, reported side by side and never
  reranked into one list, so a change cannot pass one axis by masking the other. The Spec axis reports what the
  spec asked for that is missing or partial, what the diff does that nobody asked for, and what looks implemented
  but wrong, quoting the spec line for each.
- `diagnosing-bugs`: redact secrets in shown output (`efce423`); when a red was forced by mutating code or a fixture,
  diff against a pristine copy to prove the mutation landed (`f3fc563`).
- Smaller: `tdd` notes what each proposed seam catches and misses (`3f59913`); `grilling` words questions so a bare
  yes accepts the recommendation (`95249b0`); a new `pr` skill (summary as the smallest view, before and after
  evidence, one-way or two-way door, blast radius); `SCOPE.md` keeps an idea only with an observed failure from a
  real session, and `.out-of-scope/` records each rejected concept with its prior requests (`f0588cd`), including
  "subagent recursion is the harness's job"; em-dashes were removed repo-wide and banned (`3216582`).

Reverts and flip-flops: cross-skill invocation changed four times. `d28dfdc` standardized on a literal "call the
Skill tool"; `fcf0071` reverted a rewording the same day; `1dab982`, also the same day, stopped skills calling
user-invoked skills, because the literal wording made calls that can never succeed fire more reliably; `5ae4816`
(2026-10-31) has a skill read a sibling's `SKILL.md` by relative path instead. That is weak evidence for any one
invocation wording, and north does not chain skills that way. `resolving-merge-conflicts` was promoted on
2026-07-13 (`d327545`) and removed on 2026-09-24 (`daa01d8`). `chief-of-staff`'s retro rule went from "every large
subagent run" to "a one-in-five sample" inside one pull request (#1363), so the rate is a fresh, unmeasured choice.
A docs re-sync was reverted (`da2cb7d`), and the debugging post-mortem hand-off was removed (`1dab982`) and later
rerouted to `retro` (`c5b9869`).

## How we evaluated

1. Each item taken on 2026-07-10 was checked live in north, with file or command evidence.
2. Each mechanism was ranked by expected effect on landing speed, quality and usage per closed box, with a measured
   or a reasoned basis, labeled.
3. The top three each got an experiment with a frozen input, one metric and a keep or revert rule fixed before it
   ran. E1 cost under 30 minutes of worker time and ran in this cycle; E2 and E3 did not and are filed.

## What we took last time, live or not (2026-11-04)

`684e72bfc` (2026-07-10) added "Authoring norms" to the orchestration package's `docs/method.md` and a "BRIEF
DONE-BARS" rule to its `doctrine.md`. Both were cut 22 days later by `8aace10df` (2026-08-02, "make delivery the
default"), and `docs/method.md` itself was deleted in `e74ad5b1f` (2026-10-08). No record says why the norms were
cut, which is a finding: an adopted rule left with no trace of its removal.

- Done-bars: live, and stronger than the prose, in code. `north brief new` prints ``Done when: `<command that proves
  it>` `` (`crates/north-core/src/guard/brief_skeleton.rs:21`); `harness/skills/workers/references/brief.md:8` makes
  the first Done when line an oracle whose observed result the worker attaches; the issue-filing guard reads Done
  when (`crates/north-core/src/guard/issue_filing.rs:19`). The refusal "Done when has no command"
  (`crates/north-core/src/router/brief.rs:157`) sits on the Codex qualification path only.
- Information hierarchy: live as practice, without the name. `harness/skills/agent-policy/SKILL.md` § SKILL.md and
  references keeps `SKILL.md` complete for ordinary work and links each reference with when to read it;
  `north agent-config-check tree-policy` enforces the 6,144-byte and 500-line skill limits; the workers skill became
  a router over references today (`37fd09f46`).
- Leading words, the no-op test and positive phrasing: not live. `rg -i 'leading word|no-op|positive (phrasing|target)|prohibition'`
  over `harness/skills`, `harness/claude` and `agent-machinery/doctrine.md` finds nothing outside the vendored
  guides. The weekly `skill-prune` session prunes toward a 60-line body, which is length, not whether a line changes
  behavior. South `agents/AGENTS.md`, the always-loaded policy, has 25 bullets: 7 lead with a prohibition and 15
  contain one. Enforcers hold one of the 7 outright (force-push, the safe-push argument check) and two in part:
  recursive deletes (the tripwire's fixtures refuse home and system roots) and weakened tests
  (`test_integrity_scan` refuses an added skip or focus).

## Measured basis (2026-11-04)

- Ledger, `north worker-ledger --summary --by category`: 5,998 worker runs, 3,372 Done (56%), about 2,064 kept (61%
  of Done). A Done run is kept only when no other run on its item started within 24 h and its landing was not
  reworked, so about 39% of Done runs were followed by another run on the same item or reworked. That is an upper bound on
  first-pass misses, because a follow-up box also counts. Runs per closed issue, over the 15 Claude first-tier cells
  with at least 10 closed issues, run from 1.4 (bug-known-cause, Opus high) to 9.3 (research, Opus medium); feature
  work, the largest, takes 1.9 on Sonnet (101 closed) and 3.3 on Opus medium (107 closed).
- Incidents: south `agents/incidents.jsonl` has 541 rows and its newest is dated 2026-10-14. `north incident-mine`
  appears in none of the 54 entries of `north agents routines`. Run again on 2026-11-04 with `--days 7 --chunks 6
  --per-thread 3`, it mined 1,254 candidates in about 20 s (623 flagged as Tom pushback, 456 Not done, 111 reverts,
  33 blocked, 23 admission, 6 relayed corrections, 2 fix chains), and nothing classifies them.
- Context, `north context audit --since 7d`: 1,149 sessions; startup policy 2,975 tokens a session; total injected
  context p50 3,636 tokens for a lead and 3,049 for a worker.
- Reviews: the ledger's 77 review lenses are defect hunts (drift against an original, credential scope, attacker of
  the boundary, landing review); none checks a diff against what its brief asked for. North staffs a reviewer only
  for planner designs, trust-boundary code and `Spec: judged` cross-module lanes
  (`harness/skills/workers/references/review.md`).
- Subagents, `subagent_type` values in transcripts changed in the last 7 days: 12 forks out of 2,393 (0.5%); fleet
  sessions start fresh by construction.
- Exception lists: north tracked 66 Python and 89 shell files on 2026-10-20 and 25 and 54 on 2026-11-04; the
  language allowlist went from 11 entries to 6, and `crates/north-landing/src/language_gate.rs` expires a Python entry 7
  days after it was created or once its Rust port lands, and refuses any widening without Tom's approval.

## Decision table

| Rank | Mechanism (source) | Basis | North today | Decision |
| --- | --- | --- | --- | --- |
| 1 | A retro on a clock over a sample of finished sessions, failures first, with mechanical findings turned into checks (`retro`, `chief-of-staff` `5ae4816`, `software-factory` retro task) | Measured: incident ledger frozen since 2026-10-14; 1,254 unclassified candidates in 7 days; 39% of Done runs not kept | Miner, classifier recipe (south `agents/incident-baseline.toml`), rules register and enforcer ladder exist; the loop has run once, by hand | Kept by E1: 8 recurring modes with no enforcer |
| 2 | A Spec axis kept apart from Standards: missing, unasked-for and wrong-looking work against the brief, each quoting its line (`code-review`) | Measured for the gap (39% of Done runs not kept, 77 lenses with no brief check); reasoned for how much a spec check would catch | Done when oracles check named outcomes; no check of the diff against the rest of the brief | Test with E2 |
| 3 | The no-op test settled by running the document, and positive targets in place of prohibitions (`writing-for-agents` § Pruning and Leading words) | Measured: taken in July, cut on 2026-08-02, not live; 7 of 25 startup bullets lead with a prohibition. Reasoned for the effect | `agents sync` gates additions by replay; deletions only by length (`skill-prune`) | Test with E3 |
| - | Fresh context for every subagent, never a fork (`7739eae`) | Measured: 0.5% of subagent launches are forks | Fleet sessions are fresh | Leave out |
| - | A ratchet that takes one exception off a list a day (`software-factory`) | Measured: Python files fell from 66 to 25 in 15 days without one | Code-enforced expiry and no-widening in `language_gate.rs` | Leave out (live, stronger) |
| - | Coding standards on the reviewer, mechanical rules as checks (`retro`) | Measured: `enforcers.md` lists 34 edit-time hook rows and 16 safe-push gates | `agent-policy` fixes a correction at the highest level and deletes the prose once an enforcer holds it | Leave out (convergent) |
| - | Prove a forced mutation landed (`diagnosing-bugs` `f3fc563`) | Reasoned: small | `north mutant record` makes mutants on scratch copies and `mutant verify` reruns each kill claim | Leave out (convergent) |
| - | Dated, commit-cited claims; an observed-failure bar for new rules; rejected ideas kept with their reasons (`chief-of-staff`, `SCOPE.md`) | Reasoned | Watch and rationale records date and cite; `agent-policy` takes a demonstrated lesson and replays a real past mistake | Leave out (convergent) |

## Experiments

### E1, a sampled retro (ran 2026-11-04)

Fixed before it ran. Frozen input: the 1,254 candidates `north incident-mine --days 7 --chunks 6 --per-thread 3`
mined at 11:31 AM on 2026-11-04, sorted by timestamp and session, every fifth row from the first: 251 rows
(sha256 prefix `f804b24d6df42af7`), the source's one-in-five rate. Classifier: one Haiku worker given the 2026-10-12
baseline's keep rule (a failure, a correction or a pushback; routine status, briefs and normal handoffs dropped),
`enforcers.md` and south `agents/AGENTS.md`, grouping kept rows into failure modes. Metric: failure modes seen at
least twice in the sample that no enforcer row would refuse or flag. Keep rule: 3 or more such modes, and a sampled
retro goes on a weekly timer with each mode filed as an enforcer candidate; fewer, and the miner stays manual and
this record keeps the count.

Result. The classifier kept 104 of the 251 rows (49 pushbacks, 46 failures, 9 corrections) and grouped them into 55
modes, 11 of them seen at least twice; it took about 148k Haiku tokens and 5 minutes. The lead checked the rows behind
the two largest modes and set three aside: 11 rows of host-saturation alerts are machine alerts the miner flagged as
Tom's pushback, so a detector already exists; a red main after landing (5) is held by the pre-push checks; and a
landing refused for a missing trailer or mutant record (3) is an enforcer working. The other 8 recurring modes have
no enforcer:

- 16: shipped a visible result that contradicted the asked-for behavior or its reference, almost all game behavior
  seen in play (prose only: the startup rule to build, launch and exercise the product before deeper checks).
- 5: gave an explanation the reader could not follow (prose only).
- 5: kept retrying a landing that GitHub or Autoland had refused (none).
- 4: ran a bulk script or formatter that changed code outside the task (none).
- 4: let a stall go unnoticed for hours (prose only).
- 3: asked to confirm an instruction already given (prose only).
- 2: chased a wrong cause for a symptom (prose only).
- 2: left a requested fix unlanded while reporting progress (prose only).

8 is above the threshold of 3, so E1 is kept. The largest mode is the Spec-axis gap of rank 2, and 15% of the kept
rows. Most of it is behavior visible only in play, which a reviewer reading a diff against its brief cannot see, so
E2 measures only the diff-visible part. The classified rows and modes, with transcript refs and no message text, are
in south `agents/decisions/harness/watch-2026-11-04-mattpocock-e1/`.

### E2, a Spec axis before landing (filed)

Frozen input: north Claude worker runs that finished Done between 2026-10-28 and 2026-11-03, 20 not kept (another
run on the item within 24 h) and 20 kept, each with its brief and its landed diff. One fresh Sonnet reviewer per
run, blind to the outcome, answers the source's three Spec questions against the brief. Metric: the share of the 20
not-kept runs where the reviewer names the gap that the next run on the item then fixed. Keep rule: add a Spec-axis
reviewer before landing `Spec: judged` lanes if that share is at least 30% and the reviewer flags at most 2 of the 20
kept runs; otherwise record the numbers and change nothing.

### E3, the no-op test by replay (filed)

Frozen input: south `agents/AGENTS.md` and the policy-replay scenarios the `agents sync` gate scores, both at their
2026-11-04 heads. Two arms against the incumbent: arm A deletes the force-push bullet and the parts of
the recursive-delete and weakened-test bullets that an enforcer already refuses; arm B rewrites the 7 prohibition-first bullets to lead with their positive target, keeping each
guardrail. Metric: replay conformance as the gate scores it. Keep rule: land each arm whose conformance does not
drop beyond the gate's own noise floor and whose startup tokens do not grow; revert the rest.

E2 and E3 are on north#525 with their Done-when boxes and owner. Neither costs under 30 minutes: E2 is 40
reviews plus gathering briefs and diffs, and E3 is two replay sweeps.

## Decided

- Ideas only; no file from `mattpocock/skills` copied.
- The July take stands as live for done-bars (enforced in code) and the information hierarchy (practice and skill
  limits). Leading words, the no-op test and positive phrasing are not restored as prose; E3 tests the two that can
  be measured, and the prose returns only if E3 keeps an arm.
- Keep the sampled retro (E1): a weekly timer mines the last 7 days, classifies every fifth candidate against
  `enforcers.md` and the startup policy, and files each mode seen at least twice with no enforcer into the rules
  register as an enforcer candidate. The timer and a decision on each of the 8 modes above are boxes on north#525.
- Test a Spec axis (E2) before any change to § Review.
- Leave out fresh-context, ratchet, mechanical-rules-as-checks, forced-mutation proof and dated-claim rules: each is
  live in north already or measured as not a problem.
- Next review of this source: from `4ae2317`, when it next comes due under watching.md.
