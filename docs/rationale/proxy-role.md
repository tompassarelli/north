# The proxy's role: route and judge, delegate the rest, and measure it

Tom asked on 2026-11-05 for the orchestrating session to act as one: do brief cross-cutting work itself
and delegate everything that takes several steps, with a deterministic mechanism behind each rule
wherever one can carry it. This page measures what the proxy did in one day, then designs the role so
that code, hooks, gates and alarmed metrics hold it, and prose only where nothing can check the rule.
Each design part ends with an **Enforced by** line.

## 1. Evidence: one day of the proxy

Source: the terminal proxy's transcript, conversation `895929a7-6218-4168-b513-3dced3440bf6` (account d,
laptop), copied at 3:24 AM on 2026-11-05 (laptop time, UTC+7). It covers 3:50 AM on 2026-11-04 to
3:23 AM on 2026-11-05: 5,880 Opus 5.5 turns, 2,753 tool calls, 116 messages typed into the session and
11 context compactions. Its turns read 1,549 M cache tokens, about 263 k per turn, so every call the
proxy makes re-reads a quarter of a million Opus tokens of context.

Headline: **2,259 Bash calls against 220 Agent delegations** (the brief counted 2,241 Bash at its time
of writing; the session kept running). With fleet launches added, 282 calls of 2,753 handed work away.

Method: a jq extract of every `tool_use` with its full input, classified by an ordered first-match
rule list over the tool name and the command text (the same classifier box P2 makes code). Kinds, in
the order the rules try them:

- delegate: `Agent`, `north fleet launch|revive`, `north dispatch`, `threads queue follow`, `north cloud launch`
- child message: `SendMessage`, `ListAgents`, `north fleet send|ask`
- Tom-facing: `north gate`, `date '+%I:%M %p'` (clock times for Tom)
- briefing: `north brief`, a brief file or heredoc starting `Item:`
- monitoring scripts: `Monitor`, `TaskStop`, `ScheduleWakeup`, `CronCreate`, shell loops and sleeps, `nohup`/`setsid`, writing a `.sh` or `.ts` helper
- bookkeeping: `gh issue edit|close|comment`, `threads tick|close|claim|release`, `north fleet stop|forget`, `git merge-base --is-ancestor`, worktree and branch removal, notes appended to the scratchpad
- hands-on landing and editing: `Edit`, `Write`, `safe-push --to`, `north land enqueue`, `git commit|push|rebase|add`
- landing logs: safe-push and land-check logs, the land queue directory, `journalctl -u north-land*`
- status checks: `north fleet show|list|tree`, `journalctl`, `north capacity`, `north flow`, `ps`, `gh run`, usage and pace
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

Streaks: between two delegations, child messages, briefs or Tom-facing calls the proxy ran 281 streaks
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
  proxy's context.
- Polling status: 214 calls, 75 of them `north fleet show NAME`. Receipts deliver a child's report to
  its parent, but the proxy is no fleet session: 54 of its 60 fleet launches passed `--parent none`,
  so no report, landing or silence notice could reach it and it polled instead. Its 220 `Agent`
  delegations (112 `worker-sonnet`, 80 `worker`, 20 `worker-haiku`, 8 `planner`) were in-process
  subagents, whose results return into the proxy's own context.
- Post-landing bookkeeping: 122 calls verifying ancestry, stopping and forgetting sessions, ticking and
  closing issues and keeping a scratchpad queue. The landing knows every fact these calls re-derive.
- Monitoring scripts: 182 calls standing up and tearing down watchers for runs that the fleet's waits
  and receipts already report.

The root cause under all ten kinds is one gap: **the harness cannot name the proxy.** The proxy runs
outside the fleet with `NORTH_FLEET_SESSION` unset, so it cannot be a parent, a lander ticket's
session or an alarm's recipient. Its children report to nobody (`--parent none`) or to the idle
fleet session that is also called `proxy`, on account a: launches with no `--parent` and no caller
default to that name (`src/cli/fleet/lineage.rs:19`), and `north ops check`, `north ops dividend` and
`north flow` alerts all send to it (`crates/north-core/src/ops/check/mod.rs:45`,
`crates/north-core/src/ops/flow/alerts.rs:68`). So the session Tom reads hears nothing, and the
session that hears everything has no reader. The lander already tells a ticket's session when its
lane lands or is refused, with the conflicting files (`crates/north-landing/src/land/bisect.rs:126`),
but a lane the proxy pushed itself, or one an in-process subagent pushed, has no session it can reach,
so the proxy read the journal instead.

What Tom does in that terminal besides typing, from the same transcript: one each of `/model`, `/effort` and `/goal`, one pasted image and one `!` shell line. 6,576 of the transcript's 8,578 working-directory entries are `~/code/north/main`, whose project memory and project instructions load with it. In-process subagent calls do not appear in it (every turn has `isSidechain: false`), so subagent work is invisible to this count.

The counts reproduce on the laptop through box P2's readout (`north proxy calls TRANSCRIPT`); the transcript itself stays private and is not committed.

The same inference by absence recurs one level down, at the sender: every `north fleet send` from
outside a session is unsigned (`src/cli/send.rs:24`), and so are fleet receipts and silence notices
(`crates/north-core/src/fleet/registry/parents.rs:48`), the lander's messages
(`crates/north-landing/src/land/after/mod.rs:47`), wait results (`src/cli/fleet/waits.rs:76`), about
ten timers (`crates/north-core/src/ops/check/facts.rs:90`, `ops/dividend/live.rs:213`,
`ops/runtime/update.rs:416`, `ops/tune/mod.rs:257`, `ops/timers/fleet_session.rs:95`,
`ops/timers/cross_project.rs:175`, `health/failed_starts.rs:115`, `ops/flow/alerts.rs:188`) and the
TUI composer Tom types into (`crates/north-tui/src/tui/fleet/desk.rs:54`). Nothing can tell Tom's
message from an alarm. The design fixes both absences with values the compiler checks.

## 2. Design

### 2.1 The sender is a required value

`send_signed` and the fleet row's sender take `Sender`, an enum of `Tom`, `Session(name)` and
`Service(unit)`, with no default. The TUI composer and `north proxy` pass `Tom`; the lander, receipts,
waits and every timer pass `Service` with their own unit name; a fleet session passes its name. A
message from `Tom` reaches the session unprefixed, any other with `[from NAME]`, and the session's
hooks see the sender as a field of the delivered message, not as the absence of a prefix.

Enforced by: types. A new timer cannot send without naming itself, so it cannot pose as Tom.

### 2.2 The session's role is set by the launcher

- `SessionRecord` (`crates/north-core/src/fleet/model.rs:135`) gains `role: Role` with `Proxy`,
  `Lead` and `Worker`. `north fleet launch --role proxy` sets it; a launch with a task (`--task` or an
  `Item:` line) is `Worker`, any other `Lead`. The launcher exports `NORTH_FLEET_ROLE` beside
  `NORTH_FLEET_SESSION` (`crates/north-core/src/providers/claude/session_env.rs:30`).
- At most one live session has role `Proxy`, named `proxy`, the constant receipts, alarms and lineage
  already use (`crates/north-core/src/fleet/runtime.rs:94`). A second proxy launch is refused unless it
  passes `--take-over`. For a proxy, take-over archives the old record and hands over one digest line
  (how many messages, from which senders, the oldest age) instead of replaying the backlog into the new
  proxy's first turn; the account-a `proxy` record and the parked `proxy-gex45` record (account c) are
  both archived this way by the first `north proxy`.
- The proxy moves to another account through the same take-over when its window runs out, the
  handover the provider-reserve plan already designs for work in flight (north `d7509e4ea`). While
  idle it parks like any session and the next message revives it.
- Every place that infers the proxy from an absence reads the field:
  `crates/north-core/src/providers/claude/runs/mod.rs:147` and `crates/north-core/src/providers/ledger_rows.rs:89`
  (no parent and no children means proxy), `crates/north-guard/src/guard/lead_timer.rs:36` (a name
  containing `lead`), `harness/hooks/prompt-grounding.sh:29` (absent from fleet records).

Enforced by: code. Membership is set by the launcher, never remembered; the single-proxy rule is a
registry refusal with a test.

### 2.3 The session Tom types into is that fleet session

`north proxy` launches or revives the fleet session `proxy` with `--role proxy --parent none`, in
`~/code/north/main` so the project memory and instructions the proxy has today keep loading, on the
account `agents plan` routes it to. It then opens north's fleet TUI focused on it; the TUI already
sends from a composer and renders the transcript (`crates/north-tui/src/tui/fleet/screen.rs:70`,
`:378`). The launcher passes `--agent proxy` (Claude Code 2.1.318 has the flag), so the role file is
the agent definition from the first turn. Firn's entry for Tom's proxy window runs `north proxy`.

What Tom loses: a fleet session runs `claude -p --input-format stream-json`
(`crates/north-core/src/providers/claude/fleet.rs:50`), which has no slash commands beyond the wired
`/compact`, no image paste and no `!` shell. In the measured day he used each once (`/model`,
`/effort`, `/goal`, one image, one shell line). `north proxy` covers model and effort with a TUI slash
command that relaunches the proxy through take-over; image paste and `!` are not covered, and how voice
dictation reaches the TUI is untested. Box F1 waits on Tom's answer for this reason.

Rejected, two ways to keep the interactive Claude Code window as the proxy:

- Adopting it into the fleet through a SessionStart hook and `CLAUDE_ENV_FILE`. Messages would reach
  it only when a hook fires, so an idle window would hear nothing until Tom typed, the failure being
  fixed, and it too is a vendor-UI workaround.
- Pushing north's messages through Claude Code's own cross-session inbox
  (`/run/user/1000/cc-socks/<pid>.sock`, which `SendMessage` uses). It does reach an idle
  interactive session: at 3:58 AM on 2026-11-05 (laptop time) a peer's message woke the idle proxy
  terminal. It is rejected because north would still not own the runtime: no failover to another
  account when one runs out, no parking and no sender on messages (Claude Code already resumes on
  its own after a usage limit, as Tom noted on 2026-11-05); because the socket protocol is
  internal to Claude Code and may change under any update; because
  [read-isolation.md](read-isolation.md) already denies workers those sockets as a boundary, and a
  north writer into them would cross it from the other side; and because it is the vendor-UI
  workaround the standing order rules out.

Receipt volume: once the proxy has a reader it is the default parent of every launch from outside the
fleet (`src/cli/fleet/lineage.rs:19`), and each message wakes a turn at about 263 k context. Messages
to the proxy from `Service` and `Session` senders coalesce into one digest delivered at most every 10
minutes (`proxy.digest_minutes`), except urgent ones: red main, a gate opened, a decision stop and an
alarm. Messages from `Tom` are never held.

Enforced by: code (`north proxy`, `--agent`, the digest), plus Firn config for the entry point.

### 2.4 The role file

`harness/claude/agents/proxy.md`, beside `worker.md` and `planner.md`. Frontmatter: `model: opus`,
`effort: medium`, and a `tools` list whose only `Agent` entry is `Agent(Explore, Plan)`, which Claude
Code enforces as an allowlist of in-process subagent types ("a deny rule, the Agent(x,y) allowlist or
an agent.offer hook excludes it", from the 2.1.318 binary). In-process subagents are read-only; every
worker is a fleet session the lander can tell. The body holds only what no mechanism checks:

- You are Tom's proxy (Plato) and route and judge: turn what Tom asks into briefs, staff them, read
  their reports against what he meant, and tell him results and decisions.
- What stays with you: judging a result against Tom's intent (a `Done:` that does not do what he asked
  is not done); calls that touch many lanes at once (which overlapping lane waits, a cross-repo order, a
  revert across lanes); routine decisions under the standing orders; real decisions to Tom through
  `north gate`.
- Do a task yourself only when it takes about 3 tool calls or its answer is predictable; anything
  longer is a brief.
- The names (from `know-socrates-names`): Socrates is Tom's workforce of fleet sessions coordinated by
  the proxy; North is its harness; Plato is the proxy, which voice transcription may write "Play-Doh";
  Nexus is the always-on server.

`crates/north-core/src/fleet/prompt.rs` renders a role-specific preamble: the proxy keeps the standing
orders and drops the worker line "Tom and the proxy reach you only through fleet messages".
`harness/hooks/prompt-grounding.sh` fires for role `Proxy` on messages whose sender is `Tom`, instead
of on the absence of a fleet record, so 2.2 does not switch grounding off.

Enforced by: Claude Code for the subagent allowlist; text for the judgment lines, because whether a
result matches Tom's intent or an answer was predictable cannot be computed from a transcript; `north
agent-config-check` checks that the file is installed and holds the text of every rule placed
`agent:proxy` (section 3).

### 2.5 The delegation rule, counted by a hook

**Kinds come from the verb.** Each `north` verb declares its kind (delegate, brief, child message,
Tom-facing, bookkeeping, landing, status, diagnosis) in the CLI dispatch table, and a test fails any
verb without one, so a new delegating verb cannot fall through to diagnosis. The classifier,
`crates/north-core/src/proxy/calls.rs`, asks that table for `north ...` commands and keeps an ordered
rule list only for foreign commands (`git`, `rg`, `gh`, `cargo`, `safe-push`, `ssh`). It is pure, and
the guard, the readout and P2's replay of section 1 share it.

**The guard**, `crates/north-guard/src/guard/proxy_delegation.rs`, runs only for role `Proxy`:

- Events: `hook.rs` maps PreToolUse only for Bash, edits, `Agent` and timers
  (`crates/north-core/src/hook.rs:126`); G1 adds a `PreOther` kind so `Monitor`, `TaskStop`, `Read`,
  `ToolSearch` and `SendMessage` reach the guard too, with a test per tool.
- State is keyed by session and `agent_id`, as `codex_conduct` keys it
  (`crates/north-guard/src/guard/codex_conduct/state.rs:42`), in
  `$XDG_RUNTIME_DIR/north/proxy-calls/`, under a file lock. Calls with an `agent_id` are the read-only
  subagents of 2.4; they never count toward the proxy's streak and are counted as their own metric.
- Counted: diagnosis, landing logs, status checks, monitoring scripts and bookkeeping. Reset: only a
  delegate or brief call, or a message from `Tom`. Child messages and Tom-facing calls neither count
  nor reset, so a no-op `ListAgents` cannot clear a denial.
- At 5 (`proxy.warn_streak`) PostToolUse adds one line: the streak, its kinds and the brief command
  for the work it looks like. At 15 (`proxy.deny_streak`) PreToolUse denies the next counted call with
  that line. The source of both numbers is section 1: the warning would have fired 145 times, and 42
  streaks reached 15.
- Hands-on landing and lane edits are denied from the first call: `safe-push --to`, `north land
  enqueue`, `git commit|push|rebase` and writes inside a lane worktree, because a worker owns its lane
  through landing. G1 first moves `north cloud land` to the cloud courier, since today the launching
  session runs it (`harness/skills/workers/references/cloud.md:33`).
- Always allowed, tested in the denied state: delegate and brief verbs, child messages, `north gate`,
  `north config agents off|on`, `north land-abandon` and ticket removal, `north fleet stop|forget`.
- Escape: `north proxy incident REASON` grants a budget of 30 calls (`proxy.incident_budget`) for an
  incident the proxy must work itself, and writes a metric row with the reason, so the escape is
  counted rather than a persistent switch-off.

**Recurring work becomes code**: the classifier reduces each counted call to a shape (the verb, or
the first two words of each pipeline stage after any `cd`), and the readout files one issue per
counted shape the proxy ran 20 or more times in a day. Delegating verbs are not counted kinds, so
`north fleet launch` never files one. In the measured day the rule names `north fleet show` (75), the
land queue directory (101), `merge-base --is-ancestor` (47) and the lander's journal (57).

Enforced by: hook for the streak, the landing denial and the exemptions; types for verb kinds;
metric with an alarm for recurring work. "Predictable answer" stays text; the streak bounds what
misjudging it costs.

### 2.6 The readout and its alarm

`north ops proxy` writes one row a day to the threads database (`~/.local/state/threads/threads.db`):

- calls by kind from the proxy's transcript, split into streaks a `Tom` message started and streaks
  the proxy started on its own (diagnosis Tom asks for is not the proxy's failure);
- research subagents launched and `north proxy incident` escapes;
- turns woken by fleet messages, by sender kind;
- landed boxes, counted from `Box:` trailers on commits that reached a main branch that day (section
  4), so every landing path counts, not only the lander's;
- **proxy hands-on calls per landed box**, the self-started counted calls over landed boxes. With no
  landed box that day the ratio is not computed and the alarm does not read the day.
- **Tom's corrections per day**, read from the outcome loop's count from the grounding ledger once that
  lane lands; until then the row leaves it empty. A lexical classifier over Tom's messages is rejected:
  the measured day matched 54 of 116 messages, which is noise, and the standing-order line count is
  written by the proxy itself, so it would grade its own work.

One alarm: hands-on calls per landed box above `proxy.hands_on_per_box` two days running opens or
updates one issue and sends the proxy one line. It and the repeat-shape rule are detectors in the
self-detectors filer once that lane lands, rather than a second issue filer; if the experiments judge
has landed first, the thresholds of 2.5 and this alarm each carry its registry entry. The threshold's
first value comes from the first week of rows, because the commit counts of section 1 (336 commits)
overstate boxes several times.

Nothing runs these today: no host schedules `north ops check`, Nexus schedules only `north tune`, and
gex45 has only `north-watch` and `north-runtime-follow` timers. Box T1 adds both timers to Firn and is
done only when a row is written and the check runs on its schedule.

Enforced by: metric with an alarm, on a running timer.

## 3. The five dropped proxy rules

South's `agents/rules.toml` registers five rules written for the proxy whose placement is `dropped`:
the three in its `Proxy` section (`status-in-toms-terms`, `ground-before-answering`,
`check-facts-before-stating`), `know-socrates-names`, which the same role hook carried, and
`proxy-delegates-durable-work`.

Why they are dropped, from South's history:

- 2026-10-12, South `024f6a5` (the C2 policy cut) gave every rule a placement; these five were
  `hook:org-role`, injected by the org-role hook into the session it recognised as the proxy.
- 2026-10-16, South `5684064` removed the supervisor and mailbox and with them the org-role hook and
  its role registrations. The five flipped to `global`, but no bullet for them was added to
  `agents/AGENTS.md`, so no session received them.
- 2026-10-29, South `c3c795e` (north#351): `north agent-config-check` requires each `global` rule to
  have its bullet, so these five were marked `dropped` to make the register match the loaded policy.

So since 2026-10-16 the proxy has had none of them. The cause is structural, not editorial: role
policy lost its only home when the harness stopped being able to tell the proxy apart from other
sessions, and today it still can only by absence (no fleet record, `CLAUDE_CODE_ENTRYPOINT=cli`, the
test `harness/hooks/prompt-grounding.sh` uses). The role identity in section 2 gives role policy a home
again, and a new placement kind gives the check something to verify.

Disposition, each with what enforces it:

- `proxy-delegates-durable-work`: **placed** as `hook:proxy-delegation`, with the text rewritten to the
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
- `know-socrates-names`: **placed** as `agent:proxy`, a new placement kind meaning "this rule's text
  is in `harness/claude/agents/NAME.md`". Only the proxy reads Tom's voice-transcribed messages, so the
  names belong in its role file, not in every session's global rules. Its text also carries a typo
  (`Tom.s workforce is coordinated`) fixed in the same change. Enforced by `north agent-config-check`:
  for every `agent:NAME` placement it checks that the role file exists and contains the rule's text,
  so a placement can no longer silently lose its home the way `hook:org-role` did.

## 4. Post-landing bookkeeping, done by the landing

Today the lander checks with `git cherry` on the ticket head before it messages
(`crates/north-landing/src/land/after/mod.rs:70`) and tells the ticket's session; nothing ticks a box,
closes an issue or retires a session. And only north lands through the lander: makgora's
`.safe-push` sets `queue = lock`, and cloud lanes are enqueued by whoever runs `north cloud land`, so
a ticket field would miss most landings. The box therefore travels in the commit.

1. **The box is a commit trailer.** A lane's commit carries `Item: REPO#N` and `Box: TEXT` trailers,
   the text of the Done when box it closes; `north brief new` prints both for the worker to copy, and
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
landed-ancestor checks. The proxy keeps doing them by hand, they count in its streak, and those boxes
come early because they remove the most proxy calls per minute of work.

## 5. Boxes, in landing order

Each box lands through `north land enqueue` (or safe-push in South and Firn), adds the
`docs/features.md` entry the features-index check requires for any verb it adds, and is done only when
its sha is an ancestor of the repository's `origin/main`. Prose in `docs/harness.md` and the workers
skill waits for D1, so 19 boxes do not all edit two hot files. Boxes whose Needs have landed run in
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
   CLI dispatch table under `src/cli/`, `crates/north-core/src/proxy/` (`calls.rs`, `shape.rs`), a
   `north proxy calls TRANSCRIPT` readout. Needs: nothing. Done when: `cargo test -p north-core proxy::`
   passes a truth table with a case per kind and per foreign-command rule, and a test that fails a verb
   with no declared kind; on the laptop `north proxy calls` over the measured transcript prints the
   section 1 counts within the difference the verb table makes, recorded in this page.
3. **R1 role on the session record.** feature, cross-module, rust, local-test; 45 min. Files:
   `crates/north-core/src/fleet/model.rs`, `src/cli/fleet/launch.rs`, `src/cli/fleet/lineage.rs`,
   `crates/north-core/src/fleet/registry/launching.rs`, `crates/north-core/src/providers/claude/session_env.rs`,
   `crates/north-core/src/providers/claude/runs/mod.rs`, `crates/north-core/src/providers/ledger_rows.rs`,
   `crates/north-guard/src/guard/lead_timer.rs`. Needs: the staffing-overlap lane, which edits the same
   launch files. Done when: `cargo test -p north-core fleet::` passes cases where `--role proxy`
   records `Proxy` and exports `NORTH_FLEET_ROLE=proxy`, a task launch records `Worker`, a second live
   proxy is refused without `--take-over`, and the ledger and lead timer read the field.
4. **L1 `Item:` and `Box:` trailers, checked before the push.** feature, cross-module, rust,
   local-test; 40 min. Files: `north brief new`, `north land scan` in `crates/north-landing/src/land/scans/`,
   `north cloud land`, `harness/skills/workers/references/brief.md`. Needs: the incidents-by-construction
   lane, which edits the lander. Done when: `cargo test -p north-landing land::scans` passes cases where
   a `Box:` matching no unticked box is refused, one matching passes, and `north cloud land` keeps both
   trailers.
5. **B1 one writer for issue bodies.** feature, module, rust, local-test; 45 min. Files: a new
   `issue_body` module beside `crates/north-landing/src/issue_box_lint.rs`, a durable outbox under the
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
   30 min. Files: `harness/claude/agents/proxy.md`, `crates/north-core/src/agent_config_check/rules.rs`.
   Needs: nothing. Done when: `cargo test -p north-core agent_config_check` passes a case where an
   `agent:proxy` rule whose text is absent from `proxy.md` is refused and one where it is present
   passes, and `proxy.md`'s tools carry `Agent(Explore, Plan)`.
9. **R2 role prompt and grounding.** feature, cross-module, rust, local-test; 30 min. Files:
   `crates/north-core/src/fleet/prompt.rs`, `harness/hooks/prompt-grounding.sh`,
   `crates/north-core/src/fleet/audience.rs` (the hook's audience, passed into `north grounding hook`
   by the CLI root). Needs: R1, W1. Done when: `cargo test -p north-core fleet::audience`
   passes cases where a role-proxy session gets the block for a `Tom` message and not for a `Service`
   or `Session` one and a worker gets none, and `cargo test -p north-core fleet::prompt` shows the
   proxy preamble.
10. **G1 the delegation guard.** feature, module, rust, local-test; 45 min. Files:
    `crates/north-guard/src/guard/proxy_delegation.rs`, `crates/north-core/src/hook.rs` (`PreOther`),
    `harness/defaults.toml` (`proxy.warn_streak`, `proxy.deny_streak`). Needs: R1, P2, W1. Done when:
    `cargo test -p north-core guard::proxy_delegation` passes the 5th counted call warning, the 15th
    denied, each exemption passing while denied, a `ListAgents` neither counting nor resetting, a
    delegate call and a `Tom` message resetting, a call with an `agent_id` not counting, two parallel
    writers under the lock, a test per `PreOther` tool, and a worker session untouched; with its
    experiments registry entries if the experiments judge is on main.
11. **G2 the landing denial and the incident escape.** feature, module, rust, local-test; 40 min.
    Files: the guard, `north cloud land` and the cloud courier, a `north proxy incident` verb. Needs:
    G1. Done when: `cargo test -p north-core guard::proxy_delegation` passes a first-call denial of
    `safe-push --to` and of a write inside a lane worktree, `north proxy incident REASON` granting 30
    calls and writing its row, and a cloud lane landing with no `north cloud land` from the launching
    session.
12. **S1 South: place the five rules.** Repository: south, laptop. docs-policy, one-file, docs,
    local-test; 20 min. Files: `agents/rules.toml`. Needs: A1, G1, R2 on the runtime. Done when:
    `north agent-config-check policy-gate session` exits 0 on the South lane with the placements of
    section 3 and the typo fixed.
13. **R3 `north proxy`.** feature, cross-module, rust, local-test; 45 min. Files: `src/cli/` (a
    `proxy` verb), `crates/north-tui/src/tui/fleet/` (model and effort slash commands),
    `crates/north-core/src/providers/claude/fleet.rs` (`--agent`), `crates/north-core/src/fleet/registry/takeover.rs`.
    Needs: R1, R2, A1, W1. Done when: an integration test under `tests/it/` shows `north proxy` with no
    proxy launching one with role proxy, `--parent none`, cwd `~/code/north/main` and `--agent proxy`
    in the fake Claude's argv; with a live proxy it attaches without launching; with the old `proxy`
    and `proxy-gex45` records present it archives both and delivers one digest line;
    and `north proxy --interactive` stops the fleet proxy before it execs `claude --resume` on the same
    conversation, and refuses while the fleet session still runs.
14. **R4 the proxy's digest.** feature, module, rust, local-test; 40 min. Files:
    `crates/north-core/src/fleet/registry/messages.rs`, `crates/north-core/src/fleet/receipts.rs`,
    `harness/defaults.toml` (`proxy.digest_minutes`). Needs: W1, R1. Done when:
    `cargo test -p north-core fleet::` passes cases where five receipts within 10 minutes reach the
    proxy as one message, a red-main notice, a gate, a decision stop and an alarm pass at once, and a
    `Tom` message is never held.
15. **F1 Firn: Tom's proxy window runs `north proxy`.** Repository: nixos-config, laptop. mechanical,
    one-file, nix, local-test; 30 min. Needs: R3 on the runtime and Tom's answer to the gate below.
    Done when: `rg -n 'north proxy' ~/code/nixos-config/main` names the entry, and after the rebuild
    the box drives the real `north proxy` against `claude -p`: a typed message gets a reply, `/model`
    relaunches on the new model, and a forced compaction leaves the session answering. Escape hatch, if
    the fleet TUI fails Tom: `north proxy --interactive` (built in R3) stops the fleet proxy and resumes
    the same conversation in Claude Code's own UI with `claude --resume <session-id>`, never with both
    running; the box drives it once and sees the fleet record stopped and the resumed window answer.
16. **M1 the daily readout.** feature, module, rust, local-test; 45 min. Files:
    `crates/north-core/src/ops/proxy/`, the threads database schema. Needs: P2, L2. Done when:
    `cargo test -p north-core ops::proxy` on a synthetic transcript fixture (no Tom text) gives calls by
    kind split by who started the streak, research subagents, escapes, wakes by sender kind, landed
    boxes from trailers, the ratio, the no-box day with no ratio, and an empty corrections field until
    the outcome loop is on main.
17. **M2 the alarm and the repeat rule.** feature, module, rust, local-test; 40 min. Files: the
    self-detectors module, `harness/defaults.toml` (`proxy.hands_on_per_box`, `proxy.repeat_shape`).
    Needs: M1 and the self-detectors lane. Done when: its tests pass two days over the threshold
    opening one issue and sending one line, one day over opening none, and a counted shape run 20
    times filing one issue that the next day updates; with its experiments registry entry if the
    experiments judge is on main.
18. **T1 the timers.** Repository: nixos-config, laptop. mechanical, one-file, nix, local-test; 30
    min. Needs: M1. Done when: `systemctl --user list-timers` on the ledger host lists the
    `north ops check` and `north ops proxy` timers, and the threads database holds a `north ops proxy`
    row written by the timer.
19. **D1 the docs.** docs-policy, cross-module, docs, none; 30 min. Files: `docs/harness.md`,
    `harness/skills/workers/SKILL.md`, `harness/skills/workers/references/cloud.md`. Needs: R3, G2, L3.
    Done when: `rg -n 'north proxy|Box:|Next:' docs/harness.md harness/skills/workers/SKILL.md`
    finds the proxy's entry point, the trailers and the report line.

Order by payback: W1, P2, A1 and B1 start at once; R1 and L1 as soon as the lanes they collide with
land; L2 and L3 next, because bookkeeping is the cheapest large cut; then R2, G1, G2 and S1; R3, R4
and F1 make the window Tom uses the fleet proxy; M1, M2 and T1 measure what the others changed; D1
closes.

F1 changes the window Tom types into, so it waits on a gate to him. Recommended: his proxy window
opens north's fleet TUI on the `proxy` session, so every receipt, landing and alarm reaches the
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
- The standing-order line count as the corrections signal: the proxy writes those lines, so it would
  grade its own work. The outcome loop's count from the grounding ledger is used once it lands.
- Moving every docs line into one closing box: a box that adds a verb still adds its
  `docs/features.md` entry, because the features-index check requires it in the same landing.
- Committing the measured transcript as P2's fixture: it holds Tom's messages and north is public, so
  P2 commits a synthetic truth table and reproduces section 1 on the laptop.
