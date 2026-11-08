# T3 Code, second read: Gemini's review checked against main (2026-11-08)

The owner asked on 2026-11-08 for Gemini's adversarial review of T3 Code against north (south
`agents/decisions/harness/t3code-vs-north-plan-2026-11-08.md` at south 19a7ffb, written against north 46ce4cc10c) to be
checked claim by claim and folded into the roadmap. This note records every factual claim marked confirmed, stale or
wrong with its evidence, the review's errors in both directions, and the roadmap order that came out of it. It follows the
[first T3 Code cycle](2026-11-07-t3code.md) and the method in
[watching.md](../../../harness/skills/agent-policy/references/watching.md). T3 Code was read as data at `32a0531` (MIT,
T3 Tools Inc.); no code was copied.

Evidence is north origin/main at 8a169ecc28 and live readings on the laptop between about 5:55 and 6:30 AM +07.

## Summary

- 38 claims checked: 16 confirmed, 5 confirmed with a correction, 7 stale (fixed on main in the 30 hours before the
  review was written or since), 7 wrong and 3 ungrounded.
- The review's evidence about north's unfinished first wave is mostly right. Its framing is not: the five issues it calls
  "closed before their guarantees held" were closed on purpose in the 2026-11-08 issue cut under north#719's spine
  decision, each with a stated reason. Only north#716's ticked box was false (it cites a reverted commit), and the
  wip-branches triage reopened it before this read.
- Its plan overstates T3 Code where north already holds the same mechanism (exact-tree verdict reuse, capacity admission
  classes, typed message senders, a regenerated features index) and where the remedy is weaker than north's own direction
  (a typed report tool in place of facts derived from records).
- Its payback hours rest on no measurement. North cannot price guard loss in hours either: `north hook stats` "lost(med)"
  is the span of a repeated refusal chain in one session, not the time one refusal costs.
- It is right, and is taken further than it asked, on: the unmeasured `other` refusals, the 74 verdict reads of agent
  text, the outbox for side effects across restarts (now measured), replay of the fleet prompt and of control cases, a
  check for the code-comment rule, and the landing-to-live bar that no issue owned.
- Phase 6, the phone and web deck, is a direction call. The owner chose on 2026-11-08 to keep north terminal-only
  (north#866, closed).

## Claims

Verdicts: **confirmed**; **confirmed, corrected** (true with a named error); **stale** (true when the source was
measured, not now); **wrong** (false, or a remedy the evidence does not support); **ungrounded** (no measurement behind
it).

### North's first wave (review section 1.1)

| # | Claim | Verdict | Evidence |
| --- | --- | --- | --- |
| 1 | An issue cut closed north#709, #714, #715, #716 and #730 before their guarantees held | wrong | Each closing comment (2026-11-07 18:30 to 18:33 UTC) names the 2026-11-08 issue cut under north#719 ("cut north to its spine", a call the owner handed the proxy on 2026-11-07) and its reason: #709's views and #716's repository-wide rule cut as outside the spine, #714's runner built with only a dated verdict left, #715 merged into north#706 as a box, #730 cut for spending on a measurement no decision waited on. Only #716's box "A north check covers Rust test code" is false |
| 2 | 2c86fcbe92 landed test-nondeterminism with 250 sites in 86 files, and 24788628d2 reverted it 50 minutes later when features-index-current went red; nobody re-landed it | confirmed, corrected | 22:05 to 22:53 UTC on 2026-11-06, 48 minutes; the revert body says `Goal-red: features-index-current`. `git grep -i nondetermin origin/main -- crates src structure.toml` finds no rule. North#716 reopened at 23:00 UTC 2026-11-07 with a re-land box |
| 3 | Hermetic test determinism is not on main at all | wrong | e8610bb5a5 (2026-11-07 14:12 UTC) refuses a test file that adds `sleep(`, `Instant::now(` or `.elapsed()` (crates/north-land-scans/src/wall_clock.rs). The host-state half is what is missing |
| 4 | `harness/cases/` does not exist | confirmed | `git ls-tree -d origin/main harness/cases` prints nothing |
| 5 | detector-proof stays in shadow | confirmed, corrected | harness/gates.toml `[gate.detector-proof] mode = "shadow"`. It is north#706's gate for detector-filed boxes, not the replay case gate, and the laptop holds no `detector-proof.jsonl`, so it has no shadow decisions to promote on |
| 6 | `Kind` is printed but never reaches `replay_verdict::decide` | confirmed | crates/north-convo/src/policy_replay/case/mod.rs:244 calls `decide(base, lane, deterministic)`; the kind appears only in output (lines 104, 127, 249) |
| 7 | `policy-replay case` cannot test the fleet prompt, briefs, Codex runs or the hooks | confirmed, corrected | The fleet prompt and briefs: true, and the first cycle recorded it. Codex: outside the Claude-only standing order. Hooks: true but beside the point, since hooks are deterministic and their changes replay through `north hook` over crates/north-guard/src/guard/hook_refusal_samples.toml |
| 8 | 9,049 refusals in 7 days; 28 of 39 rules over 10% false | confirmed | docs/rationale/guard-false-refusals.md at 2aa785df4b |
| 9 | 11 rules narrowed, 17 over 10% left un-narrowed | stale | `north hook stats --since 7d` now prints a `narrowed` line for 27 of the 28; only tripwire.delete reads "not narrowed" (a north#706 box). No guard has been re-sampled since its narrowing, so none is shown under 10% yet |
| 10 | The rates listed (issue-cleanup 80%, clipboard 80%, lands-before 71%, resource-safe-search 67% "across 648 fires", privacy-judge 20% "across 650 fires" and others) | confirmed, corrected | Rates match the pre-narrowing samples. Fire counts are wrong: resource-safe-search 321, privacy-judge 30 (privacy-judge.pending 176) over 7 days |
| 11 | 1,433 unmeasured `other` refusals | confirmed | 1,439 over 7 days; over 48 h, 804 refusals and 607 advisories in 512 sessions, 10% of all 7,985 refusals |
| 12 | The frontier benchmark has 0 paired tasks; results.jsonl holds 1 row (t3code on north-685) | confirmed | `git show origin/main:docs/rationale/frontier/results.jsonl \| wc -l` prints 1 |
| 13 | t3code.rs cannot pass reasoning effort | confirmed | crates/north-frontier/src/t3code.rs:5. T3 Code's own model-selection contract carries an `effort` option (`packages/contracts/src/modelSelection.test.ts`), so a thread started through its server API may carry it; sent to frontier-run |
| 14 | The T3 arm was stripped of repository instructions and skills until north#855 | confirmed | north#855, section "Fair arms" |
| 15 | north#709 landed 2 of 25 lanes and 0 of 6 TUI lanes | confirmed, corrected | 7768678ab6 and e518c114bb are on main; the other lanes were cut as outside the spine (#709's closing comment), not abandoned |

### Mechanism claims (review section 1.2)

| # | Claim | Verdict | Evidence |
| --- | --- | --- | --- |
| 16 | 74 prose-verdict regexes in harness/agent-text-reads.toml | confirmed | 74 `kind = "verdict"` lines in 13 files (outcome.rs 29, attribution.rs 15), plus 8 hint lines in 6 files |
| 17 | A typed report tool would take those 74 to 0 | wrong | North#791's replay of 2026-11-07 found 13 of the 14 Stop lines that had a record disagreeing with it. A typed agent claim is still a claim; the fact has to come from the record (landed sha, queue verdict, admission ticket, registered wait). The intents already have typed verbs: `north gate open`, `north fleet need`, registered waits |
| 18 | `north fleet send` is unsigned and can pose as the owner (proxy-role.md:93) | stale | `Sender` (crates/north-fleet-store/src/sender.rs) is required, has no default, and `--from tom` is refused from the CLI. A session's identity is still read from its environment, not a capability; no spoofed message has been recorded since |
| 19 | The reports mirror hit 38.5 MB | stale | 5c282bbe5f (2026-11-08 04:56 +07) mirrors only the entries dispatch reads |
| 20 | fleet-tick timed out 38 times | stale | a73b350e3e is live from 4:18 AM +07; the laptop journal's last "timed out" line is at 04:18:13 |
| 21 | Landings rerun tests the lane already passed | wrong | The lander reuses a verdict keyed to the exact tree (crates/north-base/src/verdict_cache/); the first cycle dropped T3 Code's cache as convergent for this reason |
| 22 | Lost alerts, duplicate staffing and torn restarts | confirmed | Four fixes in 72 h, each in its own module: 163a23c725 (an offload offer also started after timing out), 97e4aa6827 (two launches of one name), 22571229b6 (an after-landing run lost to a lander restart), 363349d345 (7 workers' watchdogs reaped with the launcher's cgroup) |
| 23 | `.git/config.lock` failures during concurrent worktree adds | ungrounded | No north commit in 14 days names a config.lock failure |

### Baseline (review section 2)

| # | Claim | Verdict | Evidence |
| --- | --- | --- | --- |
| 24 | 50 crates, 1,958 .rs files, 253,300 lines; structure.toml 911 lines, .safe-push 57 | confirmed | At 8a169ecc28: 49 crates plus the root package, 2,034 .rs files, 263,236 lines; 911 and 57 lines |
| 25 | 1,903 commits in 7 days, about 272 a day | confirmed | `git rev-list --count --since='7 days ago' origin/main` prints 1,913 |
| 26 | 1,596 worker runs at 55% first-try success over 7 days | stale | Not reproduced; `north ledger scoreboard` reads first-try success 81% on 24 h and 84% on 6 h |
| 27 | 4 of 13 bars fail: queue p90 30 min, verification p50 71 s, harness loss 3%, live everywhere 49 min (1 h) | stale | About 6:10 AM: 3 of 13 fail on 1 h. Queue p90 28 min, verification p50 73 s, live everywhere 26 min (9.7 h on 24 h). Harness loss 0% on 1 h, 3% on 3 h, 7% on 24 h |
| 28 | The proxy ran 2,259 Bash calls against 282 delegations | confirmed | proxy-role.md, a record of 2026-11-04 to 05; not a current measure |
| 29 | docs/features.md is touched by 23% of commits, and its conflicts reverted #716 | stale | 18% (351 of 1,913 over 7 days). Five landings were reverted for features-index-current in the week to 2026-11-07 08:27 +07; a6e4ee1295 (08:38 +07) regenerates the index at enqueue and in the candidate, and none has been reverted since |

### T3 Code facts

| # | Claim | Verdict | Evidence |
| --- | --- | --- | --- |
| 30 | 33 build-time oxlint rules | confirmed | 33 rule files in `oxlint-plugin-t3code/rules/` at 32a0531 |
| 31 | About 60.5 KB of instructions and tool schemas at startup | confirmed | 60,581 bytes in its budget file (first cycle) |
| 32 | Codex `additionalContext` is chunked under 4,000 bytes | confirmed | `CodexDeveloperInstructions.ts:10-11`. Not taken: Claude only by standing order |
| 33 | About 181 eval cases in 5 suites | confirmed | First cycle: about 180 cases, five evals |

### The plan (review sections 5 and 6)

| # | Claim | Verdict | Evidence |
| --- | --- | --- | --- |
| 34 | Payback of 2.5 to 5 days per phase from 1.2 to 1.5 minutes saved per worker run | ungrounded | No measurement is cited for the per-run saving. Guard loss is only countable in refusals today (see Summary) |
| 35 | A typed control plane removes thousands of false refusals on north, gh and git commands | wrong | The guards a typed north control plane would replace (worker-tier.\*, issue-\*, privacy-judge, public-text, proxy-role, tripwire.push) refused about 1,000 of the week's calls, most now narrowed. The large classes (claude-sleep, blind-stage, launch-critical, worker-wait, session-kill, modern-search) judge general shell commands no control-plane tool replaces |
| 36 | Static repo-shape checks should move from PreBash hooks into code-health | wrong in general, right once | North's PreBash guards judge commands, not repository shape. The one static rule with a measured miss is code comments: main's `//` lines rose from 128 (2026-11-06 count) to 196, and 80 of 868 Rust commits since add comment lines net |
| 37 | Fast runtime promotion is north#719 | wrong | North#719 is the spine cut. The landing-to-live goal came from north#731, was merged into #719 and struck on 2026-11-08 as watched by goals.toml, and no open issue held a box for it |
| 38 | Phase 5's TUI views pay back in the owner's triage time | ungrounded | No measure of that time exists, and the views were cut as outside the spine |

## Where the review is right, taken further

- **The `other` refusals.** 804 refusals in 48 h with no false share, the largest unmeasured refusing class. North#706
  gains a box that names its guards and narrows each over 10% false, closed when `other` is under 5% of refusals.
- **Verdicts read from agent text.** North#791's own box for this was removed from north#706 on 2026-11-08 as covered
  by the record-only attribution box, but the 74 lines are still there. North#706 gains a box, not before
  2026-11-09T13:00Z (after #791's 24 h forward window lists the gaps), taking the verdict lines from 74 to at most 10,
  each replaced by the record that already holds the fact. That is more than the review asked for and without its new
  agent-written channel.
- **The outbox.** The first cycle dropped T3 Code's transactional outbox until a duplicate side effect was measured.
  Claim 22 measures four. North#862 adds `Store::transition_with_effects`, moves the three machines with incidents onto
  it, and keeps it only if the class of fix halves in 14 days.
- **Replay of the fleet prompt and of control cases.** North#864 makes the verdict kind-aware (a guard or control case
  refuses a lane that overcorrects), carries each arm's fleet prompt and brief into the fork, and seeds 12 cases from the
  8 classes that reproduced in E1 plus 4 controls. It waits on north#855's headline. Left out on purpose: API-envelope
  capture (the fork already replays the real request), hooks inside a replay, Codex, and the review's 20 cases.
- **The comment rule.** North#865 adds a code-health report for an added comment that names no determinism, engine,
  Lua or external-format constraint, shadow first.
- **Landing to live everywhere.** North#863 owns the bar furthest from its target, diagnosis first. Pushing prebuilt
  runtimes, the review's fix, is one candidate.
- **North#716.** Folded into the reopened issue as a comment: the revert is confirmed, e8610bb5a5 covers the clock half,
  the index cause is fixed, the newest flaky lander test (8a169ecc28, a file race between concurrent probes) is a cause
  neither half catches, and TypeScript and shell tests stay out for want of evidence.

## Dropped, with the reason

- A per-session MCP server with six typed tools (Phase 2): the facts it would carry already have records, and the
  intents already have CLI verbs. The transport adds a server and schema tokens and removes no measured loss on its own.
- Capability tokens and signed principals: typed senders landed (claim 18) and no spoof has been recorded.
- Request-id idempotency: 3 duplicate runs in 24 h in `north ledger flow`, and a second launch of one name is refused
  atomically since 97e4aa6827.
- Per-turn git checkpoints, a lock on `.git/config`, context-transfer packets and the rewind key (Phases 3 and 5): no
  measured loss.
- A provider replay harness: the outbox's fault-injection test in north#862 covers the restart case; Codex is out.
- Exact-tree check cache with hit audits and FIFO weighted admission (Phase 4): convergent with the lander's reuse
  store, and with capacity admission that ranks land checks above worker builds (a54bd7180d) and admits probe worktrees
  as critical (5d81953daa).
- Sequence-cursor delta sync and a 1 MiB snapshot budget: the measured losses were fixed (claims 19 and 20).
- Codex instruction chunking: Claude only.
- `require-suppression-reason`: north already requires a reason on each `[[implicit_state.allow]]` and
  `Code-health-allow:` exception.
- Supervisor TUI views (Phase 5): cut as outside the spine; the owner's 2026-11-08 order puts first-try success, harness
  loss and landing p90 above all other north work.
- The phone and web deck (Phase 6): the owner chose on 2026-11-08 to keep north terminal-only for now (north#866).

## Roadmap order

Ordered by measured loss removed per hour of work, under the owner's 2026-11-08 order. Work that only measures comes
after north#855's headline, so the bake-off can inform it.

1. north#842: landing queue p90, 28 min on 1 h against 60 s; 21% of north's landings in 24 h were refused and 115
   re-lands ran across 81 lanes. The probe loop has served north since 5:18 AM +07.
2. north#634: verification p50, 73 s on 1 h against 60 s.
3. north#706, the `other` box: 804 unmeasured refusals in 48 h; labelling a sample costs little.
4. north#863: landing to live everywhere p90, 26 min on 1 h and 9.7 h on 24 h against 2 min, the bar furthest from its
   target.
5. north#706, the harness-loss boxes: the 24 h pareto reads capacity 9 runs, placement 10, landing refusal 4,
   duplicate 3.
6. north#716: the host-state fixture rule; the branch is written, and a flaky verdict held the lander link for 39
   minutes on 2026-11-07 (e8d564bd60).
7. north#855: the headline, north against plain Claude Code and an equipped T3 Code arm on the 10 frozen tasks.
8. north#706, the verdict-reads box (not before 2026-11-09T13:00Z): makes the first-try bars come from records.
9. north#862: the lifecycle outbox, four torn-effect fixes in 72 h.
10. north#864: kind-aware replay of the fleet prompt and control cases; blocked by north#855.
11. north#865: the code-comment rule.

## Method notes

Net comment lines per commit, as used for claim 36 and north#865:

```sh
git log origin/main --since='2 days ago' --no-merges -p --format='@@COMMIT %h' -- 'crates/*.rs' 'src/*.rs' \
  | awk '/^@@COMMIT/{c=$2} /^\+[ \t]*\/\/[^\/!]/{a[c]++} /^-[ \t]*\/\/[^\/!]/{r[c]++}
         END{for (x in a) {d=a[x]-r[x]; if (d>0) {n++; s+=d}} print n" commits, "s" net lines"}'
```

Guard counts come from `north hook stats --since 7d` and `--since 48h`; bars from `north ledger scoreboard`; the
harness-loss pareto and landing refusals from `north ledger flow`. The measured basis of the Phase 6 call is in south agents/decisions/harness/t3code-gemini-phase6-2026-11-08.md.
