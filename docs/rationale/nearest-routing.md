# Nearest-box routing: start each box where boxes like it finished, and climb on worker-caused failure

Decided 2026-11-07 under north#705. Tier routing reads a box's self-declared `Category:` and four facet lines, looks up
that cell's success per tier in the worker ledger and falls back to the cheapest priced tier when none clears 80%. On
2026-11-07 the cell [tooling, exact, module, local-test, rust] measured Opus medium at 37% first-try success (n=23) and
Sonnet at 21% (n=57), so most launches went to Opus on a rate that says little about either model. This note covers
what replaces the cell, what it is measured on, and the 14-day replay that decides whether it ships.

## What is already there, and what this builds on

- **Facet cells with shrinkage** (`north-threads` `calibration::recommend`): per-tier success in the finest facet cell
  holding `min_runs` runs, shrunk toward the category's pooled rate with a prior of 15 runs. It stays as the fallback
  when a box has too few judged neighbours, and its shrinkage is reused per rung (prior of 3 pseudo-runs at the rung's
  pooled fitness, because a neighbour set holds 20 runs, not hundreds).
- **The climber's arms** (`docs/rationale/policy-climber.md`): seeded draws from Thompson shares on cost per kept box.
  The new rule enters as an arm against the facet cell; it does not bypass the climber or `router::settle`.
- **The fact graph** (north#669): each issue's title, Done-when boxes, landings and the files each landing touched
  (`:landed-as`, `:checked-for`, `:touched`). `north_graph_ground::footprints` reads them per issue; a finished
  box's shape is its brief plus the files its landings actually touched.
- **The convo and grounding index**: it ranks transcript hits for a query with stemmed BM25-style scores. Box-to-box
  similarity needs a few thousand short documents scored against each other in-process at launch, so the router builds
  its own tf-idf over the ledger's brief text instead of calling convo per launch; convo lives in `north-ops`, which
  depends on `north-dispatch`, so the dependency could not run the other way without a new crate.
- **Prior art that found no gain.** The 2026-10-12 learned-routing backtest and the 2026-10-15 similar-task retrieval
  replay (`docs/rationale/learned-routing-and-goal-eta.md`) both kept the cell rule: logistic regression gained 2.6
  points of accuracy and wasted more runs, and brief-cosine retrieval's waste saving straddled zero at k=10 and k=20.
  Both scored success with every failure counted against the tier. This design differs in three ways the replay tests:
  outcomes are attributed before they count, a neighbour's shape includes the files it really touched, and the
  comparison is usage per finished box with escalation, not per-run accuracy.

## 1. Outcome attribution comes first

A failed run says something about a tier only when the worker caused it. The replay reads each finished run's cause from
the shared attribution of docs/rationale/harness-attribution.md (north#706), which adds records ahead of the report
rules below: duplicate dispatch, the lander's incident rows and the landings that overturned a refusal. The report rules,
in this order:

1. **Kept**: the existing `succeeded` rule (Done, not re-run within 24 h, landed when a landing was expected).
2. **The report's own `Stop:` word.** `reasoning` and `leash` (the worker ran out of approach or budget) are the
   worker's; `landing`, `capacity`, `client`, `host`, `wait` and `decision` are the harness's.
3. **Phrase rules** for the reports that end with no `Stop:` line (most of them): a host that cannot do the box (laptop,
   GPU, unreachable, a native capture or client pair), a landing refusal or queue, capacity, a collision with another
   session, compaction, a decision from Tom, or a wait on time. A `done` that was re-run with no harness phrase is
   reworked (worker); any other end is a missed box (worker); a split, or an unclear or handed-off end with no
   phrase, is left out.

Only worker-caused outcomes enter a rung's fitness. Harness outcomes still cost: the projection charges every rung the
same harness repeat rate, so they move no box between rungs. `leash` counts against fitness but never triggers
escalation (section 5), because the leash is a wall-clock bound that also measures box size, which the brief's author
sets. Runs on no Claude rung and unclear or handed-off ends that name no reason are unattributed; the replay prints
their count and dollars so a rule gap shows up as a number.

Known limits of the phrase rules, from the review round, and what north#706 fixed: they matched anywhere in a report,
so a worker failure whose report mentions a lander or a gate in passing was excused, and now match only the opening
paragraph; a landing refused by the lane's own failing checks is the worker's, and now reads so from the lander's
incident row unless a later landing of the same tip overturned it. Dropping unclear ends still favours a rung that
produces more of them, and the classes are not yet held to a hand-labelled sample.

## 2. Computed features, not declared ones

`nearest::shape` reads a box from its text with the routing and label lines removed (`Tier`, `Arm`, `ETA`, `Budget`,
`Category`, the four facets, `Follows`), so no self-declared label enters similarity:

- the repository from `Item:`;
- the files it names and the code areas they sit in (`crates/NAME`, else the first directory), which give its reach:
  unnamed, one file, one area, several areas;
- the symbols it names (backticked paths with `::`, snake or camel case, and `file.rs (a, b)` lists);
- its words, stemmed, weighted by inverse document frequency over the ledger's boxes.

A finished box's shape adds the files its landings touched, read from the graph, so a neighbour that named one file
and touched six counts as wide. A box whose brief names both files and symbols is localized by construction and never
starts above Sonnet. The replay applies the cap to the nearest-box pick only, and to any brief naming files and
symbols; the live rule caps every start path (fallback and staffing model included) and only briefs whose files sit in
one code area, since a brief naming files across several crates is not local.

## 3. Similarity and the pick

Similarity is 0.55 word cosine, 0.25 the share of the box's named areas the neighbour touched, 0.1 the share of its
named files, and 0.1 the same repository. The k=20 nearest finished runs at similarity of at least 0.15 are read. For
each start rung (Haiku, Sonnet, Opus medium, Opus high), its fitness is the similarity-weighted share of its
worker-judged neighbours that were kept, shrunk toward that rung's pooled fitness. The pick is the cheapest rung at
80% or more with at least 3 judged neighbours; with none at 80%, the rung with the least expected dollars per kept box
(median dollars of its neighbour runs over its fitness); then the Sonnet cap for a localized brief. With no rung
holding 3 judged neighbours the box falls back to the facet cell.

## 4. The staffing model makes the final pick

Revised 2026-11-07 (north#705, point 5): staffing is a model step, not a table. The nearest-box rule above becomes the evidence
and the default, not the decision:

- A Sonnet high call per box reads the box text, its k nearest finished boxes (each one's rung, effort, cause, dollars
  and the first lines of its report), the touched code's recent landings and corrections from the graph, the first
  lines of each file the brief names, and the nearest-box pick with its reason. It returns a rung, an effort and a
  two-sentence critique of the brief. The localized cap binds its answer as it binds the rule.
- Sonnet, not Opus, because the step reads evidence already ranked and chooses among four rungs; an Opus call per
  launch would cost a sizeable share of a Sonnet box's own median spend. Its pick, reasoning and the evidence ids are
  recorded on the launch record.
- It runs as a climber arm (`Arm: staffer`) against the nearest-box rule alone (`Arm: nearest`) and the facet cell, and
  the weekly score is cost per kept box with no lower first-try success, like any other arm. If it does not beat the
  rule it reads, it is retired and the rule stays.

**Cheap pre-flight when neighbours are few: not built.** The outside review proposed a fast pass that scores the brief
and the headers of the files it names before the staffing model picks, for boxes with thin neighbour evidence. The
staffing model already reads the brief, and it can be given the named files' first lines in the same prompt for the
cost of their tokens; a separate Haiku pass would read the same text a second time and add a call and a failure mode to
every thin box while deciding nothing the staffing model does not decide. The replay below counts how many boxes fall
back for want of neighbours; the pass is reconsidered only if that share is large and the staffing arm's picks on those
boxes lose to the facet cell's picks on the same boxes (comparing them with its picks on well-neighboured boxes would
confound thin evidence with new, harder work).

## 5. Start cheap, escalate on evidence

Every box starts on its pick. The dispatcher, not the proxy, relaunches it one rung up (Haiku, Sonnet, Opus medium,
Opus high, Opus xhigh) with the failed report attached as `Prior report:` when either holds:

- the worker reports `Not done:` with `Stop: reasoning`; or
- verification failed twice in the same lane without the error count falling: the second failing run of the box's
  named check (or the landing's checks) reports as many errors or more than the first. No ledger record holds a check's
  error count today; the escalation build box records it per lane from the landing's refusal (the failing checks and
  their error lines) before it can read it.

`Stop: leash`, `Stop: landing`, `Stop: capacity` and `Stop: client` never escalate, as the escalation tiers already
refuse them.

A harness cause never escalates; the existing stop holds (`Stop: decision`, `wait until`, `host`) keep the box where
it is until the cause clears, and it relaunches on the same rung. After Opus xhigh the box parks on a gate.

## 6. Learning

Each launch records its shape (repository, reach, areas, whether localized), the nearest-box pick and its reason, the
staffing model's pick, the rung run and, at the end, the cause. The climber's arms keep exploring, so the rule's own
picks are never the only evidence for themselves.

## What replaces the facet cell

The facet cell stops deciding routes. It is the fallback for a box with fewer than 3 judged neighbours on every start
rung, and it stays the cell `north tune` and the climber's loss read until the nearest-box arm settles. `Category:`
keeps two jobs: the planning and review effort arms, and a check that the brief names a real category, which
`north fleet launch` and dispatch then refuse when it does not (north#705's fourth box).

## The replay

`north dispatch route-replay --days 14` walks the first-assignment boxes of the last 14 days in launch order. For
each it uses only runs that finished before that launch, on other items, and reads its 40 nearest finished boxes: the
first 20 pick the rung, the next 20 score it, so the pick is never judged on the evidence that chose it. The old rule
is the facet cell's pick for every box, computed over the same earlier runs; the rungs boxes really ran on (the cell,
arm draws and tiers named by hand) get their own line. Each rung is projected through the same escalation ladder (at
most three attempts, one rung up on a worker-caused failure, the box's harness repeat rate charged to every rung), with
each rung's keep chance and median dollars per priced run read from the scoring neighbours, so the ratio compares
rules rather than the luck of which tier a box happened to get. Usage is API-equivalent dollars, the ledger's proxy for
the share of a five-hour window a run takes; the staffing model's own call is not charged.

Two checks sit beside the projection: the old routing's observed dollars per finished box, and the replay method (Li et
al. 2011) on the boxes whose tier the Sonnet and Opus arms drew, keeping the logged outcome of each box whose new pick
matches its draw. That check is only approximately unbiased: Thompson shares are uneven, the draw's share is not
recorded so it cannot be weighted by it, and a Haiku or Opus high pick can never match a two-arm draw.

The biases the review round found and that remain all favour the new rule: the ladder gives the next rung its average
keep chance as if the failure below said nothing about the box's difficulty and escalates every worker-caused
failure; a neighbour's touched files and its kept label come from the graph and the ledger as they are now, after the
launch; and the phrase rules (section 1) excuse some worker failures, which flatters cheap rungs. Unpriced runs are
left out of the dollar figures rather than counted as free.

### Result, 2026-11-07

Run on a copy of the threads snapshot and the fact graph taken at 3:11 AM laptop time on 2026-11-07, over the 903
first-assignment boxes of the 14 days before it (`north dispatch route-replay --days 14`).

Where the usage went, by the cause of each of the 3,814 runs that ended in the window (shares of API-equivalent usage;
the absolute figures are in south's harness decisions):

- kept: 1,142 runs, 32% of usage;
- harness: 1,118 runs, 31%: landing 362 runs and 10%, host 337 and 8%, decision 248 and 8%, capacity 119 and 3%,
  collision with another session 43 and 1%, wait and compaction under 0.1%;
- worker: 945 runs, 24%: missed the box 572 and 13%, reworked 373 and 11%;
- unattributed: 609 runs, 14%.

The rule against the old routing, 14 days, with usage per finished box indexed to the old rule's projection (1.00):

- observed, old routing as it ran: 524 of 903 boxes finished, 1.80, 41% kept on the first try;
- projected, rungs as run: 1.10, 44% first try;
- projected, old rule (facet cell for every box): 1.00, 46% first try;
- projected, new rule: 0.93, 47% first try;
- projected gain in finished boxes per unit of usage over the old rule: **1.07x** (1.18x over the rungs as run, whose
  arm draws and hand-named tiers the climber keeps under either rule);
- moves from the rung run: 160 Opus medium and 57 Opus high starts to Sonnet, 77 Sonnet starts to Opus medium, 15 boxes
  to Haiku, 551 unchanged;
- evidence: 699 boxes had no finished box at similarity 0.15 or more and 702 fell back to the facet cell; 816 boxes
  name no file, 28 name files and symbols, and 8 picks were capped at Sonnet;
- replay method on the 108 arm-drawn boxes: 51% first try as run; on the 59 where the new pick matches the draw, 53%
  first try at 1.36 times the usage per finished box. At 59 boxes neither difference is distinguishable from noise.

The last 4 days alone (`--days 4`), when nearly every launch recorded its brief: 208 boxes, 60 fell back to the facet
cell (59 with no neighbour); the new rule projects 0.86 of the facet cell's usage per finished box at 45% first try
against 46%: **1.16x**, at one point lower first-try success. With the neighbour evidence present, the rule still
misses the bar.

The projection sits below the observed usage (1.00 against 1.80) because it charges each attempt its neighbours' median
run, not the long tail; both rules carry the same model, so the ratio is the figure that counts.

## Verdict: the rule does not ship

It projects 1.07x more finished boxes per dollar than the facet cell against a bar of 2x, at one point more first-try
success, and every bias left in the replay favours it, so a fair figure is lower still. Two things hold it there:

- **Evidence.** The ledger records brief text only from 2026-11-03, and the graph's issue text rarely names files, so
  699 of the 903 boxes had no finished box alike enough to read and fell back to the facet cell. The rule cannot beat
  the cell on boxes it never decides.
- **Ceiling.** Routing moves only worker-caused outcomes between rungs. Opus costs twice Sonnet per token
  (`crates/north-pace/src/router/prices.json`), so even a perfect pick saves at most about half of an Opus box that
  Sonnet would have finished. The larger loss is elsewhere: harness-caused runs took 31% of the window's usage and
  unattributed ends another 14%. Removing the harness share alone would raise finished boxes per dollar by up to
  1/(1 - 0.31) = 1.45x, more than any routing rule measured here or in the two replays before it.

What follows, each tracked on north#705 or north#706:

- **Attribution goes into the ledger now** (north#706, first box), gate or no gate: the ingest writes each run's cause, the facet cells,
  `north tune` and the climber count only worker-caused outcomes, and the phrase rules get the fixes in section 1 and a
  hand-labelled sample. This is the clean outcome signal north#705 puts first, and every later routing decision reads it.
- **The harness causes by dollars** (landing, host, decision) are north#706's Pareto box, because that is where a 2x in
  finished boxes per unit of usage is available.
- **Escalation by the dispatcher** (section 5) and the **`Category:` refusal** ship on their own: neither changes which
  rung a box starts on.
- **The nearest-box rule and the staffing model arm wait** for a second replay on 2026-11-17, when the ledger holds 14
  days of brief text. They launch, and the live 24-hour box opens, only if that replay clears 2x at no lower first-try
  success with attribution in the ledger. The staffing model waits with the rule because it reads the same neighbour
  evidence, which 77% of boxes do not have yet.

## Reviewer round, 2026-11-07

One outside read (an Opus reviewer given the issue, this note, the code and the two earlier replays) returned ten
findings. Acted on in the code before the final run: the pick and its score now read disjoint neighbours (it had
scored the pick on the evidence that chose it), the old rule is the facet cell for every box rather than whatever
rung ran, unpriced runs are left out of the dollar figures instead of counting as free, and neighbours on the scored
box's own item are excluded. Those changes moved the gain from 1.18x to 1.07x. Acted on in this note: leash no longer
escalates, the localized cap's scope, the staffing model's file-header input, the missing error-count record for
escalation, the approximate nature of the arm-draw check, and the claims that described live wiring as built. Left as
stated limits, because each one flatters the new rule and the rule already fails: the escalation ladder's
independence assumption, post-launch footprints and kept labels, and the phrase rules' false excuses.
