# Context economics: what a worker's context costs, and where it compacts

Decided 2026-11-06 under north#636. Tom asked for a deep pass on context and cache cost, one of the larger levers on
account usage, after reading a community thread and two reviews of it ([references](../references.md#context-thread)).
The thread was a source of hypotheses only. Every number below comes from the installed Claude Code 2.1.318 binary,
north's transcripts or its ledger.

## Method

- Transcripts: every Claude transcript under `~/.local/state/north/profiles/claude-{a,b,c,d}/projects` on whiterabbit
  changed since 2026-10-19 (6,180 files, 2,319 with API requests). Streamed records were deduplicated by request id,
  which leaves 265,855 requests. Each request was priced at list price from its `usage`: Opus 5.5 at $4 input, $20
  output, $0.20 cache read and $8 one-hour cache write per million tokens; Sonnet 5.5 at $2, $10, $0.20 and $4; Haiku
  5.5 at $0.10, $0.50, $0.01 and $0.20 ([pricing](../references.md#claude-code-compaction)).
- Roles: each transcript was joined to `runs.session` in a read-only copy of `~/.local/state/threads/snapshot.db`
  (2,265 of 2,319 files matched).
- Closed boxes: `north ledger summary --since 18d --json` and `--since 30d`, in the ledger's own API-equivalent dollars.
- Compaction point: the threshold code in the binary, the `compact_boundary` records in the transcripts, and `/context`
  run on a fresh session twice: once as launched and once with `CLAUDE_CODE_AUTO_COMPACT_WINDOW=300000`.
- The scratch extraction and the replay lived in `/tmp` and are not committed. This page records the numbers.

## What the measurements say

### Re-reading cached context is two thirds of spend

Over the 18 days the requests cost $13,589 at list price. Cache reads were $8,966 (66.0%), cache writes $2,697 (19.8%),
output $1,925 (14.2%) and uncached input $2. Every cache write was a one-hour write (265,822 requests), and none was a
five-minute write. Fleet workers spent $11,909 (87.6%) over 1,073 sessions, parent sessions 6.7% and the proxy 3.0%.

Each turn re-reads the whole cached context, so a turn's read cost grows linearly with context. The mean read per turn
by context band:

- under 50k: $0.006
- 100k to 150k: $0.024
- 200k to 250k: $0.044
- 300k to 350k: $0.064
- 400k to 450k: $0.083
- 450k to 500k: $0.090

Requests at 300k or more were 14.4% of requests and 31.3% of read spend.

### The effective compaction point is 467k, and an override applies to the window less its output reserve

The binary computes the threshold as the window less min(max output, 20,000) less 13,000. `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=p`
makes it min(floor((window − 20,000) × p / 100), window − 33,000). The two settings do compound, but 300k at 75%
compacts at 210,000, not at the thread's 225,000. `CLAUDE_CODE_AUTO_COMPACT_WINDOW` outranks the `autoCompactWindow`
setting, and the model's window caps both.

North sets `autoCompactWindow` to 500,000 (nixos-config `modules/north-profile/context.json`) and no override, so
sessions compact at 467,000. Three checks agree:

- 145 of the 153 automatic compactions in the window fired between 460k and 470k tokens (Opus mean 468,516).
- On a fresh session, `/context` reported a 500k window with a 33k autocompact buffer.
- With `CLAUDE_CODE_AUTO_COMPACT_WINDOW=300000` set at launch, it reported a 300k window with the same 33k buffer. So the
  window can be set per launch, and that launch compacts at 267k.

After an automatic compaction the context was 48.8k at the median. The first turn after it rewrote 30.6k tokens on
average ($0.24).

### Cost per closed box rises steeply with peak context, and the done rate does not fall

From the ledger's 866 closed boxes over 18 days (mean dollars per box, and runs per box):

- peak under 100k: 107 boxes, $1.31, 1.6 runs
- 100k to 200k: 353 boxes, $4.28, 2.5 runs
- 200k to 300k: 256 boxes, $11.52, 3.7 runs
- 300k to 400k: 87 boxes, $24.63, 5.4 runs
- 400k or more: 63 boxes, $42.61, 6.7 runs

The 150 boxes that peaked at 300k or more (17%) took 51.2% of closed-box spend. The done rate of the 2,620 Claude worker
runs is flat across the same bands: 47%, 54%, 52%, 58% and 56%. The 2026-10-12 source line of `handoff_ctx` ("0% over
400k") no longer holds. Peak context and box size are confounded, so these bands say where the money is, not what a
lower window would save. The replay below estimates that, and the arms measure it.

### Waits past the cache TTL cost 3% of spend

After a wait of more than an hour, 339 requests rewrote at least half their context at the two-times write price:
$426.90, 3.1% of all spend ($415 more than warm reads would have cost). Workers made 249 of these resumes ($343.70,
2.9% of worker spend), parent sessions 61 ($53) and the proxy 24 ($18).

The ledger's `idle_gap_misses` counts only 102 of the 339. It counts a resume only when the cache read is at most 5%
of context, but the shared system and tools prefix (about 12k tokens) stays cached across sessions on an account. That
puts 218 real resumes at a 5% to 25% read share.

### The one-hour TTL pays for itself

4,164 requests came 5 to 60 minutes after the previous one. Under a five-minute TTL they would have rewritten their
cached context at 1.25 times input, which would have cost $3,644 more. The one-hour write premium over five-minute
writes was $1,011. Net, the one-hour TTL (Claude Code's main-session default for north's sessions, plus north's
`subagentPromptCacheTtl = "1h"`) saved $2,633 in 18 days. Keep it. Subagents spent 11.6% of the total, and 0.6% of
their requests followed a 5-to-60-minute gap.

### The handoff arms did not win

In the 30-day closed-box report:

- handoff-150k: 38 boxes, 25 closed, 21 kept, $290.91, so $13.85 per kept box
- no-handoff: 57 boxes, 33 closed, 30 kept, $277.00, so $9.23 per kept box
- the retired 350k arm: 27 boxes, 18 kept, $8.49 per kept box

The kept shares are alike (55% and 53%). Within the noise below, handoff-150k shows no gain on any reading.

The 350k worker-handoff hook fired 80 times on 2,620 Claude worker runs in 18 days. It was ignored 69 times, and 138
runs passed 350k. The nudge and Claude Code's compaction are two mechanisms for one decision, and the nudge is mostly
ignored.

### Ledger pricing errors that bias these comparisons

- `crates/north-pace/src/router/prices.json` priced Sonnet 5.5 cache reads at $0.10 per million against the list price
  of $0.20. Reads are 74% of Sonnet spend, so the closed-box report understated Sonnet's cost per box by about a third.
  That tilted the Sonnet-against-Opus tier arms toward Sonnet. Corrected in this change.
- `Prices::weight` in `crates/north-core/src/providers/claude/runs/telemetry.rs`, which splits an account's plan dollars
  across concurrent runs, prices writes at 1.25 times input and reads at 0.1 times. Claude Code writes at 2 times, and
  Opus 5.5 reads at 0.05 times. Over the window its formula gives $19,082 against the list price of $13,589. Left as a
  box.

## Replay: what a lower window would save mechanically

Each worker transcript was replayed with a lower threshold. When context minus the tokens already shed crosses it, a
compaction happens. Its cost is one read of the context, 20k summary output tokens (an estimate, because the
compaction call writes no usage record) and a rewrite of the 49k post-compaction prefix. Context then continues from
49k and grows by the same turns. The read saved is the context shed. Over the 18 days, against $11,909 of worker spend:

- window 200k (threshold 167k): 1,191 compactions, $3,503 of reads saved, $846 of compaction, net $2,657 (22%)
- window 300k (threshold 267k): 336 compactions, $1,934 saved, $250 of compaction, net $1,684 (14%)
- window 400k (threshold 367k): 186 compactions, $1,092 saved, $149 of compaction, net $943 (8%)

These are upper bounds. The replay holds the turns fixed, but a compaction that loses working state costs turns and
rework. That loss is what the arms measure, so the replay picks no winner. 207 of the 1,073 worker sessions crossed 267k.

## Design: one decision point (Tom, 2026-11-06)

The decision is remaining work against context cost. Continuing at context C for R more turns costs about R × C × the
read price. Compacting to c costs one compaction, K, plus R × c × the read price. So compacting pays when
R > K / ((C − c) × read price).

With c = 49k and K about $0.73 on Opus (a $0.09 read, a $0.40 summary, a $0.24 rewrite), the break-even is 18 more
turns at 250k, 15 at 300k and 10 at 400k. On Sonnet, K is about $0.41, so the break-even is 10 turns at 250k and 8 at
300k. A worker landing its last box with a few turns left should finish without compacting. A worker with a long road
should compact early. Only the working agent holds the context to judge which.

- **Backstop: a per-launch window, armed (built here).** A dispatched local Claude box with no tier or effort arm
  draws one of `CONTEXT_WINDOWS` = 200k, 300k, 400k and 500k in equal seeded shares. Its brief carries
  `Arm: context-NNNk`, and `north fleet launch` turns that into `CLAUDE_CODE_AUTO_COMPACT_WINDOW` for the session. A
  relaunch keeps it. The 500k arm is the current policy. Settling the arms per category, so that deep single-issue work
  that measures better with a larger window keeps it, belongs to the north#636 box "The policy loop owns experiment
  design end to end"; nobody picks by hand. Tom expects about 300k to win; the replay alone would pick 200k, which
  is why quality has to be measured.
- **Checkpoint: the worker decides (box).** At 250k, then every 50k, the session host asks the worker for one line:
  `Turns left: N. Focus: TEXT`. The fleet watch's handoff nudge already re-fires every 50k and can be rebuilt as this
  checkpoint. When N is under the break-even for the tier at that context, the worker continues to the next
  checkpoint, with the backstop as the ceiling. Otherwise the session host sends `/compact TEXT` as its own unsigned
  frame.
  - Verified on 2026-11-06: a stream-json user message `/compact Keep the code word and the brief path verbatim`
    compacted a live session from 24,433 to 6,809 tokens, and the next turn recalled both facts.
  - `north fleet send` from another session signs its text `[from NAME] …`, so the command has to come from the
    session host, as the nudge's frames and `Runtime::compact` already do.
  - This retires the 350k worker-handoff hook, the capacity watchdog's handoff check and the handoff nudge as separate
    mechanisms. No supervisor model is used unless the worker's estimates prove uncalibrated.
- **What compaction keeps, by construction (box).** North's SessionStart hook runs only the unlanded-work check. After
  a compaction the brief, the lane, the open boxes and the running jobs survive only inside the summary.
  - Claude Code's built-in primitive for this is SessionStart with source `compact`. North should use it to re-inject
    the session's brief (the fleet record's prompt), its worktree, branch and unlanded commits, the unticked boxes of
    its Item, and its registered background waits.
  - Decisions and dead ends live in the worker's own focus text.
  - Nothing else is missing for a fleet worker. The repeat-runs audit found 916 of 1,344 worker ledger rows without
    `brief_text`, and the fleet record holds the prompt either way.
- **Calibration (box).** The worker's turns-left estimate is logged with the run and scored against the turns that
  actually followed. The near-done cutoff is then set per tier and category from that score, not guessed.
- **Idle workers.** Waits past the TTL cost workers 2.9% of their spend. The time-gated-box rule on north#636 already
  ends a worker whose only remaining check is a wait (`Stop: wait until`). No arm is opened for this: the most it could
  recover is $344 in 18 days, less the cost of rebuilding the context.
- **Retired here.** Dispatch no longer assigns `handoff-150k` or `no-handoff`; the context arms take those boxes. The
  dispatch arm code and `handoff_arm_ctx` are deleted. Already running sessions keep their nudge until the checkpoint
  box replaces it.

## Judging the arms, and the noise at 15 boxes

Each context arm is judged as every arm is: on closed issues and cost per kept box from the closed-box report, with a
kept share no lower than the 500k arm's.

Cost per closed box is heavy-tailed. In the 30-day report its coefficient of variation is 0.84 to 1.54 across cells
with 15 or more closed boxes. At 15 boxes per arm, the standard error of an arm's mean is 22% to 40% of the mean, and
the standard error of a two-arm difference is 31% to 57%. The replay's whole-fleet effect of 8% to 22% sits inside that
noise.

Restricting the comparison to boxes whose peak crossed the lowest arm's threshold helps. Those are the only boxes a
window treats, and the same rule can be applied to every arm. On those boxes the effect is about twice as large, but
at a CV near 1 it still needs about 200 boxes per arm to show at the usual levels.

So at 15 boxes a point estimate will crown a winner by chance. The two existing north#636 boxes own that problem: "An
arm settles on evidence, not on a point difference" (bootstrap before settling) and "The climber's reward is the one
its design names" (checker-verified boxes, not worker-ticked ones). The context arms inherit both and add no judge of
their own.

## Review

Overturned by: a context arm whose cost per kept box, among boxes whose peak crossed 167k, is lower than the 500k arm's with a kept share no lower, under the settle rule the adaptive-shares and settle-on-evidence boxes of north#636 define; or every arm short of 15 closed boxes by 2026-12-07, which means dispatch reaches too few boxes and hand launches must draw the arm too.
