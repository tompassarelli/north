# Semantic physics review: how much of north's state comes from git and checks, and how much from what agents say

An outside design proposal, reviewed on 2026-11-07, argues that a coding harness should keep its own model of the work and treat each agent as a stateless worker that only proposes a change. This note answers the proposal's closing question with measurements of north and rates its five primitives and three invariants. Its recommendations are filed as boxes on the issues that own them.

## The proposal

The proposal treats the harness as a deterministic solver over a work graph, with the model owning none of the world state. It names five primitives:

1. **Explicit state topology.** The graph owns every state machine; an agent proposes a delta.
2. **Conservation laws.** Interface contracts, budgets and gates are hard constraints, so a delta that breaks one is refused before it propagates.
3. **Causal rollback.** A dependency graph computes the blast radius of a broken assumption, invalidates only the dependent work and replays the downstream checks.
4. **Least-action routing.** Route by the verification surface and context footprint of the exact subgraph, not by historical averages.
5. **A ratchet.** Every recurring failure is attributed to the harness or the worker and compiled into a permanent guard.

It names three invariants:

- Agents never write the truth: graph facts come from git, verification exit codes and lifecycle hooks.
- Harness faults never feed model or brief routing scores.
- The graph holds the multi-step plan, and a worker moves one node across one gate.

A plainer restatement adds "one step per session" and "every failure becomes a permanent guardrail". The question it closes on: how much of north's graph state is derived mechanically, and how much relies on the agent reporting facts back?

## The answer, measured

The inventory below was taken on 2026-11-07 against f7b160b842. It covers the threads runs ledger and graph.db, and is posted with every file:line on north#791 (box 2).

- **mech:** git, lander verdicts and queue, exit codes, admission and pace samples, placement decisions, fleet lifecycle.
- **gh:** a structured GitHub field. A machine holds it, but an agent sets it.
- **text:** agent prose: report openers, `Stop:` lines, box and issue prose, docs.

What each reader rests on:

- **Scoreboard:** 5 of the 11 readiness bars are mech: landing p90, verification p50 and p90, live p90 and idle capacity (src/cli/scoreboard/ready.rs:305-345). Priority-now is gh. Rework and ask-to-landing join mechanical records to text. First try and harness loss are decided by text (src/cli/scoreboard/workers.rs:62-66). Corrections are judged by a model.
- **Attribution:** 19 of the 181 worker runs that finished between 2026-11-06 23:00 and 2026-11-07 06:14 UTC (10.5%) took their cause from a boundary record. 88 (48.6%) were Done because their report opened with `Done:`, and 74 (40.9%) were decided by the phrase rules or the worker default (crates/north-threads/src/threads/attribution.rs:60-87). Of the 93 runs that did not finish Done, 80% were read from the report: 22 from `Stop: wait`, 16 from `Stop: host`, 10 from `Stop: capacity` or `client`, 3 from `Stop: landing`, and 20 with no `Stop:` line at all.
- **Routing:** 3 of the 18 inputs dispatch and placement read are mech: the climb after two lander refusals, the host refusals at the placement gate, and placement refusals in the decision log. Every hold that keeps a box from being staffed is text or gh:
  - box waits and `Follows:` lines (crates/north-graph-ingest/src/waits.rs:23, 147)
  - the backlog DAG (crates/north-graph-ingest/src/ranking.rs:59-71)
  - dated checks in box prose (crates/north-graph-ground/src/due.rs:9-11)
  - holds after `Stop: decision`, `Stop: wait until` and `Stop: host` (crates/north-dispatch/src/dispatch/stops.rs:17-19)
  - `Category:` for the starting tier (crates/north-dispatch/src/dispatch/tier.rs:1-24)
  - the measured worth, which comes from attribution (crates/north-graph-ingest/src/losses.rs:35)
- **Fact graph:** 344,329 of its 353,929 live facts (97.3%) are mech. They come from the decision log, the coverage map, the push log, git `touched`, checker exit codes and the ledger's landed shas. The ready set reads none of them. Its rules (crates/north-graph-ground/src/rules.rs:31-60) read only issue state, boxes, ticks, box text, titles, rank, waits, labels and the two worth figures: 8,866 facts, all text, gh or derived from text. The mechanical 97% feeds the grounding lines of prompts and briefs, where a ticked box's claim reads "observed" only when the checker saw its commit pass (crates/north-graph-ground/src/issue.rs:46-56).

So most of what north records is mechanical, but most of what north decides from is text. The gap is in the readers, not in the store.

One case shows that text is not always the wrong side. From 23:00 to 06:14 UTC, 24 of the 76 runs that reported Done and expected a landing show no landing on their run row. For 22 of them, the push log names a landing by the same fleet session. The workers were right, and the run row's landing join missed the landing (crates/north-providers/src/providers/claude/runs/mod.rs:277-280). A mechanical source counts only once the reader actually joins it.

## The primitives and invariants, rated against north

Each rating names the 2026-11-07 failures the missing part would have prevented.

1. **Explicit state topology: partial.**
   - Present: lifecycle records are typed (docs/rationale/state-machines.md inventories 36 machines still held in markers and flags, north#757), lander tickets carry verdicts, and registered waits live in `fleet_waits`.
   - Absent: the work state itself (box ticked, box waits, box needs, done claims) is issue prose that agents edit, and the graph mirrors it instead of owning it.
   - Would have prevented: waits read from `Stop:` text. wait-stops.jsonl is learned from report lines (c949079a55), while the registered waits sat in `fleet_waits` (f7b160b842). Also an account move read as a death: the move was recorded as a relaunch under another model or effort, and a watchdog judged the stop from a status flag, until 59d1a38b22 made it a typed move.
2. **Conservation laws: partial.**
   - Present: land checks run before landing (clippy, nextest, the gate-shadow scan and, since 159b37e59b, tree-policy). Enqueue refuses a lane that already conflicts with main (f1cf56ede0). Test admission requires a bug test to fail first. A new gate runs in shadow before it may refuse.
   - Absent: from 2026-11-06 17:00 to 2026-11-07 06:20 UTC, 24 of the 33 red-main after-landing runs failed a check that builds nothing: tree-policy 14, code-health layers 5, the indexes 4, experiments 1. Those checks ran only after landing, and while `landing_refusals` is in shadow even the `@enqueue` lines do not run before landing (docs/rationale/landing-refusals-shadow.md).
   - Would have prevented: the policy-size check that ran only after landing.
3. **Causal rollback: partial.**
   - Present: the coverage map links tests to the functions they reach (114,822 facts). Affected-test selection, the import-checked tests/it table (1274d536ee), the culprit search over landed lanes and the red-main revert all exist.
   - Absent: no edge links a landing to the queued lanes it invalidates. Of 78 conflict send-backs in the week, 30 conflicted with a lane that landed while they waited, and 2026-11-07 landed four crate splits that moved whole module trees. Coverage selection ran once in 19 landings (north#634).
   - Would have prevented: a lane that broke a test still passing its candidate check (8c5d22002), and refactors breaking queued lanes.
4. **Least-action routing: partial.**
   - Present: placement reads needs, capacity and refusals.
   - Absent: an issue's footprint (title, boxes, files its landings touched) is computed (crates/north-graph-ground/src/footprint.rs), but only the nearest-box replay reads it (crates/north-dispatch/src/dispatch/nearest/replay.rs:74). Tiers start from the `Category:` line and settle by sampled arms on cost per kept box, the historical averages the proposal warns about. Those arms are also fed the 22 Done runs mis-joined as unlanded.
   - Would have prevented: host needs guessed from prose. At 12:58 PM, 19 of 27 ready items were held for hosts on the strength of their prose while 11 slots were free (north#787).
5. **Ratchet: partial.**
   - Present: the flow detectors file one box per recurring class (crates/north-flow/src/detectors/mod.rs:1-3), and 42 such lines sit in open issues. Each kind of guard has its own proof: a bug test must fail first, a gate needs its shadow review, and harness text needs `north policy-replay`, which reads kept or regressed only at the 90% level (north#714).
   - Absent: a worker still writes each guard by hand. Only skill prose asks a harness-text fix for its replay case, and harness/cases/ holds none yet.
   - Would have prevented: the same skill-size red twice in one day. 6df2cdbe7a broke it in the morning and was fixed by trimming the skill. 98d30d226 broke it again in the afternoon, and only then did the check move before landing (159b37e59b).

The invariants:

- **Agents never write the truth: absent wherever a decision is made.** The store is 97% mechanical, but attribution decides 89.5% of its verdicts from text, and every staffing hold reads text. north#791 now owns this rule.
- **Harness faults never feed routing scores: partial.** Tier learning treats harness-caused outcomes as unjudged (docs/rationale/attribution.md). But 80% of failures are classified from report text, and a failure no rule claims defaults to the worker (attribution.rs:86), so a harness fault the report does not name is scored against the model. The account move recorded as a model or effort relaunch is the same leak in a lifecycle record.
- **The graph holds the multi-step plan: partial.** Dispatch launches one item per session and holds boxes one by one, and a hand launch can name its box (9f5e349605). The plan itself lives in prose, though. The graph holds 29 wait edges, read by regex from box text and backlog-ranking.md, and 12 more that the ingest cannot parse. north-722-d0300 was staffed on boxes that all waited on crate splits (62025a79cc).

## The failure-to-guard path today, and how it becomes automatic

Today a failure becomes a guard by a chain of hands:
1. A detector or the attribution line in `north ledger flow` surfaces the class.
2. The proxy or a detector files a box.
3. Dispatch staffs it.
4. A worker writes a test, a gate or harness text.

Detection and filing are already automatic for six detector kinds. Writing the guard is not, and must not be: a rule written without proof can refuse correct work. Today's gate refusals show that cost.

The step that makes the ratchet safe already exists, one proof per kind of guard:
- a code guard must fail on the base and pass on the lane (fails-first test admission);
- a refusing gate must earn its shadow review of 24 h or 20 decisions;
- harness text must show its replay case regressed on the base text and kept on the lane's.

What is missing is binding the detector to the proof. The detector's box should carry its failing sessions as replay cases, and the lane that ticks the box should land only with its proof. A land scan then refuses a lane that ticks a detector box without its proof, and an inconclusive replay keeps the box open. That loop runs without a person, and still admits no unproven rule. Its inputs get better as north#791 moves attribution onto mechanical causes, because a detector keyed on mechanical causes files fewer wrong boxes.

## One step per session

The data do not support making it a rule. The 29 first tries the harness lost in the 3 h window on 2026-11-07 broke down as 11 waits, 10 host, 4 capacity, 3 landing and 1 unclear (north#790). Not one came from a worker that planned too many steps. Workers already hand back a multi-step box as a `Split:` (36 runs over 24 h). The lever these failures point to is automatic resume after a harness stop (north#790), not a shorter horizon.

## Recommendations, ranked by harness usage saved per box

1. **No build-free check runs only after landing.** On north#729: 24 of 33 red-main runs in 13 hours came from such checks, and the fix is a classification of check lines plus one land scan. A contract line runs before landing in every mode. A debt line files its section and never marks main red.
2. **A run's landing comes from the push log for its fleet session.** On north#791: 22 of 76 Done runs were mis-scored as unlanded. This is a join, and north#791's done-from-landed-sha box depends on it.
3. **Box prose never holds a box for a host.** On north#787: a wrong placement costs one short `Stop: host` run that records the need (0.2% of 24 h worker usage). A prose hold idles a slot for as long as the box waits.
4. **A landing that moves files re-merges the queued lanes it touches at once.** On north#360, which owns the landing-refusal class: 30 of 78 conflict send-backs in a week came from lanes landed while the lane waited.
5. **A detector-filed box closes only with a proven guard.** On north#706: this makes the ratchet automatic behind its existing proofs. It ranks last because it compounds over weeks, and it needs mechanical attribution (north#791) to aim well.

## What north will not do

- **No separate solver or world model.** North already has a fact store with a Datalog reader (north-graph) that holds the mechanical facts. The fix is to point its readers at them, and a second engine would duplicate the store.
- **No ban on agents writing graph facts.** Box and issue text is how intent reaches the work. The rule north adopts is that text never decides a verdict, and a claim stays labelled as a claim until a check observes it.
- **No strict one step per session.** None of the measured losses came from horizon, and a `Split:` already hands back what a worker cannot finish.
- **No footprint routing in place of tier learning yet.** The arms are fed mis-joined outcomes, and harness stops cost more than tier misfit. Fix the inputs first, then measure whether footprint features beat the category.
- **No second 60 s gate.** north#634 owns making the existing checks that fast.
- **No rules written without proof.** The ratchet compiles a guard only through the proof that its kind already has.

## Method

Attribution figures come from the threads runs ledger. They cover worker runs (`ROLE_SQL`) that finished between 2026-11-06 23:00 and 2026-11-07 06:14 UTC, joined to `boundary_causes` the way `recorded_column` joins them. Every run in that window carries a fleet name; a trailing 24 h window would include runs ingested before fleet names were kept and would understate the mechanical share. Red-main figures come from post-land.tsv rows for north, grouped by the failing command, with distinct run timestamps counted as runs. Graph figures are live facts in graph.db grouped by writer and predicate. Landings by session come from the push log's `landed-by` facts.
