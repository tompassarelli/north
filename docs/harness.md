# How the harness works

This document follows the harness end to end: how it is installed, where its policy lives, how a
worker is staffed and how a change lands. It names each step and links to the file that owns it, so
values and flags are read there, not here: a command's `--help` for its flags,
[harness/defaults.toml](../harness/defaults.toml) for every tuned number, [.safe-push](../.safe-push)
for land checks and [features.md](features.md) for what each command is for. The words follow
[terms.md](terms.md).

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
   [crates/north-core/src/agent_policy_contract/wiring.rs](../crates/north-core/src/agent_policy_contract/wiring.rs).

3. `north runtime update` builds origin/main, installs the binary and the commit's own tree under
   `~/.local/state/north/runtimes/<sha>/`, points `~/.local/state/north/current` at it, then runs
   `north runtime health` and `agents sync`, rolling back if either fails. The `north` command in
   [bin/north](../bin/north) launches that installed runtime, and scheduled jobs run harness code
   from `current/tree/bin`, so the code they run and the binary they call share a commit.

4. Each host promotes landings itself: a timer runs `north runtime update`, `north runtime deploy`
   carries a build to the other declared hosts, and safe-push starts `north runtime follow` after a
   landing on north's main. The "Runtime promotion" entry in
   [features.md](features.md#machines-capacity-and-scheduled-jobs) gives the order, the fallbacks
   and the schedule.

5. `agents sync` publishes policy. It fast-forwards the south and north main checkouts, runs the
   policy gate in [crates/north-core/src/agent_config_check/](../crates/north-core/src/agent_config_check/)
   (`north agent-config-check policy-gate session`), then `north config agents sync` builds a new
   activation generation under `~/.local/state/north/agents/` from South's `catalog-config.json` and
   links each allowed skill into `~/.claude/skills` and `~/.codex/skills`. It reads north's skills,
   scripts and hooks from the tree of the runtime `current` names, never from the checkout, so only
   promotion moves them.

6. The generation's instructions and `provider-hooks` reach Claude Code and Codex through links
   Firn's profile module makes; `north agent-config-check` checks that wiring and
   `north agent-config-check --local` lints the shared hooks. A hook script deleted from
   `harness/hooks/` must be named in `harness/retired-hooks.txt`, which
   `north config agents check-owners` enforces at landing; the generation then carries a no-op stub
   under that name until each host's Firn profile stops registering it.

7. South registers a north-owned guard by its id; sync, `check-owners` and the policy checks resolve
   it through the `[[registered]]` entry for that id in north's [structure.toml](../structure.toml),
   so moving the guard file edits that `path` and nothing in South.

To try only the `north` app, follow the [README](../README.md#try-it); it needs neither South nor
Firn.

## Where policy lives

Policy is the rules agents follow and the checks that hold them to it. Each kind has one home and
one way to reach agents:

- Global agent rules: `south:agents/AGENTS.md`, each bullet registered in `south:agents/rules.toml`;
  published by `agents sync`.

- Rules for `~/code`: [harness/code/AGENTS.md](../harness/code/AGENTS.md); `agents sync` links it as
  `~/code/AGENTS.md`.

- Rules for one repository: that repository's `AGENTS.md`, such as [this one](../AGENTS.md), read in
  the checkout.

- Procedures: skills in [harness/skills/](../harness/skills/) and
  [agent-machinery/skills/](../agent-machinery/skills/), linked by `agents sync`.

- Guards on tool calls: hooks in [harness/hooks/](../harness/hooks/), bound to events by
  [harness/policy-owners.toml](../harness/policy-owners.toml), reaching agents through the
  generation's `provider-hooks`.

- Claude worker definitions and Codex prompts: [harness/claude/agents/](../harness/claude/agents/)
  and [harness/codex/prompts/](../harness/codex/prompts/); how they are installed is not traced in
  this repository, and `north agent-config-check` reads the installed copies in `~/.claude/agents`.

- Thresholds and priors: [harness/defaults.toml](../harness/defaults.toml), each with its source,
  read by the commands that use them.

- Models, prices and benchmarks: [harness/model-catalog.toml](../harness/model-catalog.toml), read
  by routing.

- Staffing tiers, accounts, project shares and targets: `south:agents/orchestration.toml`, read by
  `agents plan` and the spawn gate.

- Recurring routines: `south:agents/routines/`, run by `agents routines`.

A policy change lands like any other change. `agents sync` refuses to publish policy that fails its
gate, including a measured drop in replay conformance against `south:agents/operating-point.toml`;
the "Policy regression gate" entry in [features.md](features.md#policy-measurement) gives the rules.

Each hook names the runtime gap it fills and is deleted when Claude Code closes that gap; the audit
of every installed Claude hook entry is [rationale/hooks-and-runtime.md](rationale/hooks-and-runtime.md).

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

3. **Pick the tier and ETA.** `north worker-ledger --recommend` picks the cheapest tier whose
   measured success meets the `success_threshold` key in
   [harness/defaults.toml](../harness/defaults.toml) and prints the ETA.

4. **Write the brief.** Carry Item, Category, facet lines, files, Done when and ETA. A retry or
   follow-up adds `Follows:`.

5. **Launch through Fleet.** `north fleet launch` starts the session with the brief as its system
   prompt and records the parent link. Before anything starts it runs the placement gate, and a
   Claude launch that holds an issue starts on the tier its brief earns, so leave the model and
   effort to it. The "Placement gate" and "Launch tier" entries in
   [features.md](features.md#fleet-sessions) and `north fleet launch --help` give the verdicts and
   the recorded overrides.

6. **Queue what waits.** A box that waits on another lane is queued with `threads queue follow`;
   `follows-release` starts it once its predecessor lands on main.

7. **Record.** Fleet receipts capture delivery and completion. `north worker-ledger` records each
   run's outcome, cost and duration, and those records feed the next recommendation
   ([feedback-loop.md](feedback-loop.md)).

## Messages between fleet sessions

`north fleet send NAME TEXT...` sends a message and `north fleet ask NAME QUESTION...` waits for a
reply. Use `--parent NAME` when launching a child session so `north fleet tree` shows the
relationship. A worker's final message is its report; Fleet records delivery and session
completion.

## Landing a change

Agents edit in `~/code/<project>/worktrees/<slug>`, never in `main/`, and land with
`safe-push --to main` ([bin/safe-push](../bin/safe-push); `safe-push --help` is the full contract;
the [repo-safety skill](../harness/skills/repo-safety/SKILL.md) is the procedure). The "Landing and
lanes" entries in [features.md](features.md#landing-and-lanes) describe each gate. In outline it:

1. fetches and rebases onto origin/main under the per-origin lock, holding no capacity lease while
   it does; red-main fixes take priority, and queued lanes can land as one train;

2. runs the checks the repository's committed `.safe-push` maps to the changed paths
   (`land.check` entries; [this repository's](../.safe-push) cover the Rust host, the harness
   scripts, agent-machinery and the docs), or the pre-push hook when there are none;

3. sends those checks where they can run: with a `land.workflow`, every check except the
   `land.local-only` and `land.machine-bound` ones runs as one GitHub Actions job through
   `north land-actions`; machine-bound checks run on the `land.runner` build host; and a host in
   game-reserve ships the lane to another host with room (`north capacity offload`);

4. refuses the push unless every gate passes, among them a betterleaks secret scan, the dependency
   gate (`north land scan dep-gate`), the stale-revert gate (`north stale-revert`), the bin-reference gate
   (`north bin-references`) and the CI time budgets in `.safe-push`; it never force-pushes;

5. pushes to main, removes the clean lane and its branch, and runs `follows-release`.

`north land checks` runs the local checks one after another under one capacity lease, so the
fetch, rebase, queue wait and the wait on GitHub Actions hold no cores. Checks run with the landing
lock released, so two landings' checks overlap. If main moved during the checks, a lane whose files
the new commits do not touch rebases and keeps its verdict; any other lane rechecks. A landed
receipt names the lane head verified on the destination.

A repository with `landing.queue = autoland` in its `.safe-push`, or an Autoland workflow and no
`landing.queue`, instead gets a `claude/land-*` branch that the workflow lands; North has neither.
A worker runs `safe-push` in the background and reports when it exits; a lead runs it in the
foreground.

After the push, CI runs on main ([.github/workflows/](../.github/workflows/)). When a run fails, the
"Main is red" workflow re-runs the failed job, and if the same step fails again after a green parent
it pushes `claude/land-revert-<sha>` naming the failing cases and reports on a "main is red" issue
(`north main-red`, [crates/north-ops/src/ops/ci_watch/sheriff.rs](../crates/north-ops/src/ops/ci_watch/sheriff.rs)). The lander reverts, as a red-fix
lane checked like any other, only the landed lane whose removal turns those cases green, and tells
that lane's session ([crates/north-landing/src/land/sheriff.rs](../crates/north-landing/src/land/sheriff.rs)).
