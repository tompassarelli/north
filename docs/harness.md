# How the harness works

This document follows the harness end to end: how it is installed, where its policy lives, how a
worker is staffed and how a change lands. It names each step and links to the file that owns it, so
values and flags are read there, not here. What each command does in detail is in
[features.md](features.md); the words follow [terms.md](terms.md).

North is the harness layer. Socrates, the workforce, is the set of agents that run on it: Tom's
top-level session (the proxy), a lead per domain, sub-leads and workers, as `agents org show`
prints. Three repositories take part:

- **north** (this one, public): the harness commands in [bin/](../bin/), the hooks, skills and
  defaults in [harness/](../harness/), the shared procedures in
  [agent-machinery/](../agent-machinery/), and the `north` app.
- **south** (private): Tom's operator policy, read from `~/code/south/main/agents` unless
  `AGENTS_OPERATOR_DIR` names another directory.
- **firn**, the `nixos-config` repository: Tom's machine configuration, which puts the harness on
  each machine. Its contents are outside this repository; claims about it below are marked.

## Install

The full harness assumes Tom's layout and South, so it is not a public install. On Tom's machines:

1. Each repository is a container `~/code/<project>/` with `main/`, `worktrees/<slug>/` and
   `pins/<object-id>/` ([harness/code/AGENTS.md](../harness/code/AGENTS.md)). North's commands
   expect `~/code/north/main` and `~/code/south/main`.
2. Firn puts [bin/](../bin/) on `PATH`. Not verified here: the only trace in this repository is the
   `~/.local/share/north/bin` path in [harness/policy-owners.toml](../harness/policy-owners.toml) and
   [harness/scripts/agent-policy-contract.py](../harness/scripts/agent-policy-contract.py).
3. `north-runtime-update` builds origin/main in a detached worktree, installs the binary under
   `~/.local/state/north/runtimes/<sha>/`, points `~/.local/state/north/current` at it and runs
   `agents sync`, rolling back if that fails. `agent-runtime-update` runs it in its nightly pass. The
   `north` command in [bin/north](../bin/north) launches that installed runtime.
4. `agents sync` publishes policy. It fast-forwards the south and north main checkouts, runs the
   policy gate in [harness/scripts/agent-config-check.sh](../harness/scripts/agent-config-check.sh),
   then `north config agents sync` builds a new activation generation under
   `~/.local/state/north/agents/` from South's `catalog-config.json` and links each allowed skill into
   `~/.claude/skills` and `~/.codex/skills`. The generation's instructions and `provider-hooks` reach
   Claude Code and Codex through links Firn's profile module makes; `agent-config-check.sh` checks
   that wiring.

To try only the `north` app, follow the [README](../README.md#try-it); it needs neither South nor
Firn.

## Where policy lives

Policy is the rules agents follow and the checks that hold them to it. Each kind has one home:

| Policy | Lives in | Reaches agents through |
| --- | --- | --- |
| Global agent rules | `south:agents/AGENTS.md`, each bullet registered in `south:agents/rules.toml` | `agents sync` |
| Rules for `~/code` | [harness/code/AGENTS.md](../harness/code/AGENTS.md) | `agents sync`, linked as `~/code/AGENTS.md` |
| Rules for one repository | that repository's `AGENTS.md`, such as [this one](../AGENTS.md) | the agent's harness reads it in the checkout |
| Procedures | skills in [harness/skills/](../harness/skills/) and [agent-machinery/skills/](../agent-machinery/skills/) | `agents sync` links them |
| Guards on tool calls | hooks in [harness/hooks/](../harness/hooks/), bound to events by [harness/policy-owners.toml](../harness/policy-owners.toml) | the generation's `provider-hooks` |
| Claude worker definitions and Codex prompts | [harness/claude/agents/](../harness/claude/agents/) and [harness/codex/prompts/](../harness/codex/prompts/) | not traced in this repository; `agent-config-check.sh` reads the installed copies in `~/.claude/agents` |
| Thresholds and priors | [harness/defaults.toml](../harness/defaults.toml), each with its source | read by the commands that use them |
| Models, prices and benchmarks | [harness/model-catalog.toml](../harness/model-catalog.toml) | read by routing |
| Staffing tiers, accounts, project shares and targets | `south:agents/orchestration.toml` | read by `agents plan` and the spawn gate |
| Recurring routines | `south:agents/routines/` | `agents routines` |

A policy change lands like any other change. `agents sync` refuses to publish policy that fails its
gate, including a measured drop in replay conformance against `south:agents/operating-point.toml`; the
"Policy regression gate" entry in [features.md](features.md) gives the rules.

Harness landing uses the workforce role, independently of provider or model. Proxy, lead and
sub-lead roles may review and land launcher and other durable harness changes; workers commit for
their lead. `safe-push --harness-role` resolves the registered session or org node, then `AGENT_ROLE`;
an unregistered top-level session is the proxy. Assign roles through `agents org add` or
`agents org set`. Both `safe-push` and the Codex hook use this same role resolution.

## Staffing a worker

A lead staffs workers; the [workers skill](../harness/skills/workers/SKILL.md) is the procedure and
holds the tier table. In order:

1. **Pick the work.** `threads unowned` lists priority items nobody holds, `threads ready` lists
   ready work, and `threads show REPO#N` reads an item's Done when boxes. A held item is never
   duplicated.
2. **Pick the provider and account.** `agents plan` reads the signed-in providers, South's
   `orchestration.toml` and live usage, and routes each tier to an account.
3. **Pick the tier and ETA.** `worker-ledger --recommend` picks the cheapest tier whose measured
   success meets the threshold in [harness/defaults.toml](../harness/defaults.toml) and prints the
   ETA.
4. **Write the brief.** It starts with `Delegation: role=<worker|sub-lead> budget=<n>` and carries
   Item, Category, the facet lines, files, Done when and ETA. A retry or follow-up adds `Follows:`.
5. **Spawn through the gates.** For Claude workers,
   [spawn-capacity-guard](../harness/hooks/spawn-capacity-guard.sh) refuses a spawn at delegation
   budget zero, when capacity leases hold the machine, past the headroom kept for Tom's first-priority
   domain, or past the account's pace line without an approved deadline (rules in
   [spawn_capacity.py](../harness/hooks/lib/spawn_capacity.py));
   [worker-tier-guard](../harness/hooks/worker-tier-guard.sh) checks the category
   and facets against the tier. A Codex lead spawns Codex workers with `spawn_agent`, setting model
   and effort explicitly.
6. **Queue what waits.** A box that waits on another lane is queued with `threads queue follow`;
   `follows-release` starts it once its predecessor lands on main.
7. **Watch and record.** `worker-sweep --wait` flags stalled, overtime and parked workers;
   `worker-ledger` records each run's outcome, cost and duration, and those records feed the next
   recommendation ([feedback-loop.md](feedback-loop.md)).

## Events between agents

Agents report to each other through `agents event`, not by appending lines to status files that
nothing turns into a turn ([harness/lib/events.py](../harness/lib/events.py); `agents event --help`
is the contract):

1. `agents event KIND [ITEM] --from-node NAME --to-node NAME --body TEXT` records a row (KICKOFF,
   STUCK, QUESTION, LAND-READY, LANDED, RECYCLE-READY with `--packet FILE`, or ANSWER) and mirrors it
   to the sender's status and recipient's inbox. Names default only from the caller's exact registered session and explicit org parent; shared
   server tools without that identity must supply both names. All rows live in the canonical threads SQLite; an offline
   publication retains its stable key in the existing private argv spool and reports deferred.
   Pending reads fail visibly while the canonical ledger is unavailable.
2. `agents supervise`, a systemd user service that Firn's `agents-supervise` module runs, passes
   over the table every `event_poll_s` seconds ([harness/defaults.toml](../harness/defaults.toml)). It
   delivers each undelivered, unanswered row to a Claude node by appending its inbox and writing one
   `{"type":"user",...}` line to the session socket whose process argv holds the node's session id,
   or, for a Codex node, through `codex-lead send` on its thread, and marks it delivered. Claude
   leads launch with `--settings '{"crossSessionInbound": "accept"}'`; delivery to a session whose
   argv or settings.json lacks it fails loudly instead of writing to a socket that drops it. A failed delivery is
   retried on the next pass. All state is in the table, so restarting the service loses nothing.
3. `agents event ANSWER --from-node NAME --to-node SENDER --answer ID --body TEXT` from the target
   or proxy resolves that exact row and its linked root atomically. Separate work on the same item
   stays unanswered. A recorded SHA verified on origin/main resolves its LAND-READY event.
4. Stall rules, each once per row, from the thresholds in defaults.toml:
   - a STUCK or QUESTION unanswered for `event_stuck_resend_min` minutes is re-sent to its target
     and raised to the proxy as a linked QUESTION row;
   - a KICKOFF, QUESTION or STUCK ask with no ANSWER and no new owner status line (`ACK <id>`)
     after `event_ack_min` minutes is re-pushed once, and after twice that raises one ALARM row to
     the proxy;
   - a KICKOFF or QUESTION ask is owned by its target node; one still unowned, or whose owner has
     written no status line and started no worker since it was sent, after `event_ask_progress_min`
     minutes raises one ALARM row to the proxy;
   - a LAND-READY lane whose sha is not on the repository's origin/main after `event_land_page_min`
     minutes pages the parent, and after `event_land_proxy_min` minutes the proxy;
   - a RECYCLE-READY with a packet and no answer after `event_recycle_restart_min` minutes stops the
     sender's session and runs `agents lead restart NAME --packet FILE` with a fresh session.
     The launcher refuses until org and native children are settled; it leaves the old session
     alone on failed admission and records failed attempts for retry. A matching packet and an
     already-created successor are reused after a service crash;
   - failed or missing recipients remain pending, with their delivery error retained, and are
     escalated by the same deadline rules. No unknown or unique socket is guessed to be the proxy.

`agents event` refuses, with a non-zero exit naming the reason, an event addressed to a node that
is removed from the org, finished, has a status file whose last line starts with FINISHED, or has a
dead session. The refused event is still stored, with `successor` set to proxy, and a linked copy to
the proxy is what gets delivered, so nothing waits silently in an inbox nobody reads.

`agents event list` prints unanswered rows and their delivery state. Pace controls use this same
publisher while preserving their inbox and ntfy records; there is no second direct turn-send path.

## Landing a change

Agents edit in `~/code/<project>/worktrees/<slug>`, never in `main/`, and land with
`safe-push --to main` ([bin/safe-push](../bin/safe-push); `safe-push --help` is the full contract;
the [repo-safety skill](../harness/skills/repo-safety/SKILL.md) is the procedure). It:

1. rebases the lane onto origin/main and takes the per-origin landing lock; lanes queued behind the
   holder land together as one train;
2. runs the checks that the repository's committed `.safe-push` maps to the changed paths
   (`land.check` entries; [this repository's](../.safe-push) cover the Rust host, the harness scripts
   and agent-machinery), or the pre-push hook when there are none;
3. refuses the push unless every gate passes, among them a gitleaks secret scan, the dependency gate
   (`north dep-gate BASE HEAD`), a `Provenance:` record for each new
   gating number (`provenance check`), a `Test-kind:` claim for each new test naming a recorded mutant
   only it kills, and the CI time budgets in `.safe-push`; it never force-pushes;
4. pushes to main, removes the clean lane and its branch, and runs `follows-release`.

A repository with an Autoland workflow, or `landing.queue = autoland` in its `.safe-push`, instead
gets a `claude/land-*` branch that the workflow lands; North has neither. A worker runs `safe-push` in
the background and reports when it exits; a lead runs it in the foreground.

After the push, CI runs on main ([.github/workflows/](../.github/workflows/)). When a run fails,
the "Main is red" workflow re-runs the failed job once, and if the same step fails again after a
green parent it reverts the culprit commits and reports on a "main is red" issue
([harness/scripts/main-red.ts](../harness/scripts/main-red.ts)).
