# The self-improvement loop: what runs, what was cut, and the rungs that bring it back

Audit and plan of 2026-11-12 for north#1279: which of north's learning runs today, how the loop that changes structure lost its owner between 2026-11-07 and 2026-11-10, what outside work says about a harness taking over its model's judgment, and the order in which north builds that. Tom asked on 2026-11-12 at what point the machinery north builds starts to do the thinking its models do now, and said the same day that self-improvement is central to north's purpose, that cutting it was a mistake, and that the plan restoring it should be bold.

Owner: north#1279

## The answer in short

- Machinery takes over a judgment one class at a time, never all at once. A class crosses when its inputs are recorded, its outcome is graded by a machine and not by a report, and a mechanism written from that history decides cases it has not seen no worse than the model, for less.
- No class has crossed yet. North learns numbers: an ETA, a budget, a core count, a deadline, an arm share. Every judgment is still a model's.
- The limit is not ambition or machinery. It is graded outcomes: 8% of ticked boxes have a recorded passing run of their own check, and a loop can settle only on those.
- The model is not replaced. Its work moves from deciding the familiar to proposing mechanisms and reading the unfamiliar, and what north has compiled and recorded carries over when a better model arrives.

## What runs, measured 2026-11-12

Read on the laptop (the lander host) unless a host is named.

- **Registry.** `north loops check` prints 0 of 22 failing: 14 closed loops, 7 open ones and the red-main line. The three ledger loops (tier, eta, budget) read ok on the ledger host, updated an hour before. Every closed loop sets a number or files a box from a count; none changes how a decision is made.
- **Replay.** `north ops strategy replay --since 24h` decided 9,432 logged cases again (5,014 admission, 3,669 placement, 749 landing) in 0.03 s; one admission verdict changed. These are the harness's own decisions. No judgment a model makes is recorded as a case.
- **Claims and evidence.** The graph holds 476 done claims on 459 boxes. `harness/graph-rules/corroborated.json` corroborates 384 of them: 370 by a landing and 14 by a passing run of the box's own check.
- **Graded outcomes.** Of 1,971 boxes, 557 are ticked. 232 of the ticked (42%) carry a runnable Done-when command, and 44 (8%) have a recorded passing run of it. 104 boxes hold a recorded pass and no tick, which nobody has read.
- **Corrections.** 57 corrections in 7 days, 16 of them in no class. `harness/corrections.toml` names 19 classes, 5 carry a guard date, and `harness/graph-guards` holds 3 compiled guards.
- **Experiments and arms.** `docs/rationale/experiments.toml` holds 55 entries judged on their review day, and dispatch draws each category's model and effort by Thompson sampling on cost per kept box (north#939). North does intervene; it does not only correlate.
- **The fleet's own result.** `north dash --weekly`: worker first-try success 59% (1,180 of 2,006) against 69% the week before, rework within 48 h 13% in both weeks.

Three earlier measurements bound what a mechanism has replaced so far. The learned tier pick did not beat the cell-rate rule on 806 decisions (2026-10-12, [learned routing](learned-routing-and-goal-eta.md)). Similar-task retrieval showed no gain (2026-10-15, [measured no gain](measured-no-gain.md)). Closing confirmed reports by rule would have covered 13.8% of the coordinator's turns and was dropped ([close-by-rule](close-by-rule-share.md)).

## What was cut, and how

- 2026-11-06: the [policy climber](policy-climber.md) is decided under north#636 and its first commit lands (3bee302ba5). It was the one loop meant to change policy structure from outcomes.
- 2026-11-07: north#719 cuts north to its spine (take a brief, run a worker, land the change, record the result) after three days in which the code grew by half while most scoreboard bars failed. The same evening north#112 (the learning loop) and north#636 are closed as not planned, as outside the spine.
- 2026-11-10: north#970 deletes the climber (bce9c60404, 922 lines) on the subsystem ledger of 2026-11-07: 9 fix commits and 0 decisions in 14 days, and a run that refused every call without `--dry-run` ([leaner north](leaner-north.md)).

The cut had cause, and the retirement was by the book. The method had three faults:

1. **The window.** A 14-day decision count was read against a crate that was one day old.
2. **The order.** Its owner issue was closed three days before the count was acted on, so the box that would have given it a live path could not be staffed. Zero decisions was the only possible reading.
3. **The source of scope.** The spine was read from the README, and the README did not name learning as part of north's purpose. An agent told to keep what the project's stated goals cover could not keep it.

What the cut got right matters as much. On 2026-11-08 the climber's reward, read through the corroboration rule, found 0 checker-verified boxes across 53 arms ([world model](world-model.md)). A selector with nothing to select on would have earned no decisions with a live path either. Restoring it as it was is not the repair.

What the cut left behind: six of the seven open loops in `harness/loops.toml` name a closed issue as the box that will close them (north#112 four times, north#636, north#939), and the check that says nothing is hand-set without an owner reports no failure, because it does not read whether the owner is open.

## What outside work says

Sources are in [docs/references.md](../references.md#machinery-that-takes-over-a-models-judgment). Figures come from abstracts and search summaries read on 2026-11-12, not from full texts, except where a north note already cites the paper.

- **Compiling deliberation into rules is old, and it has a known cost.** Soar turns the result of working through a subgoal into a production that fires next time without the work. The utility problem is that learned rules can cost more to match than they save, so a system can get slower as it learns. North met the same thing as code that outran its gain. Every compiled mechanism therefore carries its saving against its repair, and the ledger that admits it can delete it.
- **A strong model writing tools for a cheaper one works when the tool is executable and checked.** In "LLMs as tool makers" a function written once by the strong model lets the weak one match it on the task. Agent skill induction reports that skills kept as verified programs beat the same lessons kept as text. A compiled judgment in north is a rule or a function with replay cases, never a paragraph in a skill.
- **A world model built as code scales as many small programs, not one large one.** WorldCoder writes its model of an environment as a program and plans against it without a model call per step; PoE-World reaches thousands of lines by multiplying hundreds of small programmatic experts. This supports north's choice of 2026-11-07 against a separate solver: the graph plus many narrow rules is the model.
- **The verifier stays outside what it verifies.** The LLM-Modulo position is that a model proposes and sound external critics dispose. AlphaEvolve runs only where a program can score the candidate. The 2026 survey of self-improving agents asks that the component proposing an update be separate from the one accepting it, that a critic change only by additions under audit, and that noisy improvements stay in the scaffold until stable before any is distilled into weights.
- **Harnesses that rewrite themselves gain, slowly, and only under held-out acceptance.** The Darwin Gödel Machine went from 20% to 50% on SWE-bench and faked a metric by deleting what detected the fault. A study of self-improving code agents found most optimizations on one benchmark were proxy gains. AIDE² accepted 7 of 99 rewrites of its own harness, judged on tasks it had not seen, and its reward hacking fell as a side effect. Later systems mostly cut the cost of scoring: a cheap comparison gates the costly run. One reports no gain where the task is beyond the backbone model. So candidates and their scoring must be cheap, about one in fourteen survives, and the ceiling of the loop is the model that proposes.
- **Weights come last, and narrow.** A 32-billion-parameter open model trained on about five thousand agent trajectories reached 40% on SWE-bench Verified; SERA specializes an open model to one repository at a fraction of the cost of reinforcement learning; classifiers distilled from a large model's labels run far cheaper than the model on the same classification. A provider's terms decide what may be trained on: Anthropic's help article of 2026-03-16 allows models that do not compete with its own (classification, categorization, extraction, anomaly detection) and forbids competing ones and the use of outputs as targets for general generation.
- **Discovery by small edits works only where small edits mean something.** Lenat and Brown found that the method which discovered concepts could not discover new heuristics, and concluded that representation plays a crucial part. A rule language in which a one-line change is a different, checkable decision is the precondition for rung three; the graph's rule files and the typed case records are that language here.

## Tuning against learning

An outside model, given the repository on 2026-11-12, put the line here: tuning improves a process north already measures, and learning is getting better at making and testing new ways to understand a problem. It proposed a test. Can north notice a failure pattern nobody programmed, state an explanation, devise the check that would refute it, and produce a mechanism that helps on cases it was not built from?

North adopts that as the bar of rung three, with two corrections. North already intervenes, through 55 registered experiments and sampled arms, so the missing part is choosing what to test, not testing. And every hard step in the test is still a model's: the test measures whether the harness keeps and proves what its models work out. That is the part a harness can own, and it is how capability has always accumulated around a mind that does not itself change: in what is written down, checked and handed on.

## The rungs

Each rung starts with a measurement or a fix that has a stop bar, and each is north#1279's box of the same order.

0. **Graded outcomes.** Half of a week's ticked boxes carry their own check and a recorded passing run of it, and the scoreboard prints the share. A box is launchable only once its check has failed on the base commit. This is first because every rung above settles on it.
1. **Numbers.** Built: the 14 closed loops. The repair is the registry: each hand-set decision names a live owner or is recorded as hand-set by decision, and the check fails on a closed owner.
2. **Rules.** A census of one frozen day ranks the judgments the fleet's models repeat by tokens spent and by the share a machine graded. The top class is recorded as cases beside the placement, admission and landing cases. A worker writes the rule; replay on later cases proves it; it runs in shadow, then decides live with the model as fallback outside what it has seen and a sampled share still judged by the model. The scoreboard gains the share of model judgments a proven mechanism decides.
3. **Questions.** A registered loop takes the largest measured loss no open issue owns, states two explanations whose predictions differ, runs the check that separates them (replay first, a bounded live arm only for a survivor) and keeps a mechanism only when it holds on a class or repository it was not built from. Where a mechanism's predictions fail most is where the loop looks next. The proposer cannot edit the replay, the checker, the ledger's counting or a bar. The loop is opened as an experiment with a bar and a review day, and judged on that day.
4. **Specialists.** On frozen cases of one judgment class, a small open-weight classifier trained only on north's own machine-graded cases is compared with the model's verdicts under a rule fixed before the run. It runs on a host north already has.

## Rung three as a daily turn: the plan of 2026-11-12

Rung three starts as a turn a fleet worker runs once a day, not as a crate. On 2026-11-12 the owner approved starting it that day. The first turn was run by hand the same day (north#1279, first comment): four ledger queries refuted one of two explanations for the fall in first-try success. The `Loop:` boxes of north#1279 build the rest, in the order below. Each decision rests on the code as it stood on main that day.

1. **Launch.** The `mechanism-discovery` flow detector files one box a day on north#1279, and dispatch staffs it like any other box. A timer that launches its own session, as `skill-prune` and `incident-retro` do, was not chosen. Precedents: the code-health review and the followed-source review each became a detector box whose detail names a brief (`north watch-review --help` now leaves a due source to "the followed-source detector's box, not a launch"). Owner decisions of 2026-11-10 back this, under north#1059: reviews fire on a signal, not a calendar. Here the signal is an untaken loss, capped at one turn a day. The detector's filer already files once per key and comments, instead of filing a second box, while a box of the same title is open (`crates/north-flow/src/detectors/filing.rs`). As a dispatched box, a turn gets route and pace admission, the worker brief header, an ETA, a budget and a ledger run row. A session kept alive between turns is not wanted either: the fleet tick retires a session quiet for `fleet_stale_hours` (2 h), and the questions already taken are read from the turn log, not from a session's memory.
2. **Brief and scope.** The brief is `harness/mechanism-discovery/turn-brief.md`, beside `harness/code-health/review-brief.md`. A turn writes one `[[turn]]` to `docs/rationale/discovery-turns.toml` and adds entries to `docs/rationale/experiments.toml`. Mechanism code is filed as a box and built by an ordinary worker under the ordinary gates. As of 2026-11-12 no landing gate limits a lane to the paths its brief names. The rule therefore gets a land scan, `turn-scope`, that the turn's own deliverable identifies: a lane that adds a turn may otherwise only add lines to the registry. The scan is shadowed and then enforced under the gate-shadow rule of `harness/gates.toml`, and it is held in `harness/holdlist.toml` so that `holdlist-guards-resolve` refuses a lane that drops it without a stated reason.
3. **Tick and score.** A `[[turn]]` that has landed on origin/main is a finished turn. The detector writes one file per turn under `north/mechanism-discovery/turns`, dated by the landing commit. `north loops check` reads a record loop's newest write (`crates/north-tune/src/loops/hosts.rs`) and fails it past twice its 24 h cadence. The score line counts the turns and the `discovery-` registry entries the judge kept, dropped or has not yet reviewed.
4. **Choosing the question.** The detector ranks losses in one unit, runs. The two sources are each class of `north dash losses --since 7d` (73 first tries lost to the harness in the week to 2026-11-12, 40 of them provider errors) and each `north dash --weekly` row with a k/n figure in both weeks, counted as n times its move for the worse. A key is taken while a turn of the last 28 days holds it. A turn that finds an open issue already owning its loss records `owned`, which takes the key, and moves on to the next one its box lists. Until rung 0 holds, these sources and the decision log, run rows, git and the lander are all a turn reads.
5. **Stop rule.** The last `Loop:` box is deferred in the experiment registry to 2026-12-17. The loop is kept if it finished at least 20 turns in 28 days and the registry kept at least one of its mechanisms on a class or repository the mechanism was not built from. If not, the box reopens to delete the detector, the brief, the scan and the loop entry. The turn log stays as the record.

## Census of 2026-11-11 (box one of north#1279)

Frozen day: 11 November 2026, UTC. Read from the laptop's Claude Code transcripts (five account profiles, files modified on 11 or 12 November, decisions timed on 11 November). The laptop's ledger holds no fleet worker run for that day: its 76 runs are DeepSeek canary probes, so the transcripts are the record. Hosts that run their own sessions are not in this count.

A decision is the first tool call of one API message; a message with no tool call is an answer. Classes come from the first line of the command. New tokens are input, output and cache creation; cache reads are shown apart. Machine-graded means the decision's own result carries a verdict (a test or check summary, an exit code, a lander line), or a check ran in the background and a verdict line appears later in its session.

| Class | Decisions | Sessions | New tokens | Cache-read tokens | Machine-graded decisions | Tokens in graded decisions |
| --- | --- | --- | --- | --- | --- | --- |
| locate (read, search, inspect) | 1,109 | 61 | 2,650,341 | 100,134,660 | 0 (0%) | 0 |
| edit (change files) | 244 | 59 | 471,317 | 22,982,712 | 0 (0%) | 0 |
| coordinate (fleet, gates, issues) | 191 | 20 | 456,368 | 15,644,358 | 0 (0%) | 0 |
| check (run a build, test or check) | 134 | 23 | 203,497 | 14,188,194 | 76 (57%) | 103,492 |
| land-finish (finish call, hand-in, safe-push) | 130 | 57 | 124,663 | 9,444,350 | 77 (59%) | 53,268 |
| wait (monitor, schedule, poll) | 122 | 19 | 131,907 | 14,908,871 | 5 (4%) | 3,583 |
| answer (no tool call) | 123 | 42 | 97,936 | 11,436,168 | 0 (0%) | 0 |
| land-git (commit, push, PR) | 52 | 27 | 65,184 | 4,544,411 | 6 (12%) | 5,627 |
| other shell | 5 | 3 | 7,150 | 143,537 | 0 (0%) | 0 |
| **All** | **2,110** | **64** | **4,208,363** | **193,427,261** | **164 (8%)** | **165,970 (4%)** |

Ranking. The class with the most new tokens is locate, 63% of all new tokens, and no machine grades any of its decisions: nothing checks whether the next file read was the right one, so no rule can learn from its outcomes. Ranked by tokens a machine graded, the top class is check: 103,492 new tokens in 76 graded decisions across 23 sessions, with 57% of its decisions graded. Land-finish is second with 53,268 tokens in 77 graded decisions, and it grades a larger share (59%) on fewer tokens.

Top class and its cases: check. Its 76 recorded cases (a command, the verdict it drew and its time) are kept in south's agents/decisions as a private record, not here. The next step of this box is a rule for check: which build or test filter a change needs, decided from the paths it touches, replayed on these cases.

## What north will not do

- **No restored climber.** Its design note stays as the record; rung three takes its scope guard and its reward rule and gives them a reward and a live path first.
- **No general world model, second fact store or solver.** The decision of 2026-11-07 stands ([semantic physics review](semantic-physics-review.md)).
- **No general model trained on a hosted provider's outputs, and no paid API call.**
- **No mechanism without its proof and its utility line.** A compiled rule that costs more repair than it saves is deleted by the ledger that admitted it.
- **No loop that can edit what scores it.**

## Method

Registry and loops: `north loops check` on the laptop and `north loops check --role ledger` on the ledger host. Replay: `north ops strategy replay --since 24h`. Claims: `north graph ask` over `harness/graph-rules/corroborated.json` and over the `done-when`, `ticked`, `text` and `check-passed` predicates of the laptop's graph, which holds what that host ingests; the ledger host's graph was not read. Corrections and experiments: the files named. Issue states and closing notes: the issues named. Weekly figures: `north dash --weekly`.
