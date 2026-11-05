# Measured no gain: what to retire

Audited 2026-11-04 on north origin/main `7c2a76350`. An outside review on 2026-11-05 judged North's measured
intelligence ahead of its demonstrated gains. This page lists every feature, experiment and decision path whose
own recorded measurement says it did not clear its evidence bar, or which shows no use in the last 14 days. It
also lists every signal and command that changes no decision. For each it gives the measurement, the code still
on a live path, and a verdict. Account-usage pacing is out of scope; its owner is the account-capacity rationale,
which is not written yet, so the pace and account types in `crates/north-core/src/router/model.rs` and
`crates/north-core/src/router/policy.rs` (both under `#![allow(dead_code)]` in
`crates/north-core/src/router.rs`) are named here only so that owner finds them.

## Method

- Measurements on record: [feedback-loop.md](../feedback-loop.md), the rationale pages, the north issue threads,
  `north worker-ledger --summary` and `north defaults-measure --priors` against `~/.local/state/threads/threads.db`
  on gex45, read 2026-11-04. A feature is never called unused for lacking a test.
- Use in the last 14 days: every Claude transcript under `~/.local/state/north/profiles/claude-*/projects`
  on gex45 changed since 2026-10-21 (5,843 files), counting the Bash `command` fields that run `north SUB`.
  Transcripts on the laptop and Nexus were not read, and Nexus's dispatch and tune journals were not reachable,
  so a zero here means no use from gex45 sessions.
- Callers: `rg` for `north SUB` and the Rust path across north `harness/`, `bin/`, `agent-machinery/`, `crates/`
  and `src/`, south `modules/` and `agents/routines/`, and nixos-config `modules/`. A timer counts when its
  module is enabled on a host.
- Line counts are `wc -l` of the whole file, or the stated line range, including tests that go with the code.

## Did not clear its bar

### Similar-task retrieval trigger in `north flow`

- Measurement: the retrieval replay ran on 2026-10-15 over 138 targets and neither arm's waste-saving interval
  excluded zero, so routing stayed as it was ([feedback-loop.md](../feedback-loop.md#similar-task-retrieval-keep-current-routing-2026-10-15)).
  The learned routing backtest found no durable gain on 2026-10-12, and the script holding both replays was
  deleted on 2026-10-30 (cbe96542f), so the sentence saying the broad replay's CLI and arms are preserved is wrong.
- Live code: `crates/north-ops/src/ops/flow/notes.rs:9-18` and `:28-55` (`brief_coverage`),
  `crates/north-ops/src/ops/flow/mod.rs:93-94`, and its test at `crates/north-ops/src/ops/flow/tests.rs:87-106`.
  `announce` prints `tagged finished worker briefs N/300 retrieval trigger; backtest requires >500` on every
  `north flow` and `agents flow` run, including the 5-minute `agents-flow-stall` timer (nixos-config
  `modules/agents-flow-stall/default.nix`, enabled on whiterabbit). The trigger points at a replay that no longer
  exists, so the line changes no decision.
- Lines: about 62.
- Verdict: delete. Box 1.

### Claude Code advisor arm

- Measurement: tested with no gain for adding an advisor to the cheapest tier; the replay found no durable
  improvement (north#36, commits f471ef40, 784e1b90 and 8445503c, [references.md](../references.md#claude-code-advisor)).
- Live code, `north advisor-report`: `crates/north-ops/src/ops/advisor_report/` (mod.rs 292, stats.rs 107,
  tests.rs 95), `src/cli/tools.rs:60`, `src/cli/help.txt:60`, and the two priors only it reads,
  `noninferiority_margin` and `plan_per_list_claude` (`harness/defaults.toml:263-275`). No command, hook, timer
  or land check calls it, and no gex45 session ran it in 14 days. `crates/north-ops/src/ops/policy_replay/events.rs:8`
  and `crates/north-ops/src/ops/policy_replay/rows.rs:7` import `float_sum` through it; it is a re-export of
  `crate::platform::pystats::float_sum`, so they import that instead.
- Live code, the arm inside `north policy-replay`: the `--advisor` flag (`crates/north-ops/src/ops/policy_replay/cli.rs:9`,
  `:49`, `:125-127`), advisor parsing (`crates/north-ops/src/ops/policy_replay/events.rs:70-144`), the advisor
  fields and their null-scoring (`crates/north-ops/src/ops/policy_replay/run.rs:203-204`, `:215-216`, `:235-236`, `:253`,
  `:265-291`), the tests at `crates/north-ops/src/ops/policy_replay/tests.rs:140-183` and
  `tests/it/policy_replay_run.rs:151-200`, and the fixtures `harness/scripts/policy-replay.advisor.jsonl` and
  `harness/scripts/fixtures/haiku-opus-advisor.*`. Every sweep trial runs through `run_one`, so the parsing runs on
  each sweep. `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1` stays, set on every trial, so a replay never measures an
  advisor by accident.
- Lines: about 510 for the command and its priors, about 230 for the arm.
- Verdict: delete. Boxes 2 and 3.

### Planner effort band

- Measurement: `north ledger planner-band` on 2026-11-01 over 613 finished items ranked 5 of 54 off-the-rails
  items in the top decile, 9.26% against 10% chance (binomial p=0.64); the boxes were retired the same day when
  the north#242 gate took its default ([tier-routing.md](tier-routing.md#the-planners-effort-band-is-not-predicted-from-planning-time-facets)).
  That page kept `recommend --planner` printing a band that nothing routes on.
- Live code: `crates/north-core/src/ledger/planner/` (mod.rs 346, readout.rs 223, history.rs 229, tests.rs 232),
  `src/cli/ledger.rs:16-18`, the `--planner` branch at `crates/north-core/src/threads/calibration/verbs.rs:80-86`,
  `crates/north-core/src/threads/client.rs:20` and `:267`, the Python wrapper's `planner-band` verb and proxy
  branch in `bin/threads` (lines 31, 489-490, 530, 591, 683) and its checks in `bin/threads.test.sh:116-159`.
  No timer, hook or skill calls it; gex45 sessions ran it in 5 transcripts, the measurements themselves and audits.
- Lines: about 1,085.
- Verdict: delete. The tier-routing section stays as the record and loses its trade-off paragraph. Box 4.

### Handoff experiment arms

- Measurement: the bar (north#252) was "keep whichever wins on cost per Done without losing Done rate". On
  2026-10-31 `north dispatch --dry-run` on Nexus printed no-handoff winning on sonnet-high ($2.92 against $7.68
  per closed box) and handoff-150k winning on worker ($4.07 against $4.93). On gex45 since 2026-10-21 the arms
  close the same share: handoff-150k 27 done of 118 runs, no-handoff 23 of 109. On the worker tier's feature
  cell the kept arm closes fewer boxes (handoff-150k 11% of 9, no-handoff 28% of 7), and the wake-paths count
  for 2026-10-28 to 2026-11-04 found 0 handoff-nudge wakes while 23 handoff-150k runs passed 150k.
  The 350k arm never separated from no-handoff because no run reached 350k (north#252, 2026-10-29) and nothing
  assigns it now.
- Live code: `crates/north-dispatch/src/dispatch/handoff.rs` (114; `Arms::winner` at `:77-86` compares cost only,
  so the Done-rate half of the bar is not enforced), `crates/north-dispatch/src/dispatch/mod.rs:179-181`,
  `crates/north-core/src/fleet/handoff.rs` (63), `crates/north-core/src/fleet/watch.rs:221-256`,
  `crates/north-core/src/fleet/registry/launching.rs:66`, and `handoff_arm_ctx` (`harness/defaults.toml:101-105`,
  read at `crates/north-core/src/pace/limits.rs:40`). `north dispatch` runs it every 5 minutes on Nexus (south
  `modules/north-dispatch.nix`).
- Lines: about 220.
- Verdict: re-measure. Bar: on each tier with 30 launches per arm, handoff-150k stays only if its cost per closed
  box is lower and its Done rate is no more than 10 points below no-handoff; otherwise that tier runs no-handoff,
  and when no tier keeps handoff-150k the nudge is deleted. Date: 2026-11-18. Box 5 wires the Done-rate term;
  box 6 is the conditional deletion.
- Kept: the 350k worker-handoff hook (`crates/north-guard/src/guard/worker_handoff.rs`, 97, at
  `crates/north-core/src/hook.rs:199`, and `bin/capacity-watchdog:190`). It is not an arm; it fired 22 times
  over 2,675 runs in 14 days (15 ignored) and 104 runs passed 350k.

### Restaff arms escalate, cheap-fix and split

- Measurement: the bar is 15 runs per arm (`harness/skills/workers/references/tiers.md:10`, south
  `agents/decisions/harness/escalation-review.md`). `north report` prints escalate 1/3, cheap-fix 2/8, split 0/1,
  and the last such run was 2026-10-26: 12 runs in three weeks against 45 needed, so no decision is in reach.
- Live code: only prose assigns the arms (`harness/skills/workers/references/tiers.md:10`); readers are
  `crates/north-ops/src/ops/report/arms.rs` (74, via `crates/north-ops/src/ops/report/mod.rs:86` and `:118`)
  and `crates/north-core/src/threads/calibration/routing.rs:95`; `crates/north-core/src/ledger/planner/history.rs:163`
  goes with box 4.
- Lines: about 80.
- Verdict: delete. Restaffing keeps the two-failed-fixes rule the arms were tested against. Box 7.

### `threads replay-staffing`

- Measurement: its one recorded use is the 2026-10-12 snapshot reading in
  [feedback-loop.md](../feedback-loop.md) (unjudged runs, north#35 FX5). It replays the legacy tier names
  (`OLD_TIERS`) against the current ones, a comparison for a tier rename that is finished. No gex45 session ran
  it in 14 days and no timer, hook or skill calls it.
- Live code: `crates/north-core/src/threads/calibration/replay.rs` (116), its arm in
  `crates/north-core/src/threads/calibration/verbs.rs:32`, `crates/north-core/src/threads/client.rs:20`, the
  `READS` and `RUST_VERBS` entries in `bin/threads`, and the golden check at `bin/threads.test.sh:270-296`.
- Lines: about 147.
- Verdict: delete; the feedback-loop paragraph stays as the dated record. Box 8.

### Kept after checking

- `north policy-replay` sweep, run and promote: gex45 sessions ran them 193 times in 14 days, sweeps were last
  written 2026-11-04 (south `agents/replay/`), and `agents sync` and safe-push read the operating point it
  promotes. The 2026-11-04 E3 arms that found no gain were reverted by that gate, which is the gate working.
- `north wisp payoff`: it prints no retirement candidate yet (`0 retire`), but it is the measurement that would
  name one; it feeds the fleet Ctrl+T digest (`crates/north-tui/src/tui/fleet/desk.rs:206`).
- `north tune`: its dividend lines on 2026-11-03 and 2026-11-04 are all PAYING (cost per closed box $38.49
  against a $42 bar).
- Planned measurements not yet due: capacity-scheduler W2-0, pstack E1 to E3 (north#524), one-landing-per-change
  (on or after 2026-11-08), failing-first (2026-11-18).

## Signals and commands that change no decision

Each row of [feedback-loop.md](../feedback-loop.md) not read by a decision, with its owner.

- `runs.landed_sha`: in flight, the outcome-loop worker is wiring it; the rework signal already reads it
  (`crates/north-core/src/threads/calibration/rework.rs:118`), so the row's "none yet" is stale.
- The five `unverifiable` File signals rows: in flight, the outcome-loop worker is adding their reader stamps.
- `capacity_runs.command` and `cpuSeconds`, "held for north#17": north#17 closed 2026-10-27 and the capacity
  model reads both (`crates/north-core/src/capacity/host/model.rs:76` and `:172`). Verdict: wire, a doc fix
  only; the Still unread section and the two rows change to read. Box 9.
- The pointer "Thresholds and priors ... belong to north#35 lane D": north#35 closed 2026-10-13 and the 350k
  decision moved to north#252, now done. Verdict: remove the line. Box 9.
- South's 2026-09-28..10-12 per-rule incident baseline, read by "tick.md step 3": that routine no longer exists,
  so nothing read it. The 2026-11-04 harness review named it as a regression bar, but no check computes that bar,
  and the weekly incident retro samples transcripts, so its counts cannot be compared with the baseline's full
  mining of 215 sessions. Verdict: deleted 2026-11-05; the review's bar now compares `rule_ids` in
  `agents/incidents.jsonl` from one retro to the last. Box 11.
- Ten `harness/defaults.toml` entries carry `settles_when` but a source not starting `prior:`, so
  `north defaults-measure --priors` never prints them (`crates/north-ops/src/ops/defaults_measure/priors.rs:18`):
  build_input_retention_hours, capacity_mode_play_cpus, capture_batch_retention_hours, three codex_tier_*,
  evidence_retention_hours, explore_every, plan_meter_weighting, retention_budget_minutes. Verdict: wire, so the
  check lists every entry that has a settle count. Box 10.
- 22 priors print MEASURABLE-NOT-MEASURED and 26 WAIT; none is SETTLED. This is a prior with no measurement,
  not a failed one, so none is proposed for deletion; the nine codex_* priors stay as the provider seam.

Commands no timer, hook, skill or land check calls (14-day gex45 uses in brackets), and why each stays:
`advisor-report` [0] goes in box 2; `agent-contain` [0] runs inside the laptop runner
(`crates/north-runtime/src/laptop_runner/mod.rs:158`); `safe-push-result` [0] is called by `bin/safe-push`;
`session-host` [0] is the fleet's runtime host; `context` [0] was added on 2026-11-04 to measure the skill and hook
slimming; `orders` [0] is the standing-orders store `north fleet` reads; `laptop-ntfy` [0] is a nixos-config
service (`modules/nexus-laptop-ntfy`). The rest ran by hand: `agents` [138], `ops` [105], `autoland-status`
[66], `help` [62], `provenance` [57], `land-tests` [40, also called from safe-push], `wisp` [37], `wt-rescue`
[32], `code-health` [26], `nexus-run` [23], `citation-check` [14], `land-actions` [13], `worktree-layout-check`
[13], `archives` [6], `incident-mine`, `land-cache`, `secrets-proxy-exec` and `usage-web-read` [4 each],
`landed-tree` [3], `task` [2], `autoland-wait` [42].

## Deletion boxes

Unconditional deletions total about 2,114 lines (boxes 1 to 4, 7 and 8); box 6 adds about 220 if the handoff
arm loses. Each box is at most 45 minutes, lands alone through safe-push, and leaves the Rust land checks green.

1. Retrieval trigger. Delete `brief_coverage` and its constants and test from `crates/north-ops/src/ops/flow/`,
   the `retrieval trigger` line from `announce`, and the "CLI and arms are preserved" sentence from
   feedback-loop.md. Done when: `rg -n 'retrieval trigger|brief_coverage' crates docs/feedback-loop.md` prints
   nothing on origin/main.
2. Advisor report. Delete `crates/north-ops/src/ops/advisor_report/`, its `tools.rs` and `help.txt` lines and
   the `noninferiority_margin` and `plan_per_list_claude` entries; policy-replay imports
   `crate::platform::pystats::float_sum`. Done when: `north advisor-report` exits as an unknown command and
   `rg -n 'advisor_report|noninferiority_margin|plan_per_list_claude' crates src harness` prints nothing.
3. Advisor arm in policy-replay. Delete the `--advisor` flag, `AdvisorUsage` and its parsing, the advisor row
   fields and scoring, their tests and the advisor fixtures; set `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1` on every
   trial. Done when: `rg -n -i advisor crates/north-ops/src/ops/policy_replay tests/it` prints only that env line.
4. Planner band. Delete `crates/north-core/src/ledger/planner/`, `north ledger planner-band`,
   `recommend --planner` in Rust and in `bin/threads`, and their checks in `bin/threads.test.sh`; drop the
   trade-off paragraph from tier-routing.md. Done when: `rg -n 'planner-band|--planner|ledger::planner' crates src bin`
   prints nothing.
5. Handoff Done-rate term. `Arms::winner` keeps handoff-150k on a tier only when its cost per closed box is lower
   and its Done rate is no more than 10 points below no-handoff, with a test for each side. Done when: that test
   passes on origin/main and `north dispatch --dry-run` on Nexus prints the winner per tier with both rates.
6. Handoff nudge, not before 2026-11-18 and only when no tier keeps handoff-150k under box 5. Delete
   `crates/north-core/src/fleet/handoff.rs`, the nudge in `crates/north-core/src/fleet/watch.rs`, the arm pick in
   `crates/north-dispatch/src/dispatch/handoff.rs` and `mod.rs`, the session threshold and `handoff_arm_ctx`.
   Done when: `rg -n 'handoff_arm_ctx|handoff-150k' crates harness` prints nothing; when a tier keeps it,
   this page records the reading instead.
7. Restaff arms. Delete the arm sentence from `harness/skills/workers/references/tiers.md`,
   `crates/north-ops/src/ops/report/arms.rs` and its report call, and the arm read in
   `crates/north-core/src/threads/calibration/routing.rs:95`. Done when:
   `rg -n 'cheap-fix|Arm: escalate|Arm: split' crates harness` prints nothing.
8. Replay-staffing. Delete `crates/north-core/src/threads/calibration/replay.rs`, its verb in Rust and in
   `bin/threads`, and its golden check. Done when: `north threads replay-staffing` exits as an unknown verb and
   `rg -n replay-staffing crates bin` prints nothing.
9. Feedback-loop rows. Mark `capacity_runs.command, cpuSeconds` read with their readers, drop the Still unread
   section and the north#35 lane D pointer. Done when: `rg -n 'held for north#17|lane D' docs/feedback-loop.md`
   prints nothing.
10. Priors listing. `north defaults-measure --priors` lists every entry with `settles_when`, whatever its source
    prefix. Done when: its output names `explore_every` and a test covers a non-`prior:` entry.
