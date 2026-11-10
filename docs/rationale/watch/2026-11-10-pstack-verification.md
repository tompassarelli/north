# pstack's formal-verification and statechart skills, and what north built instead (2026-11-10)

Record for the followed source [pstack](../../references.md#pstack), outside its regular cycle: Tom asked on
2026-11-10 for a better version of three of its skills, built and wired to run by itself (north#1042). The previous
cycle is [2026-11-04-pstack.md](2026-11-04-pstack.md); the method is
[watching.md](../../../harness/skills/agent-policy/references/watching.md).

## What we looked at

- pstack in `cursor/plugins` under `pstack/`. Fresh clone into an empty scratch directory, read as data. Repository
  head `1e9c3f4` (2026-11-09); the last commit under `pstack/` is still `c742dd6` (2026-10-27), so the three skills
  are as the 2026-11-04 cycle saw them. License MIT (`pstack/LICENSE`). No file was copied and no text adapted.
- `create-formal-verification-skill` (43 lines), `maintain-formal-verification-skill` (58 lines) and `statechart`
  (82 lines and about 405 lines of TypeScript). Each sets `disable-model-invocation: true`: it runs when a person
  types its name and at no other time.
- North at origin/main `a990f6e20`: [state-machines.md](../state-machines.md) (the 2026-11-07 inventory, north#757,
  north#758), the `north_base::lifecycle` kernel, and the 15 machines that implement `Lifecycle` in 11 files.
- North's own earlier model: a stateright model of the lander, kept by the 2026-11-04 cycle and deleted two days
  later ([landing-train.md](../landing-train.md), "The protocol model").
- Prior art, read for the comparison and not adopted: stateright (MIT), Kani (MIT and Apache-2.0), the TLA+ tools
  (MIT, Java 11 or newer), "eXtreme Modelling in Practice" (Davis, Hirschhorn, Schvimer; PVLDB 13(9), 2020;
  arXiv 2006.00915) and "Smart Casual Verification of the Confidential Consortium Framework" (Howard, Kuppe, Ashton,
  Chamayou, Crooks; NSDI 2025; arXiv 2406.17455).

## What the three skills offer

- `create-formal-verification-skill` picks the promise where a bug costs most and tests reach least, writes a model
  of it (TLA+ by default) and holds the model to seven rules: true to the code, able to fail, findings stay findings,
  honest about scope, readable without the tool, cheap to rerun, kept in step with the code.
- `maintain-formal-verification-skill` is the pass that keeps a model in step: it reads a coverage record of which
  code the model stands for, compares it with what changed, and ends in one of four outcomes (in sync, updated, bug,
  blocked) with a sync commit.
- `statechart` runs TLC on the model (Java through mise), and a hand-written TypeScript view module turns the
  explored graph into a Mermaid chart: a budget of 500 states, about 20 arrows, and the shortest path to each broken
  rule.

The ideas are right: check the costly promise over every ordering, prove the check can fail, show people a picture
of what was checked. Four parts are half-built.

- **The model is a second copy of the machine.** A TLA+ spec stands beside the code, and the only thing tying them is
  the maintain pass, which a person must remember to run and an agent must do by reading both. North has run this
  experiment: its stateright model of the lander "checked a model rather than the lander, had already drifted from
  it (`4efb7995b`, then the batch landing of `152c6a303`), and its two seeded failures were variants of the model,
  not mutants of the code" (landing-train.md), and was deleted in north#635 two days after the 2026-11-04 cycle kept
  it. The language was Rust; the drift came from the copy.
- **Able to fail is shown on an invented variant.** The skill breaks the model in a scratch copy and watches the
  check fail. That shows the checker works, not that the model would have caught a bug the code had.
- **Nothing triggers it.** All three skills are typed by a person. No change to the code reruns the check, no
  incident asks for a model, and no run of the real system is compared with the model.
- **The chart needs a hand-written view** per model, so the picture is a third artifact that can drift.

## The 2026-11-07 inventory, reviewed

The inventory found the machines and the pattern, and its conversions are what make a check over the real code
possible, so it stays. Its method has four limits, and this lane replaces the part that goes stale.

- **Its rank is a keyword count.** It ranks by fix commits whose message names the machine times a cost weight set
  by judgment, and says itself that the count is coarse and that the incident ledger was unreadable from the
  inventorying host. Counting instead the commits that narrate a dated incident and change a machine's code (the
  registry's paths, `north check machines --replay`) gives, for the six days since the sweep: runtime promotion 19
  and its canary 8, after-landing run 16, threads spool 13, land ticket 11, fleet session 9, row delivery 3, capacity
  lease 3, gate 2, check environment 2, claim 1. The inventory ranked claims fourth and runtime promotion fifth. The
  two counts cover different days and the conversions themselves landed inside the six, so this is a second reading,
  not a correction of the first; it does say where fixes land now.
- **It was one sweep.** Six read-only passes at `289fe31ea` produced a note that marks five machines converted; three
  days later the tree has 15. Nothing reran the sweep or told the note.
- **It looks for shapes, not orderings.** The north#758 shapes (a phase in a file suffix, a status string, a
  presence flag) find state held badly. They do not find the ordering that breaks a machine once it is held well,
  and the recorded fixes of the two top machines are of that kind (below).
- **It did not read the one incident source that names machines.** The lifecycle store logs every move it refuses,
  with the machine and both states. gex45's ledger held seven on 2026-11-10 (E4), each a real disagreement between a
  writer and a table.

What replaces the sweep: `src/cli/machines.rs` is a registry of every `Lifecycle` machine in the tree, with the code
that drives each and whether it is modelled. The landing refuses a new `impl Lifecycle for` that the registry does
not name, so the list cannot go stale; the ranking is recomputed from history on every detector run instead of
written down once.

## Model source: three candidates

The brief named three, to be picked by bugs found on recorded fix parents, drift risk, cost per landing and what a
DeepSeek or Claude worker can maintain. Only (b) was built; (a) and (c) are judged from what they would have to
model and from north's own record, not from a run.

- **(a) TLA+ or PlusCal with TLC or Apalache, and a conformance replay of logged moves.** Drift is the cost. North's
  stateright model drifted in days; MongoDB's team found checking implementation traces against an abstract spec
  impractical (Davis and others); the CCF team made it work with trace validation in CI and a spec written to match
  the implementation's granularity (Howard and others), which is a standing engineering job. For north it adds a
  second language and a JVM, a spec per machine (15 today), and a replay that can only say the spec and the ledger
  disagree, not which is wrong. A north machine is already a next-state relation in the code: `Lifecycle::legal` is
  a total function over a small enum.
- **(b) A checker over the Rust state type itself.** No copy exists to drift when the explorer drives the machine's
  own `Lifecycle` impl through the real `Store`. stateright would do the search; the search here is a breadth-first
  walk of at most 32 situations per machine, and the whole explorer with its rules and chart writer is about 550
  lines, so a dependency would buy nothing.
- **(c) Kani on the transition function.** `legal` takes two values of an enum with at most nine variants, so
  enumerating it is already a proof over every input. The bugs the kernel can have live in the store (SQLite rows,
  process liveness, dwell clocks), which Kani does not reach.

Chosen: (b), with no model checker library. No dependency was added: `crates/north-machine-model` depends on
`north-base` alone, so no external-code review was needed. A TLA+ toolchain, stateright or Kani would each have
needed one; none earned its place.

## The design

- **Model source.** The machine's own `impl Lifecycle` (its table, dwells, holders and replacing moves) and the
  lifecycle `Store`, driven through its public calls on an in-memory database. The only hand-stated part is two lists
  per machine in the registry: its ends and the states a writer may announce again.
- **Checker.** `north_machine_model::explore`, a breadth-first search that replays each ordering from an empty store.
- **Scope, stated on every chart.** One instance; three processes, two on one host and one on another; each tries
  every move for every holder and settles the instance; the process holding the state dies; its dwell runs out.
- **Promises checked.** `logged`: every accepted move is in the log and the log alone rebuilds the record.
  `one-holder`: a live holder is never declared gone, and a held state is taken from a live holder only by a move the
  machine lets replace one. `ends`: an end has no way out, no dwell and no holder. `repeats`: a state is accepted
  twice exactly where the machine promises it. `stuck`: every other situation can be left once nothing more dies.
  `reach`: every state is reached.
- **Where it lives.** The explorer in `crates/north-machine-model`; the registry and the verb in
  `src/cli/machines.rs`; the check is `north check machines --root .`, which exits 1 with each broken promise and the
  shortest ordering that shows it.
- **Charts.** `north check machines --write` writes one Mermaid page per modelled machine under
  [state-machines/](../state-machines/) from the moves the store accepted during the search. The check fails on a
  page that differs from what it would write and on a page no machine owns, so a chart cannot be drawn by hand or
  left behind.
- **Cost per landing.** The ten modelled machines take 1.2 s together in a debug build (3,774 steps). The landing
  runs the search when a lane changes the kernel, a machine's table, the explorer, the registry or a chart, and a
  0.2 s scan of the registry on every code lane. Both use the north the landing already builds.
- **What a worker maintains.** One registry line per machine: its name, table file, code paths and two lists. A
  worker who changes a table gets the broken promise and the ordering in the landing's refusal.

Modelled: fleet-session, land-ticket, row-delivery, after-landing-run, red-main, base-broken, capacity-lease, gate,
claim and check-environment. Registered and not modelled, because their types are private to their crates:
runtime-promotion, runtime-canary, runtime-landing-judgement, threads-spool and offload-offer.

## Triggers

The check and the review run from two layers every provider passes through, the landing and the flow detector tick,
so no agent has to remember a skill.

1. **A lane changes a table or the kernel**: the landing runs the full search (`.safe-push`, the `check machines
   --root .` line). The search reads nothing but the tables and the store, so code that only drives a machine cannot
   change its result and does not select it.
2. **A new machine appears**: the landing's registry scan refuses an `impl Lifecycle for` no registry entry names,
   and the check refuses a modelled machine whose table the `.safe-push` line would not select.
3. **An incident fix lands in a machine's code**: the `machine-fix` detector (`north ledger flow detect`, every five
   minutes) reads `north check machines --fixes` and appends one box per machine and week to this note's owner issue.
   For an unmodelled machine the box is "Model NAME"; for a modelled one it is "Extend the NAME model to the fixes
   its check let through", counting only fixes newer than the machine's chart, which is when its model last changed.
4. **The store refuses a move at run time**: the `machine-refusal` detector reads the refusals of the last hour from
   the lifecycle ledger and appends one box per machine, refused state and week: decide whether the table or the
   writer is wrong, and state the decision as a promise. This is conformance checking without a second model: the
   store already holds every move to the table, so a refusal is the run disagreeing with the model.

The detectors are clients of an engine north already has: a flow detector is a pure function from gathered facts to
findings, each with a key, an owner issue, a title and a Done when; the filer keeps filed keys so a finding becomes
one box once, files at most 12 boxes a tick, and records each detector's time and findings in the ledger. A fifth
kind of review does not need a new mechanism; it needs one more detector.

- **Budget and dedupe.** One box per machine and week per detector, inside the engine's 12 a tick. A box names the
  machine and the fix commits, so its worker reads those subjects and one table instead of sweeping the tree.
- **No calendar floor.** A machine with no change, no incident fix and no refused move gets no review. Every change
  to it is already searched at its landing.
- **How it learns today.** Extending a model changes its chart, which restarts that machine's fix count; narrowing a
  machine's code paths in the registry removes fixes wrongly counted to it (runtime promotion went from 40 fixes to
  19 that way in this lane). Both are done by hand, so `harness/loops.toml` lists `machine-review` as an open loop.
- **What closes it** (north#1059): each machine's week doubles when its last box closed with no model
  change and returns to a week on a refused move or a box that extended the model, read from the filed keys and the
  detector yield the ledger already records.
- **Not wired.** A selection miss as a signal. north#1039 selects tests by what a lane reaches (`a0e25984b`) and
  does not yet select check lines; when it does, its reachability should replace the path list in `.safe-push`. A
  phase held outside the kernel stays north#758's shapes, whose conversion is what brings a machine under trigger 2.
  The refusal detector reads the ledger of the host it runs on, not other hosts' ledgers or the capacity runtime's
  `lifecycle.db`.
- **The other reviews.** South's `code-health-review` (hourly signal, launches a session per repository),
  `watch-review` (daily, due by age or commit count) and `skill-review-queue` (weekly, due by age) are three more
  timers with their own due rules, locks and outputs. Each fits the engine as a detector whose finding is a box that
  dispatch staffs like any other, so one filer holds the budget, the dedupe and the yield record of every review.
  Moving them is north#1059 and is not part of this lane.

## Experiments

All measured on 2026-11-10 on gex45. The check is `north check machines --root .`; it exits 0 on the lane's head and
1 in every case below.

- **E1, recorded fix parents.** At a fix parent the kernel lacks calls the explorer uses, so each run puts the
  machine's table line exactly as it stood at the parent under main's kernel.
  - row-delivery as at `8f16ce454`, the parent of `ef8a44291` (2026-11-07, a delivered row delivered again):
    `row-delivery: repeats: delivered is promised to be announced again and the table refuses it, so the second
    writer fails`.
  - after-landing-run as at `5a459f460`, the parent of `4b6bf5de4` (2026-11-08): `after-landing-run: repeats:
    journaled is promised to be announced again and the table refuses it, so the second writer fails`.
- **E2, the two top machines.** Fleet session and land ticket have no fix to their tables since they were converted.
  Their recorded ordering fixes are between instances: two launches of one session name (`97e4aa682`), a withdrawn
  head queued again (`f1ff89dde`, parent `249dbce78`), a take-over racing a delivery revive (`c3b40831c`). A search
  of one instance cannot express them, so the check passes at those parents, and each table is shown killing a
  mutant instead: letting `retired` move on gives `fleet-session: ends: retired is promised as an end and the table
  lets it move to live, stopped`; letting `checking` repeat gives `land-ticket: repeats: the table lets a writer
  announce checking again and the machine does not promise that`.
- **E3, mutants of the kernel**, which every machine shares. Treating an unseen holder as gone
  (`liveness.alive(&owner).unwrap_or(overdue)` in `release`): 28 `one-holder` and 6 `stuck` findings across six
  machines, such as `fleet-session: one-holder: b3 declared a1, a live process holding live, gone (after: a1 moves it
  to live; b3 moves it to live for a1)`. Letting any move replace a holder (`!L::preempts`): 138 `one-holder`
  findings across the same six.
- **E4, refused moves in a real ledger.** gex45's lifecycle ledger held seven, all a state announced again:
  row-delivery `delivered` five times on 2026-11-07 (fixed by `ef8a44291`), after-landing-run `journaled` once
  (allowed since `4b6bf5de4`) and after-landing-run `retired` once, which main still refuses. The detector would have
  filed three boxes; the fixes named settle two, and the third is a box on north#1042.
- **E5, the trigger replayed** (`north check machines --replay --days 30 --root .` at `a990f6e20`). Commits began to
  narrate dated incidents on 2026-11-05 (50 that day, 144 on 2026-11-07, under 10 a day before), so the 30 days hold
  about six days of signal: 486 narrating commits, 87 of them fixes in the code of 11 of the 15 machines.
  - Signal (trigger 3): 12 machine reviews, 1 with no fix of that machine in the week after; 67 of the 87 fixes
    landed within 3 days after their machine's review and 76 within 7.
  - Weekly sweep of all 15 machines: 75 reviews, 57 idle; 28 fixes within 3 days, 87 within 7.
  - Monthly sweep: 15 reviews, 15 idle; 0 and 0.
  - Sweep placed by churn (as many as weekly, each after an equal share of changed lines): 60 reviews, 35 idle; 71
    within 3 days, 87 within 7.
  - Read with care. In the one dense week the signal costs about what one weekly sweep costs (11 reviews against 15
    over 7 days); its saving is the quiet weeks, and the earlier weeks of this history carry no narrated signal
    either way.
    Churn places sweeps well here because fixes follow churn, at five times the reviews and naming nothing. What the
    signal buys in the measured week is a review within five minutes of a machine's first fix that names the machine
    and the fixes; "preceded" means the box was open before the later fixes landed, not that it would have prevented
    them.
- **E6, how much of the signal is ordering bugs.** Of the 20 incident fixes counted to fleet session and land
  ticket, 3 are plainly ordering or lifecycle bugs (`c3b40831c`, `97e4aa682`, `f1ff89dde`) and up to 6 on a generous
  reading. All are between instances. The signal finds where a machine's code breaks; most of what breaks there is
  not yet something this model can state.
- **E7, the first landing was reverted.** This design landed as `a84a8a9ca` and the sheriff reverted it
  (`e67844c8b`) when `land_checks::north_land_check_refuses_a_lane_that_adds_a_reference_its_module_layers_forbid`
  went red after landing. That test copies north's `.safe-push` into a fixture that holds no machine and expects the
  landing to exit 1 on the layer refusal. The registry scan searched the fixture with `git grep`, read git's exit 1
  for "no line matches" as a failed command and exited 2. The scan now takes exit 1 as an empty answer. The
  pre-landing selection ran 203 integration tests and not this one, because the change graph's answer had replaced
  the module table that selects every module spawning the north binary; `a0e25984b` (north#1039) joined the two the
  same day.
- **Cost.** fleet-session 13 situations and 363 steps in 88 ms, land-ticket 20 and 606 in 179 ms, row-delivery 18
  and 350 in 94 ms, after-landing-run 32 and 951 in 367 ms, red-main 7 and 107 in 31 ms, base-broken 4 and 48 in
  14 ms, capacity-lease 23 and 708 in 262 ms, gate 20 and 553 in 134 ms, claim 5 and 61 in 15 ms, check-environment
  3 and 27 in 7 ms.

## Why this beats pstack's version

- The model is the code, so there is no maintain pass, no coverage record and no sync commit to forget.
- Able to fail is shown on tables the repository once had, and on mutants of the code under check, not on variants
  of a model.
- The check runs at the landing of every change that can alter its result, in 1.2 s, with no toolchain beyond cargo.
- The chart is generated from the search and refused when stale; no view module exists.
- A fix or a refused move opens the review by itself, with a budget and a key.

What pstack's version has and this lacks: a TLA+ spec can state a promise across many instances and processes with
no code to drive. North's search is bound to one instance of one machine.

## Decided

- Ideas only; no pstack file copied or adapted.
- Keep the design above. Do not add a TLA+ toolchain, stateright or Kani for these machines.
- Boxes on north#1042 for what is not done: promises across instances in the kernel, so fleet session and land
  ticket fail at `97e4aa682`'s and `f1ff89dde`'s parents; the refusal detector on every host's ledger;
  after-landing-run's refused `retired`.
- No box by hand for the five private machines. The `machine-fix` detector files "Model NAME" for runtime-promotion,
  runtime-canary and threads-spool on its first run (19, 8 and 13 incident fixes in the week), and for
  runtime-landing-judgement and offload-offer when a fix lands in their code.
- north#1059 owns the loop's learned wait, tuned on a rerun of `north check machines --replay` once four weeks of
  narrated incidents exist, and the move of the code-health, followed-source and skill reviews onto the detectors.

## Next review: start here

A review of pstack's `create-formal-verification-skill`, `maintain-formal-verification-skill` or `statechart` starts
from this record, not from the skills. Report first whether what was kept is live: `north check machines --root .`
exits 0 on main, `rg -c 'check machines' .safe-push` prints 2, the charts under `docs/rationale/state-machines/`
match, and the `machine-fix` and `machine-refusal` detectors have filed or stayed silent for a reason the ledger
shows. Then read pstack from `c742dd6` and ask only what it now does that the list under "Why this beats" does not
cover, starting with promises across instances.
