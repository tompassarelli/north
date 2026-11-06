# The policy climber: north improves its own staffing policy

Decided 2026-11-06 under north#636 after Tom asked for a self-improving harness. `north loops check` lists closed loops that tune
a constant (the tier cells, ETA, budget, cores, deadline), and nothing changes the policy's structure from outcomes, so every
structural fix waited for the proxy. The climber is the outer loop: it finds the largest measured loss in the worker ledger,
has a generator write one candidate change to routing, brief generation or dispatch rules, scores the candidate on recorded
decisions, runs the survivor as a bounded live arm, and keeps it or reverts it by cost per kept box. No person is in it.

## The loop, one turn

1. **Loss.** From the ledger's closed report (`north-providers` `ledger::closed`, the code `north tune` and dispatch already
   read), each (tier, category) cell's loss is the dollars its closed boxes spent on runs after the first, plus the dollars of
   its boxes that never closed. The largest cell of a routable category is the turn's target. Counting stays where it is: the
   climber reads the report and adds none.
2. **Generate.** A generator writes one candidate change from the loss, the archive and the failed runs of that cell. `Cheapest` is the generator built so far; a `Generator` trait arrives with the second one, because a trait with one implementation and no test double is a rust-habits finding. `Cheapest` routes the category to the tier that closed at least
   five boxes cheaper with no lower kept share, else off its dearest arm onto the category's cheapest arm, else opens a `split` or `escalate` arm on it. A Claude session (a later box) gets the loss, the archive's variants with
   their scores and the failed runs' reports, and writes a code or config diff; its prompt and the way it learns from its own
   failures follow GEPA (reflect on trajectories, keep a Pareto set of candidates that win somewhere).
3. **Refuse.** `scope::refusals` rejects a candidate whose diff touches the evaluator (`north-climb`, `north-replay`,
   `north-strategy-types`), the rule that picks an arm's winner (`router::settle` and `north tune`'s cells; `ARM_RUNS`, in the router's
   placement policy, is refused by name below), the ledger's counting and recording code (`north-providers` ledger, `north-threads`,
   `worker_ledger`, `north ledger`, `north-goals`, the scoreboard), a scoring bar (`goals.toml`, `loops.toml`,
   `corrections.toml`, `experiments.toml`, `structure.toml`), or any test or fixture. `harness/defaults.toml` is judged key by
   key: `scope::key_refusals` refuses a change to any key whose `consumers` include one of those paths (every replay bar and
   the rework judge's thresholds), and an inventory test fails when guarded code reads a key the guard would let through, so a
   new consumer cannot open a hole by forgetting its `consumers` entry. `dispatch/tier.rs` holds the arm table a candidate
   edits beside the code that routes an arm (`tiered`, `efforted`, `other_arm`) and the trial length `ARM_WINDOW`, so a path
   check cannot guard it: `scope::diff_refusals` also refuses a diff whose changed lines name those functions, `settle`,
   `ARM_RUNS`, `ARM_WINDOW` or a `replay_*` bar, and the generator box runs it on every generated diff. The refusal is made
   before the candidate is scored, so a generator cannot buy a better score by moving the thing that scores it.
4. **Replay.** The candidate is applied to every logged placement case it reaches and the window is decided again with
   `north strategy replay`'s machinery (`north_replay::replay` with a patched placement decider), beside a replay of the current
   policy. The score is what the candidate adds: outcome shifts past `replay_outcome_shift_ratio` and breached invariants
   (quota floor, offload-slot time, landing p90) that the current policy does not already show. A candidate that adds any is
   dropped and archived as refused.
5. **Live arm.** A candidate that passes takes a bounded share of the category's boxes (starting 10 to 20 percent, within the
   adaptive shares below) and its boxes carry `Arm: climb-ID` in the brief, so the ledger counts them with no new code.
6. **Select.** Once the arm and its parent hold the sample the arm rule needs, `reward::winner` picks the arm cheaper per
   checker-verified kept box that is no dearer per kept box; the winner becomes the parent and the loser's change is reverted
   by landing its inverse diff. A kept box is checker-verified when every ticked box of its issue carries the `Evidence:` stamp
   `north task check --done-when` writes for an exit of 0, and self-ticked otherwise; a brief-generation candidate rewrites
   the briefs of the workers who tick boxes, so a gain made only of self-ticked boxes never wins. Each reward records the
   rework judge (`JUDGE_VERSION`) it was taken under, and `reward::comparable` judges the older of two again before they are
   compared.
7. **Archive.** Every candidate, kept or not, is recorded (below). Losers stay: a failed variant can parent a later success.

## What replay can and cannot score

Strategy replay decides placement, admission and lander cases from the log. A candidate on placement or its config
(host order, resident caps, which account takes a launch) changes those outcomes, and replay shows exactly how. A tier candidate
such as `TierRoute` (the built kind) moves a launch's tier, which placement does not read today, so replay reaches the cases that
name its category, finds zero shifts, and the score is the guard it should be: the change breaks no placement, admission or
landing decision. The candidate's expected gain for a tier change comes from the ledger (cost per closed box and kept share of the
target cell against the current cell, as `projected saving`), and the live arm settles it. The dry run prints both numbers
and never presents the replay result as the gain.

## Guards against reward hacking

Darwin Gödel Machine runs faked test logs and removed the markers that detected it
([sakana.ai/dgm](https://sakana.ai/dgm/)). North keeps the same three things out of reach:

- The **evaluator** (`north-climb`, `north-replay`) and the **ledger's counting and recording code** are outside every candidate's
  scope (step 3).
- The **reward** is a box closed by the Done-when check and kept after 48 hours, not a worker's report: selection counts
  only checker-verified kept boxes, and the dry run prints each arm's kept boxes split into checker-verified and self-ticked.
- The **generator never grades itself**: the live arm's verdict comes from `ledger::closed`, which a candidate cannot edit, and a
  candidate that changes a test is refused, so passing cannot be bought by weakening the test.

A landed candidate still goes through `safe-push` and the full land checks like any change.

## Arm shares are adaptive (Tom, 2026-11-06)

Every arm dispatch runs (tier arms, effort arms, handoff arms, and the climber's own) starts at an equal share, 50-50 for two
arms. After each closed box the loop reallocates by Thompson sampling on cost per kept box: it draws from each arm's posterior of
cost per kept box and gives the next box to the arm with the lower draw, with a floor on every arm's share until the arm settles
(the settle rule below). A settled arm takes the whole category. `north climb run --dry-run` prints every live arm's
category, its experiment (tier, effort, handoff, context or climb arms, each splitting the category's boxes on its own), share and how it was set (Thompson over 1,000 seeded draws with a 10% floor, or settled), boxes, closed, kept and
dollars per kept box. A posterior draw resamples the arm's boxes with replacement, and arms tied cheapest split the draw.
Dispatch itself still alternates armed categories; routing its picks by these shares is the next step.

## An arm settles on evidence (2026-11-06)

`router::settle::settled` gives a comparison to the arm cheaper per kept box only when both arms hold `ARM_RUNS` runs and
closed boxes, its kept share is no lower, and a seeded bootstrap over both arms' boxes puts it ahead in 90 of 100 resamples.
Closed-box cost is heavy-tailed (the 177 closed armed boxes of the 30 days to 2026-11-06 run from $0.77 to $78.60 around a
$4.14 median), so at 15 boxes a point difference settles a 1.5 times dearer arm in more than one draw in twenty, the winner's
curse the 2026-10-12 policy-eval review found. The test draws 1,000 trials from those committed costs and holds the bootstrap
at or under 5%.

## Experiments are opened and decided by the loop (Tom, 2026-11-06)

- **Open.** A passing candidate registers itself in `docs/rationale/experiments.toml` (id `climb-ID`, the change, the owner
  north#636, the metric, the bar and the review day) in the commit that lands its arm, so no experiment exists only in a worker's
  head.
- **Decide.** `experiments.toml` sends a verdict between the bars to Tom. For a climber experiment the loop decides: a verdict
  inside the bars with fewer than `min_n` boxes extends the window by half its length for more n; a verdict inside the bars at
  `min_n` keeps the cheaper arm and records why. The extension and the settle are both written to the experiment's record, with no
  gate. `north experiments judge` does this for every entry whose id starts `climb-`: a reading under `min_n` extends the
  after window and the review by half the window (recorded in the filed-verdict state, so the next run judges the new
  review), and a verdict between the bars at `min_n` settles on keep or on a revert box. A missing reading still
  re-measures. `experiments::verdict_tests` pins both with no Tom line.

## The archive in north-graph

Each candidate is a node written by `north graph tell` under writer `climb`: `climb:ID :parent climb:PARENT` (the root is
`climb:current`), `:change` (the typed change as a string), `:loss-category`, `:loss-usd`, `:replay-refusals` (a count),
`:replay-reached` (cases), `:status` (proposed, refused, arm, kept, reverted), `:arm-share`, and at selection `:usd-per-kept`
and `:first-pass`. The generator reads the archive back with `north graph ask`, and parents are drawn across it, not only from the
current best, as in DGM. Retraction never deletes: a reverted change keeps its node with status reverted.

## Registration and cadence

`harness/loops.toml` registers `policy-climber` as an open loop owned by the live-arm box until the arm exists; the box that
writes the archive turns it closed with `signal` (cost per kept box per arm), `record` (`north/climb/archive`) and a 24-hour
cadence, run on the ledger host after `north tune`. Every run writes the record, including a run that proposes nothing, because
the record's age is the proof the loop ran (docs/rationale/learning-loops.md).

## Prior art and what is copied

Search done 2026-11-06; claims marked secondary come from summaries, not the papers. Entries are in
[docs/references.md](../references.md#self-improving-agents).

- **Darwin Gödel Machine** (archive of agents, parents sampled across it, empirical evaluation, sandbox and traceable lineage): copied
  for the archive, the parent sampling and the hacking guards. Its cost (about USD 22,000 for one SWE-bench run, secondary) rules out
  searching broadly; north runs one candidate per loss per day.
- **GEPA** (reflect on trajectories, Pareto set, 35 times fewer rollouts than GRPO): copied for the generator's use of failed runs.
- **RouteLLM and contextual bandits** (verified outcome as reward, cost as penalty, regret bounds): copied for adaptive shares. The
  replay backtest of the matrix-factorisation arm found no gain (docs/rationale/learned-routing-and-goal-eta.md), so routing here
  starts from per-category cells and ledger cost per kept box, not a learned embedding.
- **AlphaEvolve** (an evaluator that machine-checks the metric): copied as the rule that a loop runs only where the metric is
  machine-checkable; replay and the ledger are that check.
- **ADAS, Gödel Agent, Live-SWE-agent, Voyager, Reflexion, SWE-agent**: prior only. Gödel Agent rewrites its own improver, the least
  controlled design, and is the case the scope guard exists to prevent. Reflexion and Voyager's verified lessons inform later
  retry briefs (inject only lessons a later success confirmed).
- Not covered for lack of time: the OPRO primary paper, OpenHands ablations, the Reflexion primary paper, Gödel Agent's measured gains, and
  FrugalGPT's primary numbers.

## Built and to build

Built in the first box: `crates/north-climb` with `north climb run --dry-run` (loss, `CheapestCell` generator, scope guard, replay
score, live arm table). Built since: the evidence settle rule, the key-level scope guard, the checker-verified reward, the Thompson shares in the dry
run and the loop's own verdicts on `climb-` experiments. Left as boxes on north#636: dispatch routing by the sampled shares;
a passing candidate registering its experiment; the graph archive and the live-arm selection with automatic revert; the Claude-session generator; the loop
record and cadence that close `policy-climber`.

## First loss it found

The dry run on 2026-11-06 (ledger 7d, API-equivalent dollars) named category tooling on sonnet as the largest loss: $995.91, of which $42.17 was repeat runs and $953.74 boxes that never closed. `Cheapest` found no cheaper tier, so its arm rule proposed routing tooling off arm `sonnet-high` (6 of 7 boxes kept at $169.15 each) onto arm `worker` (6 of 7 kept at $31.73 each), projected saving $824.48. The candidate touches only `dispatch/tier.rs`, which is inside scope, and replay reached 28 of the window's 2210 placement cases with no outcome shifted and no invariant breached. The unclosed-box dollars include umbrella issues with many runs; a later box should rank unclosed boxes by their median, as `Cell::median_box_usd` already does for closed ones.
