# The coordinator's role: route and judge, delegate the rest, and measure it

Tom asked on 2026-11-05 for the orchestrating session to act as one: do brief cross-cutting work itself
and delegate everything that takes several steps, with a deterministic mechanism behind each rule
wherever one can carry it. This page measures what the coordinator did in one day, then designs the role so
that code, hooks, gates and alarmed metrics hold it, and prose only where nothing can check the rule.
Each design part ends with an **Enforced by** line.

## 0. The org chart

North has one primary coordinator, the coordinator, accountable to Tom, who sets goals and steers; every
other session reports up a chain that ends at it, a lead's or worker's `--parent` and then that
session's parent. Anything the coordinator cannot answer at once — an investigation, a debug, engineering,
a deep analysis — goes to a worker or a lead, and whatever can run in the background does, so a
report to the coordinator never waits behind the coordinator's own work (Tom's direction, 2026-11-11). The guard
carries the last line: `crates/north-guard/src/guard/coordinator_background.rs` refuses a coordinator foreground
call that can wait past 30 s on a launch, a remote command or a job, and names `run_in_background: true`.

## 1. Evidence: one day of the coordinator

Source: the terminal coordinator's transcript, conversation `895929a7-6218-4168-b513-3dced3440bf6` (on the
laptop), copied on 2026-11-05. It covers about 24 hours from 2026-11-04 to
2026-11-05: 5,880 Opus 5.5 turns, 2,753 tool calls, 116 messages typed into the session and
11 context compactions. Its turns read 1,549 M cache tokens, about 263 k per turn, so every call the
coordinator makes re-reads a quarter of a million Opus tokens of context.

Headline: **2,259 Bash calls against 220 Agent delegations** (the brief counted 2,241 Bash at its time
of writing; the session kept running). With fleet launches added, 282 calls of 2,753 handed work away.

Method: a jq extract of every `tool_use` with its full input, classified by an ordered first-match
rule list over the tool name and the command text (the same classifier box P2 makes code). Kinds, in
the order the rules try them:

- delegate: `Agent`, `north fleet launch|revive`, `north fleet dispatch`, `threads queue follow`, `north cloud launch`
- child message: `SendMessage`, `north fleet send|ask` (a no-op `ListAgents` is monitoring, so it neither counts nor resets a streak)
- Tom-facing: `north gate`, `date '+%I:%M %p'` (clock times for Tom)
- briefing: `north work brief`, a brief file or heredoc starting `Item:`
- monitoring scripts: `Monitor`, `TaskStop`, `ScheduleWakeup`, `CronCreate`, shell loops and sleeps, `nohup`/`setsid`, writing a `.sh` or `.ts` helper
- bookkeeping: `gh issue edit|close|comment`, `threads tick|close|claim|release`, `north fleet stop|forget`, `git merge-base --is-ancestor`, worktree and branch removal, notes appended to the scratchpad
- hands-on landing and editing: `Edit`, `Write`, `safe-push --to`, `north land enqueue`, `git commit|push|rebase|add`
- landing logs: safe-push and land-check logs, the land queue directory, `journalctl -u north-land*`
- status checks: `north fleet show|list|tree`, `journalctl`, `north capacity`, `north ledger flow`, `ps`, `gh run`, usage and pace
- diagnosis: everything else (file and log reads, `rg`, `sed -n`, `git log|show|diff`, `ssh` to a host, `sqlite3`)

Counts (2,753 calls):

- diagnosis: 1,140 (file and log reads 690, git history 179, issue reads 41, ssh to a host 34, other 196)
- landing logs: 327 (the land queue directory 101 times, the lander's journal 57 times)
- delegate: 282 (220 `Agent`, 60 `north fleet launch`, 2 other)
- hands-on landing and editing: 220 (132 `safe-push --to`, 14 `north land enqueue`, 28 `Edit`/`Write`); separately 94 calls ran `cargo` and 30 edited lane files in place with `sed -i`, `perl -pi` or `sd`
- status checks: 214 (`north fleet show` 75 times)
- monitoring scripts: 182 (46 `Monitor`, 42 `TaskStop`)
- child message: 152
- bookkeeping: 122 (47 landed-ancestor checks, 23 `north fleet forget`, 20 `north fleet stop`, 16 issue edits, closes and comments, 27 appends to a scratchpad `queue.md`)
- Tom-facing: 99
- briefing: 15

Streaks: between two delegations, child messages, briefs or Tom-facing calls the coordinator ran 281 streaks
of other calls, median 5, 90th percentile 18, longest 64. A warning at 5 consecutive calls would have
fired 145 times; 42 streaks reached 15. 646 calls ran inside 112 distinct lane worktrees, the lanes
other sessions own.

Work that should have been delegated, by the calls it took:

- Landing other sessions' lanes by hand: `lander-l0` (27 calls), `landing-train-checks` (24),
  `lander-remote` (18), `runtime-reuse-land` (16), `spec-batch-race` (15), `landing-path` (13): rebase,
  rebuild, `safe-push --to main`, read the refusal, fix, retry. A worker owns its lane through landing;
  a refused lane goes back to its session (the lander already knows which one).
- Writing a South review: `south/worktrees/harness-review-1104` (25 calls) is one worker box.
- Fixing code in lanes: `grounding-hook-lean` (19), `retire-review-gate` (18), `ci-budget-wording` (16),
  `spawn-guard-rust` (14), `battery-shed` (14), `teardown-wait` (11), plus 30 in-place edits and 94
  `cargo` runs.
- Reading lander logs to learn why a lane was refused: 327 calls. The lander's verdict line and the
  conflicting files already exist in its journal (for example `etxtbsy-helper ... conflict
  tests/it/land_serve.rs` at 2:54 AM); they belong in a message to the lane's session, not in the
  coordinator's context.
- Polling status: 214 calls, 75 of them `north fleet show NAME`. Receipts deliver a child's report to
  its parent, but the coordinator is no fleet session: 54 of its 60 fleet launches passed `--parent none`,
  so no report, landing or silence notice could reach it and it polled instead. Its 220 `Agent`
  delegations (112 `worker-sonnet`, 80 `worker`, 20 `worker-haiku`, 8 `planner`) were in-process
  subagents, whose results return into the coordinator's own context.
- Post-landing bookkeeping: 122 calls verifying ancestry, stopping and forgetting sessions, ticking and
  closing issues and keeping a scratchpad queue. The landing knows every fact these calls re-derive.
- Monitoring scripts: 182 calls standing up and tearing down watchers for runs that the fleet's waits
  and receipts already report.

The root cause under all ten kinds is one gap: **the harness cannot name the coordinator.** The coordinator runs
outside the fleet with `NORTH_FLEET_SESSION` unset, so it cannot be a parent, a lander ticket's
session or an alarm's recipient. Its children report to nobody (`--parent none`) or to the idle
fleet session that is also called `coordinator`: launches with no `--parent` and no caller
default to that name (`src/cli/fleet/lineage.rs:19`), and `north ops check`, `north ops dividend` and
`north ledger flow` alerts all send to it (`crates/north-ops/src/ops/check/mod.rs:45`,
`crates/north-flow/src/flow/alerts.rs:68`). So the session Tom reads hears nothing, and the
session that hears everything has no reader. The lander already tells a ticket's session when its
lane lands or is refused, with the conflicting files (`crates/north-landing/src/land/bisect.rs:126`),
but a lane the coordinator pushed itself, or one an in-process subagent pushed, has no session it can reach,
so the coordinator read the journal instead.

What Tom does in that terminal besides typing, from the same transcript: one each of `/model`, `/effort` and `/goal`, one pasted image and one `!` shell line. 6,576 of the transcript's 8,578 working-directory entries are `~/code/north/main`, whose project memory and project instructions load with it. In-process subagent calls do not appear in it (every turn has `isSidechain: false`), so subagent work is invisible to this count.

The counts reproduce on the laptop through box P2's readout (`north coordinator calls TRANSCRIPT`); the transcript itself stays private and is not committed.

The same inference by absence recurs one level down, at the sender: every `north fleet send` from
outside a session is unsigned (`src/cli/send.rs:24`), and so are fleet receipts and silence notices
(`crates/north-core/src/fleet/registry/parents.rs:48`), the lander's messages
(`crates/north-landing/src/land/after/mod.rs:47`), wait results (`src/cli/fleet/waits.rs:76`), about
ten timers (`crates/north-ops/src/ops/check/facts.rs:90`, `ops/dividend/live.rs:213`,
`ops/runtime/update.rs:416`, `ops/tune/mod.rs:257`, `ops/timers/fleet_session.rs:95`,
`ops/timers/cross_project.rs:175`, `health/failed_starts.rs:115`, `ops/flow/alerts.rs:188`) and the
TUI composer Tom types into (`crates/north-tui/src/tui/fleet/desk.rs:54`). Nothing can tell Tom's
message from an alarm. The design fixes both absences with values the compiler checks.

## 2. Design

### 2.1 The sender is a required value

`send_signed` and the fleet row's sender take `Sender`, an enum of `Tom`, `Session(name)` and
`Service(unit)`, with no default. The TUI composer and `north coordinator` pass `Tom`; the lander, receipts,
waits and every timer pass `Service` with their own unit name; a fleet session passes its name. A
message from `Tom` reaches the session unprefixed, any other with `[from NAME]`, and the session's
hooks see the sender as a field of the delivered message, not as the absence of a prefix.

Enforced by: types. A new timer cannot send without naming itself, so it cannot pose as Tom.

### 2.2 The session's role is set by the launcher

- `SessionRecord` (`crates/north-core/src/fleet/model.rs:135`) gains `role: Role` with `Coordinator`,
  `Lead` and `Worker`. `north fleet launch --role coordinator` sets it; a launch with a task (`--task` or an
  `Item:` line) is `Worker`, any other `Lead`. The launcher exports `NORTH_FLEET_ROLE` beside
  `NORTH_FLEET_SESSION` (`crates/north-providers/src/providers/claude/session_env.rs:30`).
- At most one live session has role `Coordinator`, named `coordinator`, the constant receipts, alarms and lineage
  already use (`crates/north-core/src/fleet/runtime.rs:94`). A second coordinator launch is refused unless it
  passes `--take-over`. For a coordinator, take-over archives the old record and hands over one digest line
  (how many messages, from which senders, the oldest age) instead of replaying the backlog into the new
  coordinator's first turn; the live `coordinator` record and the parked `coordinator-gex45` record are
  both archived this way by the first `north coordinator`.
- The coordinator moves to another account through the same take-over when its window runs out, the
  handover the provider-reserve plan already designs for work in flight (north `d7509e4ea`). While
  idle it parks like any session and the next message revives it.
- Every place that infers the coordinator from an absence reads the field:
  `crates/north-providers/src/providers/claude/runs/mod.rs:147` and `crates/north-providers/src/providers/ledger_rows.rs:89`
  (no parent and no children means coordinator), the lead-timer guard (removed in north#1650) (a name
  containing `lead`), `harness/hooks/prompt-grounding.sh:29` (absent from fleet records).

Enforced by: code. Membership is set by the launcher, never remembered; the single-coordinator rule is a
registry refusal with a test.

### 2.3 The session Tom types into is that fleet session

`north coordinator` launches or revives the fleet session `coordinator` with `--role coordinator --parent none`, in
`~/code/north/main` so the project memory and instructions the coordinator has today keep loading, on the
account `agents plan` routes it to. It then opens north's fleet TUI focused on it; the TUI already
sends from a composer and renders the transcript (`crates/north-tui/src/tui/fleet/screen.rs:70`,
`:378`). The launcher passes `--agent coordinator` (Claude Code 2.1.318 has the flag), so the role file is
the agent definition from the first turn. Firn's entry for Tom's coordinator window runs `north coordinator`.

What Tom loses: a fleet session runs `claude -p --input-format stream-json`
(`crates/north-providers/src/providers/claude/fleet.rs:50`), which has no slash commands beyond the wired
`/compact`, no image paste and no `!` shell. In the measured day he used each once (`/model`,
`/effort`, `/goal`, one image, one shell line). `north coordinator` covers model and effort with a TUI slash
command that relaunches the coordinator through take-over. The F1 test box (2026-11-05) found image paste
works in the TUI (Ctrl+V from the Wayland clipboard), as do Esc interrupt and a 3,426-word reply;
`!` is not covered. Voice dictation does not reach the TUI: Tom dictates with Claude Code's built-in
voice mode (`voice.enabled`, `mode: hold`, sox `rec` as a child of the interactive `claude`), which
records and transcribes inside Claude Code's own UI, and no desktop-wide dictation service exists to
type into the focused window. A hold-to-talk local Whisper module was chosen instead, filed
as box F1v on north#112. Box F1 waits on Tom's answer and on that box.

The rule that refused a second coordinator while Tom's own Claude window had written in the last 30 minutes (`src/cli/fleet/one_coordinator.rs`, 2026-11-06) is removed with the window search it needed (north#1048, 2026-11-13).

Rejected, two ways to keep the interactive Claude Code window as the coordinator:

- Adopting it into the fleet through a SessionStart hook and `CLAUDE_ENV_FILE`. Messages would reach
  it only when a hook fires, so an idle window would hear nothing until Tom typed, the failure being
  fixed, and it too is a vendor-UI workaround.
- Pushing north's messages through Claude Code's own cross-session inbox
  (`/run/user/1000/cc-socks/<pid>.sock`, which `SendMessage` uses). It does reach an idle
  interactive session: at 3:58 AM on 2026-11-05 (laptop time) a peer's message woke the idle coordinator
  terminal. It is rejected because north would still not own the runtime: no failover to another
  account when one runs out, no parking and no sender on messages (Claude Code already resumes on
  its own after a usage limit, as Tom noted on 2026-11-05); because the socket protocol is
  internal to Claude Code and may change under any update; because
  [read-isolation.md](read-isolation.md) already denies workers those sockets as a boundary, and a
  north writer into them would cross it from the other side; and because it is the vendor-UI
  workaround the standing order rules out.

Revisited 2026-11-06 (north#636): after the fleet coordinator ran out of its window and Tom took the role
over in his own window, every report, red-main alarm and credential notice sent to `coordinator` kept
reviving the idle fleet session, because no take-over path existed for a window north did not start.
The issue plan now owns this call and orders the inbox route for that case alone: `north fleet adopt
--name NAME` binds the name to the window, and north writes the two JSON lines Claude Code itself
logs as its injection form (an `auth` line with the peer token from `sessions/PID.PEER.key`, then a
`user` line) into the socket `sessions/PID.json` names. The objections above still hold for it: an
adopted window gets no account failover from the fleet (`north coordinator move` moves it), a protocol
change breaks only delivery to it and shows as a failed send naming the window, and the north binary
reading peer keys is the boundary crossing named above, accepted for this one sender path.

Revisited 2026-11-14 (north#1281, gap 1): the fleet screen may now hold the coordinator's conversation
where no client window can open. Release condition 2 of north#1281 asks that the operator's daily
loop can begin in the screen on a host with no desktop, and the window rule of north#1048 refused
every start of the coordinator there. `north coordinator` decides when the operator runs it on a coordinator that is
not live: where the fleet's desk names why no window can open, it prints that reason and starts the
coordinator held by the screen, an ordinary fleet host the screen types into, and every start the operator
did not type keeps the surface on the record, while a revive by `north coordinator` on a desktop takes a
coordinator the screen held back to a window. The plan's three questions were answered by the owner
on 2026-11-14. The coordinator holds the same commands in the screen as in the window, so a session in the
coordinator role is refused `north runtime hold`, which the window, running in no session's scope,
cannot reach either. It runs under the same user and sandbox as the window does today, neither wider
nor narrower. Any host where the operator's own user runs `north coordinator` may hold it, with no new list
of hosts. What stays: on a desktop the window is the path, with every refusal of the window rule
unchanged, and there is still no second coordinator, since the screen only sends to the host and never
takes its conversation.

Receipt volume: once the coordinator has a reader it is the default parent of every launch from outside the
fleet (`src/cli/fleet/lineage.rs:19`), and each message wakes a turn at about 263 k context. Messages
to the coordinator from `Service` and `Session` senders coalesce into one digest delivered at most every 10
minutes (`coordinator.digest_minutes`), except urgent ones: red main, a gate opened, a decision stop and an
alarm. Messages from `Tom` are never held.

Enforced by: code (`north coordinator`, `--agent`, the digest), plus Firn config for the entry point.

### 2.4 The role file

`harness/claude/agents/coordinator.md`, beside `worker.md` and `planner.md`. Frontmatter: `model: fable`,
`effort: high`, and a `tools` list whose only `Agent` entry is `Agent(Explore, Plan)`, which Claude
Code enforces as an allowlist of in-process subagent types ("a deny rule, the Agent(x,y) allowlist or
an agent.offer hook excludes it", from the 2.1.318 binary). In-process subagents are read-only; every
worker is a fleet session the lander can tell. The body holds only what no mechanism checks:

- You are Tom's coordinator and route and judge: turn what Tom asks into briefs, staff them, read
  their reports against what he meant, and tell him results and decisions.
- What stays with you: judging a result against Tom's intent (a `Done:` that does not do what he asked
  is not done); calls that touch many lanes at once (which overlapping lane waits, a cross-repo order, a
  revert across lanes); routine decisions under the standing orders; real decisions to Tom through
  `north gate`.
- Do a task yourself only when it takes about 3 tool calls or its answer is predictable; anything
  longer is a brief.
- The names (from `know-fleet-names`): the fleet is Tom's set of sessions coordinated by
  the coordinator; North is its harness; the coordinator is Tom's primary assistant, and voice transcription may write "Play-Doh";
  Nexus is the always-on server.

`crates/north-core/src/fleet/prompt.rs` renders a role-specific preamble: the coordinator keeps the standing
orders and drops the worker line "Tom and the coordinator reach you only through fleet messages".
`harness/hooks/prompt-grounding.sh` fires for role `Coordinator` on messages whose sender is `Tom`, instead
of on the absence of a fleet record, so 2.2 does not switch grounding off.

Enforced by: Claude Code for the subagent allowlist; text for the judgment lines, because whether a
result matches Tom's intent or an answer was predictable cannot be computed from a transcript; `north
agent-config-check` checks that the file is installed and holds the text of every rule placed
`agent:coordinator` (section 3).

### 2.5 The delegation rule, counted by a hook

**Kinds come from the verb.** Each `north` verb declares its kind (delegate, brief, child message,
Tom-facing, bookkeeping, landing, status, diagnosis) in the CLI dispatch table, and a test fails any
verb without one, so a new delegating verb cannot fall through to diagnosis. The classifier,
`crates/north-core/src/coordinator/calls.rs`, asks that table for `north ...` commands and keeps an ordered
rule list only for foreign commands (`git`, `rg`, `gh`, `cargo`, `safe-push`, `ssh`). It is pure, and
the guard, the readout and P2's replay of section 1 share it.

**The guard**, `crates/north-guard/src/guard/coordinator_delegation.rs`, runs only for role `Coordinator`:

- Events: `hook.rs` maps PreToolUse only for Bash, edits, `Agent` and timers
  (`crates/north-core/src/hook.rs:126`); G1 adds a `PreOther` kind so `Monitor`, `TaskStop`, `Read`,
  `ToolSearch` and `SendMessage` reach the guard too, with a test per tool.
- State is keyed by session and `agent_id`, as `codex_conduct` keys it
  (`crates/north-guard/src/guard/codex_conduct/state.rs:42`), in
  `$XDG_RUNTIME_DIR/north/coordinator-calls/`, under a file lock. Calls with an `agent_id` are the read-only
  subagents of 2.4; they never count toward the coordinator's streak and are counted as their own metric.
- Counted: diagnosis, landing logs, status checks, monitoring scripts and bookkeeping. Reset: only a
  delegate or brief call, or a message from `Tom`. Child messages and Tom-facing calls neither count
  nor reset, so a no-op `ListAgents` cannot clear a denial.
- At 5 (`coordinator.warn_streak`) PostToolUse adds one line: the streak, its kinds and the brief command
  for the work it looks like. At 15 (`coordinator.deny_streak`) PreToolUse denies the next counted call with
  that line. The source of both numbers is section 1: the warning would have fired 145 times, and 42
  streaks reached 15.
- Hands-on landing and lane edits are denied from the first call: `safe-push --to`, `north land
  enqueue`, `git commit|push|rebase` and writes inside a lane worktree, because a worker owns its lane
  through landing. G1 first moves `north cloud land` to the cloud courier, since today the launching
  session runs it (`harness/skills/workers/references/cloud.md:33`).
- Always allowed, tested in the denied state: delegate and brief verbs, child messages, `north gate`,
  `north config agents off|on`, `north land abandon` and ticket removal, `north fleet stop|forget`.
- Escape: `north coordinator incident REASON` grants a budget of 30 calls (`coordinator.incident_budget`) for an
  incident the coordinator must work itself, and writes a metric row with the reason, so the escape is
  counted rather than a persistent switch-off. What shipped instead was `north fleet break-glass` (section 7, removed in north#1650),
  which lifts only while the harness itself measures that no worker can start or that landing is stalled.

**Recurring work becomes code**: the classifier reduces each counted call to a shape (the verb, or
the first two words of each pipeline stage after any `cd`), and the readout files one issue per
counted shape the coordinator ran 20 or more times in a day. Delegating verbs are not counted kinds, so
`north fleet launch` never files one. In the measured day the rule names `north fleet show` (75), the
land queue directory (101), `merge-base --is-ancestor` (47) and the lander's journal (57).

No read count is kept. The read-streak limit (north#838, 2026-11-07) and the per-message look budget (north#1106,
2026-11-11) were removed on 2026-11-16 (north#1632), at the owner's objection to capping the coordinator's reads. The
coordinator's read-only calls are not refused for their number. `north config coordinator calls TRANSCRIPT` still prints
the streaks of a transcript between hand-offs, without a limit.

The coordinator reach guard (north#1106) was removed with break-glass in north#1650, so the coordinator's reads are not refused. The role guard still refuses builds and tests in any
runner (cargo, bun, npm, make, go, pytest, nix build) and shell writes into a repository (redirect,
tee, patch, git apply) from the coordinator.

All of this reads one table, `coordinator_role` in `harness/defaults.toml` (north#1106): one row per class of
call, owned by the coordinator (launch, send, ask, stop, adopt, gate, issue-box, status, placement-look) or by
a worker it briefs (source-read, remote-read, build, test, repository-edit).
`crates/north-guard/src/guard/coordinator_table.rs` reads it into typed rows, refusing a missing, duplicated or
unknown class, and `north config coordinator role` prints it. Section 7 records the removed `north fleet break-glass`.

Enforced by: hook for the streak, the landing denial and the exemptions; types for verb kinds;
metric with an alarm for recurring work. "Predictable answer" stays text; the streak bounds what
misjudging it costs.

### 2.6 The readout and its alarm

`north ops coordinator` writes one row a day to the threads database (`~/.local/state/threads/threads.db`):

- calls by kind from the coordinator's transcript, split into streaks a `Tom` message started and streaks
  the coordinator started on its own (diagnosis Tom asks for is not the coordinator's failure);
- research subagents launched and `north fleet break-glass` grants;
- turns woken by fleet messages, by sender kind;
- landed boxes, counted from `Box:` trailers on commits that reached a main branch that day (section
  4), so every landing path counts, not only the lander's;
- **coordinator hands-on calls per landed box**, the self-started counted calls over landed boxes. With no
  landed box that day the ratio is not computed and the alarm does not read the day.
- **Tom's corrections per day**, read from the outcome loop's count from the grounding ledger once that
  lane lands; until then the row leaves it empty. A lexical classifier over Tom's messages is rejected:
  the measured day matched 54 of 116 messages, which is noise, and the standing-order line count is
  written by the coordinator itself, so it would grade its own work.

One alarm: hands-on calls per landed box above `coordinator.hands_on_per_box` two days running opens or
updates one issue and sends the coordinator one line. It and the repeat-shape rule are detectors in the
self-detectors filer once that lane lands, rather than a second issue filer; if the experiments judge
has landed first, the thresholds of 2.5 and this alarm each carry its registry entry. The threshold's
first value comes from the first week of rows, because the commit counts of section 1 (336 commits)
overstate boxes several times.

Nothing runs these today: no host schedules `north ops check`, Nexus schedules only `north tune`, and
gex45 has only `north-watch` and `north-runtime-follow` timers. Box T1 adds both timers to Firn and is
done only when a row is written and the check runs on its schedule.

Enforced by: metric with an alarm, on a running timer.

## 3. The five dropped coordinator rules

South's `agents/rules.toml` registers five rules written for the coordinator whose placement is `dropped`:
the three in its `Coordinator` section (`status-in-toms-terms`, `ground-before-answering`,
`check-facts-before-stating`), `know-fleet-names`, which the same role hook carried, and
`coordinator-delegates-durable-work`.

Why they are dropped, from South's history:

- 2026-10-12, South `024f6a5` (the C2 policy cut) gave every rule a placement; these five were
  `hook:org-role`, injected by the org-role hook into the session it recognised as the coordinator.
- 2026-10-16, South `5684064` removed the supervisor and mailbox and with them the org-role hook and
  its role registrations. The five flipped to `global`, but no bullet for them was added to
  `agents/AGENTS.md`, so no session received them.
- 2026-10-29, South `c3c795e` (north#351): `north check harness` requires each `global` rule to
  have its bullet, so these five were marked `dropped` to make the register match the loaded policy.

So since 2026-10-16 the coordinator has had none of them. The cause is structural, not editorial: role
policy lost its only home when the harness stopped being able to tell the coordinator apart from other
sessions, and today it still can only by absence (no fleet record, `CLAUDE_CODE_ENTRYPOINT=cli`, the
test `harness/hooks/prompt-grounding.sh` uses). The role identity in section 2 gives role policy a home
again, and a new placement kind gives the check something to verify.

Disposition, each with what enforces it:

- `coordinator-delegates-durable-work`: **placed** as `hook:coordinator-delegation`, with the text rewritten to the
  delegation rule in section 2.5. Enforced by the delegation guard (hook) and the repeat alarm (metric).
- `ground-before-answering`: **placed** as `hook:prompt-grounding`. That hook already injects the
  Grounding block with each of Tom's messages; the rule's text moves into the block's header so it
  arrives with the block it refers to, and the stale clause about `pace-control.json` is checked
  against `crates/north-core/src/grounding/live.rs` (it still reads
  `~/.local/state/agents/pace-control.json`, so the clause stays). Enforced by the hook; that the
  answer actually used the block is judgment and stays text.
- `check-facts-before-stating`: **merged** into `ground-before-answering`. A platform, version or game
  fact is one more thing the grounding block names; a separate rule repeats it. Text only: no check
  can tell a checked fact from a recalled one before Tom reads it; the outcome loop's corrections count
  (2.6) measures the misses afterwards.
- `status-in-toms-terms`: **merged** into `one-line-risk`, the global rule that already says to name
  each issue, gate, session or commit in plain words with its number trailing. Text only: no Stop
  guard exists (`crates/north-core/src/hook.rs:201` registers none), and a blocking Stop check would
  cost a full-context turn to catch a bare number.
- `know-fleet-names`: **placed** as `agent:coordinator`, a new placement kind meaning "this rule's text
  is in `harness/claude/agents/NAME.md`". Only the coordinator reads Tom's voice-transcribed messages, so the
  names belong in its role file, not in every session's global rules. Its text also carries a typo
  (a misspelt possessive) fixed in the same change. Enforced by `north check harness`:
  for every `agent:NAME` placement it checks that the role file exists and contains the rule's text,
  so a placement can no longer silently lose its home the way `hook:org-role` did.

## 4. Post-landing bookkeeping, done by the landing

Today the lander checks with `git cherry` on the ticket head before it messages
(`crates/north-landing/src/land/after/mod.rs:70`) and tells the ticket's session; nothing ticks a box,
closes an issue or retires a session. And only north lands through the lander: makgora's
`.safe-push` sets `queue = lock`, and cloud lanes are enqueued by whoever runs `north cloud land`, so
a ticket field would miss most landings. The box therefore travels in the commit.

1. **The box is a commit trailer.** A lane's commit carries `Item: REPO#N` and `Box: TEXT` trailers,
   the text of the Done when box it closes; `north work brief new` prints both for the worker to copy, and
   `north cloud land` keeps them (it strips only `Claude-Session:` and `Co-Authored-By:`). `north land
   scan`, which safe-push and the lander both run before a push, refuses a `Box:` that matches no
   unticked box on the issue, so a wrong box is refused before it lands, not discovered after.
2. **One tick function over newly landed commits.** After a push, the lander's after-landing step and
   safe-push's direct-push path both hand the landed range to the same function, which reads the
   trailers of each commit in it. Squashed or rebuilt lanes change shas, so it reads commits that
   reached main, not a ticket head.
3. **What may be ticked.** A pure function over the issue body and the box text returns the new body
   or `NoMatch`, `Ambiguous`, `AlreadyTicked`, `NeedsObservation` or `NoBoxes`:
   - a box whose text carries `(observed)`, `(headless run)` or a Tom-check marker is never ticked by a
     landing, because landing is not proof of what such a box observes; the receipt says "landed, box
     not ticked: needs observation";
   - a parent box is ticked only when every indented child box under it is;
   - an issue with no parsed checkboxes (makgora#1122's numbered parts) gets a receipt and no edit.
4. **When an issue closes.** Only when at least one box was parsed, every box is ticked, and none is
   waiting on observation. The closing comment names the landed sha.
5. **One writer for issue bodies.** Ticking, the self-detectors' appended boxes and the experiments
   judge's verdict boxes all go through one shared writer: re-read the body, apply the pure edit,
   write, re-read and verify, retry on a lost race. It runs from a durable outbox after the landing,
   under a deadline, so a slow or hung `gh` never holds the landing queue (the failure class of
   north `63d9e7783`'s 34-minute hang).
6. **The receipt.** The session's parent gets one line: what landed in words, the sha, and "issue
   closed", how many boxes remain, or why the box was not ticked.
7. **Retiring the session.** A report gains a required `Next:` line: `none`, or the issue that owns the
   next step. The fleet retires a session (stops and archives it, the path idle retirement takes,
   `crates/north-core/src/fleet/watch.rs:295`) only when every ticket it enqueued has landed, its last
   report opens `Done:` and its `Next:` is `none` or names an issue. A `Next:` naming no issue, or a
   `Split:`, `Not done:` or `Stop:` report, keeps it listed, so a step without an owner never
   vanishes (standing order of 2026-10-16).

Enforced by: code (land scan, the tick function, the shared writer, the fleet watcher), with tests on
the pure body edit, a hung `gh`, and a direct safe-push landing.

The gap until boxes L1 to L3 land is the measured 122 bookkeeping calls a day, 47 of them
landed-ancestor checks. The coordinator keeps doing them by hand, they count in its streak, and those boxes
come early because they remove the most coordinator calls per minute of work.

## 5. Boxes, in landing order

Each box lands through `north land enqueue` (or safe-push in South and Firn), adds the
features entry the features-index check requires for any verb it adds, and is done only when
its sha is an ancestor of the repository's `origin/main`. Prose in `docs/briefing.md`, `docs/landing.md` and the workers
skill waits for D1, so 19 boxes do not all edit the same hot files. Boxes whose Needs have landed run in
parallel. Repository is north unless named; "laptop" marks a box gex45 cannot land, because it has no
push access to South and the Firn timers are host configuration.

1. **W1 the sender is a required value.** feature, cross-module, rust, local-test; 45 min. Files:
   `crates/north-core/src/fleet/registry/senders.rs`, `src/cli/send.rs`, `crates/north-core/src/fleet/registry/parents.rs`,
   `crates/north-tui/src/tui/fleet/desk.rs`, the timer call sites of section 1,
   `crates/north-landing/src/land/after/mod.rs`. Needs: nothing. Done when: `cargo test -p north-core fleet::`
   passes cases where the composer's message carries `Tom`, `north fleet send` from outside a session
   without `--from service:UNIT` is refused, and a source test finds no `north fleet send` shell-out
   without `--from`.
2. **P2 verb kinds and the classifier.** feature, cross-module, rust, local-test; 45 min. Files: the
   CLI dispatch table under `src/cli/`, `crates/north-core/src/coordinator/` (`calls.rs`, `shape.rs`), a
   `north coordinator calls TRANSCRIPT` readout. Needs: nothing. Done when: `cargo test -p north-core coordinator::`
   passes a truth table with a case per kind and per foreign-command rule, and a test that fails a verb
   with no declared kind; on the laptop `north coordinator calls` over the measured transcript prints the
   section 1 counts within the difference the verb table makes, recorded in this page.
3. **R1 role on the session record.** feature, cross-module, rust, local-test; 45 min. Files:
   `crates/north-core/src/fleet/model.rs`, `src/cli/fleet/launch.rs`, `src/cli/fleet/lineage.rs`,
   `crates/north-core/src/fleet/registry/launching.rs`, `crates/north-providers/src/providers/claude/session_env.rs`,
   `crates/north-providers/src/providers/claude/runs/mod.rs`, `crates/north-providers/src/providers/ledger_rows.rs`,
   the lead-timer guard (removed in north#1650). Needs: the staffing-overlap lane, which edits the same
   launch files. Done when: `cargo test -p north-core fleet::` passes cases where `--role coordinator`
   records `Coordinator` and exports `NORTH_FLEET_ROLE=coordinator`, a task launch records `Worker`, a second live
   coordinator is refused without `--take-over`, and the ledger and lead timer read the field.
4. **L1 `Item:` and `Box:` trailers, checked before the push.** feature, cross-module, rust,
   local-test; 40 min. Files: `north work brief new`, `north land scan` in `crates/north-landing/src/land/scans/`,
   `north cloud land`, `harness/skills/workers/references/brief.md`. Needs: the incidents-by-construction
   lane, which edits the lander. Done when: `cargo test -p north-landing land::scans` passes cases where
   a `Box:` matching no unticked box is refused, one matching passes, and `north cloud land` keeps both
   trailers.
5. **B1 one writer for issue bodies.** feature, module, rust, local-test; 45 min. Files: a new
   `issue_body` module beside `crates/north-landing-gates/src/issue_box_lint.rs`, a durable outbox under the
   land queue directory. Needs: nothing; the self-detectors and experiments-judge lanes switch to it in
   whichever lands second. Done when: `cargo test -p north-landing issue_body` passes the lost-race
   retry, the verify-after-write, and a fake `gh` that hangs while the landing completes and the outbox
   entry survives a restart.
6. **L2 the tick function.** feature, module, rust, local-test; 45 min. Files:
   `crates/north-landing/src/land/after/mod.rs`, the safe-push direct-push path, the pure body edit.
   Needs: L1, B1. Done when: `cargo test -p north-landing land::after` passes the body cases (match,
   `NoMatch`, `Ambiguous`, `AlreadyTicked`, `NeedsObservation`, `NoBoxes`, a parent with an unticked
   child), a close only when at least one box parsed and all are ticked, and the same tick from a
   direct safe-push landing and from a train candidate.
7. **L3 `Next:` and retirement.** feature, module, rust, local-test; 35 min. Files:
   `crates/north-core/src/fleet/watch.rs`, `crates/north-core/src/fleet/records.rs`,
   `crates/north-core/src/fleet/receipts.rs`, `harness/claude/agents/worker.md`. Needs: L2. Done when:
   `cargo test -p north-core fleet::watch` passes cases where a session is retired only with every
   ticket landed, a `Done:` report and `Next:` none or an issue, and kept for a bare `Next:` text, a
   `Split:` or a second unlanded ticket.
8. **A1 the role file and the `agent:NAME` placement.** docs-policy, cross-module, rust, local-test;
   30 min. Files: `harness/claude/agents/coordinator.md`, `crates/north-agent-policy/src/agent_config_check/rules.rs`.
   Needs: nothing. Done when: `cargo test -p north-core agent_config_check` passes a case where an
   `agent:coordinator` rule whose text is absent from `coordinator.md` is refused and one where it is present
   passes, and `coordinator.md`'s tools carry `Agent(Explore, Plan)`.
9. **R2 role prompt and grounding.** feature, cross-module, rust, local-test; 30 min. Files:
   `crates/north-core/src/fleet/prompt.rs`, `harness/hooks/prompt-grounding.sh`,
   `crates/north-core/src/fleet/audience.rs` (the hook's audience, passed into `north graph ground hook`
   by the CLI root). Needs: R1, W1. Done when: `cargo test -p north-core fleet::audience`
   passes cases where a role-coordinator session gets the block for a `Tom` message and not for a `Service`
   or `Session` one and a worker gets none, and `cargo test -p north-core fleet::prompt` shows the
   coordinator preamble.
   **R2b grounded recommendations** extends R2 to the coordinator's final report. When the last paragraph
   that says "I recommend", "Recommended:", "the fix is" or "the fix would be" reaches the Stop hook,
   `crates/north-guard/src/guard/recommendation_grounding.rs` runs that paragraph through the
   UserPromptSubmit hook's retrieval (`north_core::grounding::bearing`) and, when a decision record or
   policy section hits at tier A (cover at the hook floor, code files left out) and shares at least
   two subject terms with it, blocks once. The recommendation words themselves leave the query, and
   edit words such as "add", "rename", "line", "heading" or "README" are not subject terms: on the
   live index "adding a blank line at the end of the changelog" and "renaming the README heading"
   reached tier A on edit words alone. The block reads:
   "These recorded decisions bear on your recommendation: …; read them and confirm or revise."
   `stop_hook_active` lets the revised report through, and a Stop that also carries an ask joins both
   reasons in one block. Recurrence on 2026-11-05: the coordinator recommended building the laptop runtime
   on gex45 and shipping it, which South's gex45 access plan (P3b, B3, B17) forbids, and found the
   record only after Tom replied. **Enforced by** the Stop hook; its test in
   `crates/north-guard/src/guard/recommendation_grounding_tests.rs` replays that recommendation: the
   fixture decision record is named once and `stop_hook_active` lets the revised report through.
10. **G1 the delegation guard.** feature, module, rust, local-test; 45 min. Files:
    `crates/north-guard/src/guard/coordinator_delegation.rs`, `crates/north-core/src/hook.rs` (`PreOther`),
    `harness/defaults.toml` (`coordinator.warn_streak`, `coordinator.deny_streak`). Needs: R1, P2, W1. Done when:
    `cargo test -p north-core guard::coordinator_delegation` passes the 5th counted call warning, the 15th
    denied, each exemption passing while denied, a `ListAgents` neither counting nor resetting, a
    delegate call and a `Tom` message resetting, a call with an `agent_id` not counting, two parallel
    writers under the lock, a test per `PreOther` tool, and a worker session untouched; with its
    experiments registry entries if the experiments judge is on main.
11. **G2 the landing denial and the incident escape.** Shipped on the role guard, as G1 was, not in a
    new module. `crates/north-guard/src/guard/coordinator_role.rs` refuses from the first call
    `safe-push`, `git commit` and an edit tool's write under `~/code`, a lane worktree included, unless
    the lane is one doc or config file; the escape `north fleet break-glass` (section 7, removed in north#1650) lifted the reach guard (removed in north#1650) in place of a
    `north coordinator incident` verb; and the laptop's
    `north-cloud-reap` timer lands every cloud handback, so no launching session runs `north cloud land`.
    Done when: `cargo test -p north-guard coordinator_role` passes
    `the_coordinator_cannot_build_test_or_edit_a_repository` (an Edit, Write, MultiEdit and NotebookEdit
    inside a lane worktree refused from the coordinator and open to a worker) and
    `proxy_lands_its_own_config_lane` (`safe-push --to main` over a lane with code refused).
12. **S1 South: place the five rules.** Repository: south, laptop. docs-policy, one-file, docs,
    local-test; 20 min. Files: `agents/rules.toml`. Needs: A1, G1, R2 on the runtime. Done when:
    `north check harness policy-gate session` exits 0 on the South lane with the placements of
    section 3 and the typo fixed.
13. **R3 `north coordinator`.** feature, cross-module, rust, local-test; 45 min. Files: `src/cli/` (a
    `coordinator` verb), `crates/north-tui/src/tui/fleet/` (model and effort slash commands),
    `crates/north-providers/src/providers/claude/fleet.rs` (`--agent`), `crates/north-core/src/fleet/registry/takeover.rs`.
    Needs: R1, R2, A1, W1. Done when: an integration test under `tests/it/` shows `north coordinator` with no
    coordinator launching one with role coordinator, `--parent none`, cwd `~/code/north/main` and `--agent coordinator`
    in the fake Claude's argv; with a live coordinator it attaches without launching; with the old `coordinator`
    and `coordinator-gex45` records present it archives both and delivers one digest line;
    and `north coordinator --interactive` stops the fleet coordinator before it execs `claude --resume` on the same
    conversation, and refuses while the fleet session still runs.
14. **R4 the coordinator's digest.** feature, module, rust, local-test; 40 min. Files:
    `crates/north-core/src/fleet/registry/messages.rs`, `crates/north-core/src/fleet/receipts.rs`,
    `harness/defaults.toml` (`coordinator.digest_minutes`). Needs: W1, R1. Done when:
    `cargo test -p north-core fleet::` passes cases where five receipts within 10 minutes reach the
    coordinator as one message, a red-main notice, a gate, a decision stop and an alarm pass at once, and a
    `Tom` message is never held.
15. **F1 Firn: Tom's coordinator window runs `north coordinator`.** Repository: nixos-config, laptop. mechanical,
    one-file, nix, local-test; 30 min. Needs: R3 on the runtime and Tom's answer to the gate below.
    Done when: `rg -n 'north coordinator' ~/code/nixos-config/main` names the entry, and after the rebuild
    the box drives the real `north coordinator` against `claude -p`: a typed message gets a reply, `/model`
    relaunches on the new model, and a forced compaction leaves the session answering. Escape hatch, if
    the fleet TUI fails Tom: `north coordinator --interactive` (built in R3) stops the fleet coordinator and resumes
    the same conversation in Claude Code's own UI with `claude --resume <session-id>`, never with both
    running; the box drives it once and sees the fleet record stopped and the resumed window answer.
    Voice in that window comes from the nixos-config `dictation` module (box F1v): hold the Menu key,
    release to type the transcript into the focused window.
16. **M1 the daily readout.** feature, module, rust, local-test; 45 min. Files:
    `crates/north-ops/src/ops/coordinator/`, the threads database schema. Needs: P2, L2. Done when:
    `cargo test -p north-core ops::coordinator` on a synthetic transcript fixture (no Tom text) gives calls by
    kind split by who started the streak, research subagents, escapes, wakes by sender kind, landed
    boxes from trailers, the ratio, the no-box day with no ratio, and an empty corrections field until
    the outcome loop is on main.
17. **M2 the alarm and the repeat rule.** feature, module, rust, local-test; 40 min. Files: the
    self-detectors module, `harness/defaults.toml` (`coordinator.hands_on_per_box`, `coordinator.repeat_shape`).
    Needs: M1 and the self-detectors lane. Done when: its tests pass two days over the threshold
    opening one issue and sending one line, one day over opening none, and a counted shape run 20
    times filing one issue that the next day updates; with its experiments registry entry if the
    experiments judge is on main.
18. **T1 the timers.** Repository: nixos-config, laptop. mechanical, one-file, nix, local-test; 30
    min. Needs: M1. Done when: `systemctl --user list-timers` on the ledger host lists the
    `north ops check` and `north ops coordinator` timers, and the threads database holds a `north ops coordinator`
    row written by the timer.
19. **D1 the docs.** docs-policy, cross-module, docs, none; 30 min. Files: `docs/briefing.md`, `docs/landing.md`,
    `harness/skills/workers/SKILL.md`, `harness/skills/workers/references/cloud.md`. Needs: R3, G2, L3.
    Done when: `rg -n 'north coordinator|Box:|Next:' docs/briefing.md harness/skills/workers/SKILL.md`
    finds the coordinator's entry point, the trailers and the report line.

Order by payback: W1, P2, A1 and B1 start at once; R1 and L1 as soon as the lanes they collide with
land; L2 and L3 next, because bookkeeping is the cheapest large cut; then R2, G1, G2 and S1; R3, R4
and F1 make the window Tom uses the fleet coordinator; M1, M2 and T1 measure what the others changed; D1
closes.

F1 changes the window Tom types into. In the recommended design the coordinator window
opens north's fleet TUI on the `coordinator` session, so every receipt, landing and alarm reaches the
session he reads; he keeps model and effort changes through the TUI, and loses image paste and the
`!` shell line, which he used once each in the measured day. The alternative keeps the Claude Code
terminal and adopts it into the fleet through hooks: he keeps everything he uses there, but messages
reach it only when he types or a tool runs, the gap this plan exists to close.

## 6. Review disposition

Two reviews of the first version (`bc3155159`): a scenario review and an architecture review. Every
must-fix was accepted. Where they overlapped, the architecture review's stricter form was taken:
the sender as a required enum, not a convention; the box as a commit trailer, not a ticket field.
Rejected or changed:

- A `--box` flag stored on the landing ticket and checked at enqueue: replaced by trailers checked in
  `north land scan`, which covers makgora's lock queue and cloud lanes, where no ticket carries a box.
- Taking landed changes from the outcome loop's join for the denominator: trailers count landed boxes
  on every path without waiting on an unlanded lane.
- The standing-order line count as the corrections signal: the coordinator writes those lines, so it would
  grade its own work. The outcome loop's count from the grounding ledger is used once it lands.
- Moving every docs line into one closing box: a box that adds a verb still adds its
  features entry, because the features-index check requires it in the same landing.
- Committing the measured transcript as P2's fixture: it holds Tom's messages and north is public, so
  P2 commits a synthetic truth table and reproduces section 1 on the laptop.

## 7. Break glass

The coordinator's reach guard (removed in north#1650) existed so that an investigation goes to a worker. It could not bind during an
incident, so `north fleet break-glass --issue REPO#N REASON...` lifts it for at most 30 minutes
(`break_glass_minutes`), and only when the harness measures the incident itself: the last two launches
failed, the launch probe exits non-zero, the laptop's capacity watchdog reports an active host-overload
episode, or the coordinator host booted in the last 30 minutes. A coordinator that merely wants to investigate is refused. The grant
is recorded with who, why and when, opens a box on the issue that tracks the cause, and is printed in
Tom's next `north gate digest`, so the lift is never silent and never outlasts the incident.
**Enforced by** the break-glass guard (removed in north#1650).

## 8. Closing confirmed reports by rule: dropped

The share of coordinator turns triggered only by a `Done:` report or landing notice was 13.8% over one frozen day, under the 25% stop bar, so closing them by rule is not built. See [close-by-rule-share.md](close-by-rule-share.md) (north#1266).

## 9. Clean-up: the tests this work keeps

Checked on 2026-11-15. The 64 commits on main that cite north#112 added 238 Rust tests and six fixtures. Six tests stay,
whole or in part, each named for its guarantee and kept for the reason given here:

- `a_message_from_tom_reaches_the_session_unprefixed_and_any_other_sender_is_named` and
  `tom_on_another_host_is_still_unprefixed_where_a_service_names_its_host` in
  `crates/north-fleet-supervisor/src/fleet/registry/messages.rs`, and
  `a_command_line_sends_as_a_service_or_its_session_and_never_as_tom_or_nobody` in
  `crates/north-fleet-store/src/sender.rs` (W1). Security: only Tom's messages arrive unprefixed, so no service, session,
  timer or remote host reads as Tom. The north-fleet cull `5f0bdaeccb` (north#635) kept them for this.
- `a_launch_naming_an_item_a_live_session_holds_is_refused_and_leaves_no_claim_behind` in
  `crates/north-fleet/src/fleet/claims.rs`: two sessions never work one item. It alone kills the recorded mutant
  `n112-refused-launch-keeps-earlier-claims` (`7840926ea6`).
- The lane-worktree Edit, Write, MultiEdit and NotebookEdit cases of
  `the_coordinator_cannot_build_test_or_edit_a_repository` in `crates/north-guard/src/guard/coordinator_role_tests.rs`
  (G2): with `edit_under_code` returning `None`, every coordinator role test passed before them (`5d1fbf905f`).
- Removed with the read counts (north#1632, 2026-11-16): the streak walk and the look-budget walk in `src/cli/hook_verb_tests.rs`
  and `crates/north-guard/src/guard/coordinator_replay_tests.rs`, the budget and streak assertions of
  `crates/north-guard/src/guard/coordinator_role_tests.rs`, and the `pre-other` row of `provider_events_select_matching_guard_list`
  (G1). That row and the streak walk reproduced the bug `7a7f8722ec` fixed (a ListAgents call reset the streak, and WebFetch and
  WebSearch reached no hook); the H10 hold names the guards that remain.

Every other test is already gone, so none is deleted here: the north#635 culls of 2026-11-06 took most of them and the
north#1583 batches B1 to B7 of 2026-11-14 and 2026-11-15 took the rest, the last being the tune tick's step-limit test
(`e8e5838d23`). The six fixtures (five ops-check breach replays and the 2026-11-04 night incident log) went with their
tests.

## Feature record "Launch claims", as written until 2026-11-13

The feature record `0021-launch-claims.md` was cut to a short entry; this is its earlier text, kept for the reasons and dates it holds.

- Launch claims: `north fleet launch` claims every issue its session holds before it starts (`--task REF`, else the issues the brief's first `Item:` line names, else the issue its body opens with, as `north#855: run ...` does), leaving a whole-issue task its parent chain holds to that holder and passing the chain to the ledger, so a lead keeps its issue while the children it staffs take the boxes they work, refusing a live claim outside the tree and taking one a stopped session left, and releases them when the session ends or fails to start; `north fleet list --json` shows them as `tasks`. With `--box TEXT`, or a `Box:` line in the brief, the claim covers that one box of the first issue: a second launch on another box of the same issue is admitted, a second launch on the same box is refused, a launch outside the leader's tree on the whole issue beside a box, or on a box beside the whole issue, is refused, and the run row records the issue and the box. A box text that names no box of its issue is refused before placement or any claim, naming the boxes the issue has (north#1152: on 2026-11-11 two such launches started their workers holding nothing and a third was refused after 30 s with a message that blamed the ledger); a claim the ledger refuses after it was queued refuses the launch in the ledger's own words as soon as the spool sends it. Dispatch staffs every issue box by box on these box claims, up to the free slots, or up to N workers when its body says `Workers: N`.
- Filed claims: when a fleet worker files an issue, on whichever host it runs, the PostToolUse filing hook runs `north fleet hold REPO#N` as soon as `gh issue create` returns the URL, which names the issue in the session's own record beside a launch's `--task` (so `north fleet list --json` shows it in `tasks`, as it does a launched session's, to every host's dispatch) and claims it in the threads ledger, so north fleet dispatch never staffs a second session on it while the session lives; before north#817, a session launched without `--task` held a filed issue only as a ledger claim, and dispatch staffed north#815 beside the session that had filed it. A blocker it files (a `Blocks:` line) and an issue the coordinator files stay unclaimed for dispatch to staff, except an issue the worker files as its own item (its brief's `Item:` line says to file it first), which it keeps whatever it names as blocked. Before this, dispatch staffed north#736, north#737, north#738 and north#739 beside the sessions that had just filed them. `north fleet hold --release REPO#N` undoes it: the session's record drops the issue, with any box its launch named on it, and every claim the session holds on the issue is given up, so dispatch staffs it on its next run; before north#1054 a hold had no release, and on 2026-11-10 dispatch kept skipping north#722 as held by the coordinator after its cause was fixed.

## Feature record "Window move", as written until 2026-11-13

The feature record `0027-coordinator-move.md` was cut to a short entry; this is its earlier text, kept for the reasons and dates it holds.

- Window move: `north coordinator move [--to ACCOUNT] [--auto] [--dry-run]` moves the window holding the conversation the fleet record names for the coordinator, whatever other interactive `claude` windows are open, when its account's outlook is under 45 minutes (or out), or when `--to` names an account with week headroom, to the account with the most week headroom whose own outlook is not under 45 minutes; it refuses when that conversation has no live window on a terminal and names the windows it saw (north#636, north#1238). The caller only starts the user unit `north-window-move` and prints its result, so a sandboxed shell, the coordinator's own included, can run it: the unit reads the windows, runs the terminal test and prints the plan, which `--dry-run` stops at. Otherwise it starts the unit `north-window-move-swap`, which runs the steps at once, beside the running old window, saying on the old window what it does: carry the session's transcript, subagent files, file history and session environment into the target profile; open a ghostty window running `claude-ACCOUNT --resume SESSION` with the old window's flags and wait until it runs under the target profile's `CLAUDE_CONFIG_DIR`; change the fleet record's account and bind the coordinator's name to the new window; deliver its undelivered messages with a fleet tick; and only then, once the old window's turn ends or after 30 seconds at most, stop the old daemon-hosted session, hang up the old window's shell and put the new window in the old one's workspace and column with focus restored. A failure before that last step closes only the new window and binds the name back to the old one, which keeps running as it was. A turn the old window takes after the carry stays in the old account's transcript, and the new window does not see it (north#1238: a move that waited for the old turn first showed nothing for minutes, because delivered messages kept waking the old window). The fleet tick runs `--auto`, which reads the coordinator's account from the fleet record and starts the swap unit at most once in 20 minutes.
