# pstack's author's lead-role bot against North's coordinator (2026-11-12)

Record for the followed source [pstack](../../references.md#pstack): the author published a lead-role bot, which is
the outer loop North's proxy plays, and North had not compared the two (north#1260). Made by the method in
[watching.md](../../../harness/skills/agent-policy/references/watching.md). The previous cycle is
[2026-11-04-pstack.md](2026-11-04-pstack.md), and the out-of-cycle record before this one is
[2026-11-10-pstack-verification.md](2026-11-10-pstack-verification.md).

## What we looked at

- The bot's public listing, "Engineering Lead" by Lauren Tan, at `x.ai/bot/marketplace/bots/engineering-lead`,
  fetched once on 2026-11-12 as one HTML page and read as data. Its record carries `updatedAt` 2026-09-25 and no
  version. It lists three memories (the bot's stated rules) and five skills: onboard, decompose, chase,
  PR attribution and digest. Each skill's published content equals its one-line description, and the routines
  list is empty, so the playbooks behind the skills are not public and were not read. No download or import link
  was followed and nothing was installed or run. Terms: the page is under the site operator's terms of service; no
  open license is stated for the bot, so nothing from it may be copied, and none of its text is quoted here.
- pstack in `cursor/plugins` under `pstack/`: fresh blobless clone into an empty scratch directory, read as data.
  Repository head `2eaf465` (2026-11-11). The diff from `519d36a` to head under `pstack/` is the two commits the
  2026-11-04 cycle already read (`ab1f702`, `c742dd6`, both 2026-10-27); nothing under `pstack/` changed since.
  License MIT (`pstack/LICENSE`). The bot's skills are not in this repository: no file names onboard, chase,
  digest or attribution, and the nearest relative is pstack's agentic `orchestrate` playbook as rewritten in
  `c742dd6`. The separate `orchestrate/` plugin (MIT, Cursor) is unchanged since 2026-08-11.
- North at origin/main `b103d2a79`: the proxy prompt (`harness/claude/agents/proxy.md`), the workers skill, `north
  gate` and its digest, the fleet supervisor's message delivery, the page-flood fold, the fleet tick notices,
  `north graph split`, and one build host's fleet registry and run ledger for 2026-11-10 to 2026-11-12.

## What the last cycles kept, live or not

- Run-proven review findings (2026-11-04, E1): kept, live in the workers review reference.
- Landing model and five properties (2026-11-04, E2): the model was later deleted (see the 2026-11-10 record);
  the five properties stand.
- No verifier for wording-only changes (2026-11-04, E3): its rerun on 20 landings after the boundary is still not
  recorded on main, past the 2026-11-08 date that record gave. That is a finding: north#524 owns it.
- Formal-verification and statechart skills: answered by `north check machines` (2026-11-10); pstack's skills are
  unchanged since, so there is nothing new to ask of them.

## The bot's rules and skills against North

- Own the loop: decompose, run, chase, report. Live. The proxy prompt makes the proxy the one session that turns
  a request into briefs and judges reports (`harness/claude/agents/proxy.md`); `north fleet launch` and
  `north fleet dispatch` run the workers.
- Done means merged, explicitly abandoned or taken back, and starting an agent is never done. Partial. Landed is
  the sha seen on origin/main (`mcp__north__finish` and `safe-push --wait` return `landed`), and reports must begin
  `Done:`, `Split:`, `Not done:` or `Blocked:` with a `Stop:` line. North has no explicit abandoned or taken-back
  verdict on a box; a box stays open until ticked or its issue closes (E3 below tests whether that costs anything).
- Stay quiet while healthy; reach the owner only when blocked, ready or finished; batch the rest. Partial. For the
  owner it is live: `north gate open` carries a recommendation and a default, and `north gate digest` prints the
  open gates. For the coordinator the proxy prompt states the rule, and the supervisor holds `Done:` reports and
  interim turns for the hourly `north work --since 1h` digest (`crates/north-fleet-supervisor/src/fleet/registry/messages.rs`,
  the interim-turn test) while the page-flood fold caps one service at three messages in ten minutes
  (`crates/north-fleet-store/src/flood.rs`). But the lander's `landed` notice is handed in to the proxy on every
  landing (the same test, "a landing notice is no alarm"), and coordinator briefs ask each worker to send
  `landed <sha>` to the proxy as well, so the coordinator is woken per commit. origin/main took 78 commits on
  2026-11-12 by mid-afternoon UTC.
- Steer an existing agent with a follow-up before starting a fresh one. Absent, and reversed by rule:
  `harness/skills/workers/SKILL.md` line 21 forbids reviving a finished worker by message because its context is
  spent, and the standing orders forget each child once its report is read. `north fleet revive` exists only for
  sessions stranded by a capacity or harness stop (`crates/north-dispatch/src/dispatch/revive.rs`).
- Onboard (repos, tracker, review norms, bot identity, digest channel, then the routine). Prose only, and little
  needed: North serves one owner, and its equivalents are configuration in the owner's private repository and the
  followed-source and fleet timers; there is no first-run command.
- Decompose (a request into a few independently shippable tasks, then launch and track). Partial.
  `north graph split --base SHA --task FILE` landed in `41bf60a59` (north#1236) and writes collision-free boxes,
  but the binary installed on the build host read here predates it (`north graph split --base` answers
  `unknown flag`), and briefs are still split by the proxy by hand.
- Chase (one pass over every tracked agent and change: unstick, escalate only what needs a human, silent when
  healthy). Live. The fleet tick's quiet note goes to a parent once when a child stops with nothing to show or goes
  quiet for twice its ETA (`crates/north-fleet/src/fleet/silent.rs`), the flow stall alarms run under
  `north ledger flow --stall-alarm`, and `north fleet dispatch` revives stranded sessions. North's pass is per
  event, not on a weekday cadence, which suits a fleet running at all hours.
- PR attribution under a bot identity, with the requesting human as co-author. Partial and inverted: issues and
  pushes go through the fleet's GitHub App token, while commits are authored as the owner with no co-author
  trailer. With one owner there is no second human to credit, so this is left out.

## Ranked: what North lacks

1. A finished session takes its own follow-up. Basis, measured: on one build host's ledger from 2026-11-10 to
   2026-11-12, 4 of 15 task sessions were fresh launches on an issue whose earlier session had exited 2 to 377
   minutes before (north#1054, north#1063, north#1076, north#902); three followed a `Done:` report that named the
   gap the follow-up closed. Each successor spent 75k to 204k input and cache-write tokens rebuilding context the
   predecessor held, and the coordinator wrote a fresh brief each time. The coordinator counted three such
   follow-ups on the same day. Expected effect: usage per closed box and first-try success on follow-ups. Against
   it, reasoned: a long finished context can carry stale assumptions, which is why the workers rule exists, so it
   goes to an A/B rather than straight in.
2. Landing notices leave the coordinator's wake path. Basis, measured in code: the supervisor test above pins
   one wake per lander notice, and briefs add a second; 78 landings by mid-afternoon on 2026-11-12. Expected
   effect: coordinator attention and usage, and the latency of the decisions it is woken for instead.
3. An explicit abandoned verdict for a box. Basis, reasoned: the bot's done rule counts abandonment as an end
   state, and North's dispatch keeps staffing any open box. Expected effect: small unless open work goes stale.

Dropped below the bar: onboard (one owner, configuration already in place), PR attribution (no second human),
the weekday cadence (North's work does not keep office hours, and its per-event notices are live), and decompose
(North has `north graph split` landed; installing it on every host is deployment, not a new mechanism).

## Experiments

- E1, a finished session takes its own follow-up: north#1261. Frozen input: the next 10 follow-up briefs on an
  issue whose last session reported `Done:` within 24 h, alternately to a revive of that session and to a fresh
  worker at the same tier. Metric: API-equivalent spend from the follow-up's first message to its landing. Keep if
  the revived arm's median is at most the fresh arm's with no more `Not done:` reports or reverts. Not run here: it
  needs ten live follow-ups, far over 30 minutes.
- E2, landing notices to the digest: north#1263, run on 2026-11-12 on the laptop. Frozen input: every message
  delivered to the proxy on 2026-11-12 UTC up to 05:46, the end of the proxy's transcript when it was read (the
  `queue-operation` rows of conversation `2a5be8f1` under the claude-a profile; the held log held 0 landing
  notices). Metric: landing-notice wakes after which the proxy acted within 10 minutes on something the hourly
  digest would not have shown in time. Keep the move if that count is 0. Result: 509 messages from the fleet were
  delivered; 36 came from the lander and none was a landing notice (the lander tells the lane's owner, not the
  proxy, so the half of the move that touches `landed BRANCH as SHA` has no wakes to remove); 70 were a worker's
  `landed <sha>` send, 69 of them opening with a sha. After 41 of those 69 the proxy sent or launched, within 10
  minutes, a message naming that sha to a session other than the sender: a read request to the independent
  reader of the landed commit, or a launch of a dependent follow-up box. The count is 41, not 0. Verdict: drop;
  the wakes stay and no code is built. The 10-minute window cannot show how many of those 41 would have been
  asked for anyway an hour later; a rerun would compare reader latency with and without the wake.
- E3, an explicit abandoned verdict: run on 2026-11-12. Frozen input: the open north issues labelled
  `priority:now` or `priority:next` at 2026-11-12, read with `gh issue list --state open --limit 1000`. Metric: the
  share not updated in 14 days. Keep (build the verdict) if at least 5%. Result: 0 of 115 (68 now, 47 next); the
  oldest update among all 153 open issues is 2026-11-10. Verdict: revert; nothing is built. The metric counts the
  fleet's own edits as activity, so it cannot show work going stale under automated touches; a rerun would read the
  last session launched per issue instead.

## Decided

- Ideas only; nothing copied from the bot or the plugin.
- The bot's rules are ones North already states, except steering a finished session, which North forbids by rule.
  The measured cost of that rule makes it the first experiment (north#1261), and the per-commit wake the second
  (north#1263). This cycle owns both issues through their verdicts.
- Next review of this source: from `2eaf465`, when it next comes due under watching.md. The bot's listing has no
  revision, so its next read compares the listing's `updatedAt` with 2026-09-25.
