# How the harness works

This document follows the harness end to end: how it is installed, where its policy lives, how a
worker is staffed and how a change lands. It names each step and links to the file that owns it, so
values and flags are read there, not here. What each command does in detail is in
[features.md](features.md); the words follow [terms.md](terms.md).

North is the harness layer. Socrates, the workforce, is a flat set of fleet sessions coordinated by
Tom's proxy. A parent is each session's `--parent` link; `north fleet tree` shows the org. Three
repositories take part:

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
3. `north runtime update` builds origin/main in a detached worktree, installs the binary and the
   commit's own tree under `~/.local/state/north/runtimes/<sha>/`, points `~/.local/state/north/current` at it and runs
   `agents sync`, rolling back if that fails. `agent-runtime-update` runs it in its nightly pass. `north runtime deploy HOST...` builds on
   the machine it runs on, realises the binary's Nix store references on each HOST, copies the programs into that
   HOST's `runtimes/<sha>/` and runs `north runtime update` there, which finds them built and only promotes. The
   `north` command in [bin/north](../bin/north) launches that installed runtime, and scheduled jobs run
   harness code from `current/tree/bin`, so the code they run and the binary they call share a commit.
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

Harness landing is independent of provider or model. Every fleet session uses the same `safe-push`
checks before a landing.

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
4. **Write the brief.** Carry Item, Category, facet lines, files, Done when and ETA. A retry or
   follow-up adds `Follows:`.
5. **Launch through Fleet.** Run `north fleet launch --provider claude --account A --parent NAME`
   with the brief as its system prompt. Fleet records the parent link and controls delivery. The
   launcher enforces placement before anything starts: it prints one verdict line per Claude account,
   refuses a code-only public box with the exact Nexus `north cloud launch` command while the
   cloud credit lasts, and refuses an account other than the one with the most headroom, naming it.
   `--local "REASON"` and `--pin "REASON"` override and are recorded on the session and shown in
   `north fleet list`; `north fleet revive` re-checks a stopped session and keeps those reasons.
6. **Queue what waits.** A box that waits on another lane is queued with `threads queue follow`;
   `follows-release` starts it once its predecessor lands on main.
7. **Record.** Fleet receipts capture delivery and completion. `worker-ledger` records each run's
   outcome, cost and duration, and those records feed the next recommendation
   ([feedback-loop.md](feedback-loop.md)).

## Messages between fleet sessions

`north fleet send NAME TEXT...` sends a message and `north fleet ask NAME QUESTION...` waits for a reply. Use `--parent NAME` when launching a child session so `north fleet tree` shows the relationship. A worker's final message is its report; Fleet records delivery and session completion.

## Landing a change

Agents edit in `~/code/<project>/worktrees/<slug>`, never in `main/`, and land with
`safe-push --to main` ([bin/safe-push](../bin/safe-push); `safe-push --help` is the full contract;
the [repo-safety skill](../harness/skills/repo-safety/SKILL.md) is the procedure). It:

1. obtains the capacity scope named by committed `landing.capacity` before joining the landing
   queue, then rebases onto origin/main and takes the per-origin lock; commits naming an open
   red-main issue or carrying `Main-red:` take priority, and queued lanes can land as one train;
2. runs the checks that the repository's committed `.safe-push` maps to the changed paths
   (`land.check` entries; [this repository's](../.safe-push) cover the Rust host, the harness scripts
   and agent-machinery), or the pre-push hook when there are none;
3. refuses the push unless every gate passes, among them a gitleaks secret scan, the dependency gate
   (`north dep-gate BASE HEAD`), a `Provenance:` record for each new
   gating number (`north provenance check`), a `Test-kind:` claim for each new test naming a recorded mutant
   only it kills, and the CI time budgets in `.safe-push`; it never force-pushes;
4. pushes to main, removes the clean lane and its branch, and runs `follows-release`.

North's land checks share that capacity scope. Checks without one release the landing lock before
admission, then requeue and recheck if main moved. Cancellation prints `safe-push: cancelled` and
exits 130; a landed receipt names the lane head verified on the destination.

A repository with an Autoland workflow, or `landing.queue = autoland` in its `.safe-push`, instead
gets a `claude/land-*` branch that the workflow lands; North has neither. A worker runs `safe-push` in
the background and reports when it exits; a lead runs it in the foreground.

After the push, CI runs on main ([.github/workflows/](../.github/workflows/)). When a run fails,
the "Main is red" workflow re-runs the failed job once, and if the same step fails again after a
green parent it reverts the culprit commits and reports on a "main is red" issue
([harness/scripts/main-red.ts](../harness/scripts/main-red.ts)).
