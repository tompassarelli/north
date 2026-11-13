# The candidate-landing scenario (north#1233)

A box in the well-specified band can be answered before it is assigned: a candidate change is computed on the
small model in idle time, kept as a lane, and a launch lands it after the box's checks pass again on current main
(north#1240, folded into north#1233). This note registers the scenario that measures that path. It was written
before any candidate ran, so the sample and the four readings cannot be chosen to fit a result.

## The sample: twenty boxes known an hour before they are launched

An hour before a launch hour, the coordinator names on north#1233 the twenty boxes, each by its `owner/repo#N`,
and the launch hour. A box that leaves the ready set before the hour is replaced before it, and nothing is added,
dropped or re-drawn after it. The sample is every box of those twenty, landed or not; a box a launch never starts
stays in the denominator with its own reason.

Twenty is large enough that one misjudged box cannot decide the verdict and small enough to launch inside a day:
the keep threshold of 15 and the delete threshold of 10 are five boxes apart.

## The four readings

Each is computed per box and summed or ordered over the twenty; the record is one comment on north#1233 whose
first line is `Candidate scenario: 20 boxes known an hour before the launch hour` and whose body lists the twenty
as `- owner/repo#N` with the hour each launched and landed, then the four labelled lines below.

1. **Time from launch to landed.** The launch record's start to the landing span's end of the commit that carried
   the box to main, per box: `north land profile` reads a landing's spans from its queue
   (`docs/features/04-landing-and-lanes/0019-land-profile.md`). Report the median and p90 over the boxes that
   landed, and the count that never launched.
2. **Share landed from candidates.** The boxes whose landed head is the candidate lane's commit, over twenty. A
   box counts as landed from a candidate when the launch record says so and that candidate's commit is an ancestor
   of the box's main; the record is the sibling box's field, so this reading is defined against it rather than
   against today's launch record, which names only its model and route.
3. **Tokens spent on candidates that were never used.** The tokens and API-equivalent cost of every candidate
   attempt whose box did not land that candidate, read from the session's own usage row in the run ledger, summed.
   A candidate is never used when the box landed another change or nothing landed; the sum charges the idle-time
   computation for the candidates it produced and the stack threw away.
4. **Candidates a fresh worker's change would not have passed.** For each landed candidate, the box's acceptance
   command run once in a fresh lane opened from the main it landed on, not from the candidate's own lane. A
   failure there counts: the candidate passed only in the state it was computed in, which a fresh worker's lane
   does not have, so the cheap attempt bought a green check that does not hold.

## Kept apart from the same-moment comparison

The live routing cells compare a leader with its challengers on the same box at the same moment
(`harness/routing-cells.toml`, `crates/north-dispatch/src/dispatch/cells.rs`), and Eval S interleaves its three
arms task by task inside one hour. This scenario is neither: it reads one path over twenty real boxes at their own
hours, so its rows enter no cell settle and no Eval S arm table. Mixing them would charge a load or a task
difference to the candidate path, which is the confound the same-moment comparisons exist to avoid.

## Keep or delete

Keep the path when 15 or more of the twenty land from their candidate and no landed candidate fails the fresh-lane
run of reading 4. Delete the idle-time candidate computation when 10 or fewer land from candidates. Between the
two, the coordinator decides with the four readings beside the case, and the unused-candidate tokens name what the
idle-time computation costs when it is wrong.
