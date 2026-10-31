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
   [crates/north-core/src/agent_policy_contract/wiring.rs](../crates/north-core/src/agent_policy_contract/wiring.rs).
3. `north runtime update` builds origin/main in a detached worktree, installs the binary and the
   commit's own tree under `~/.local/state/north/runtimes/<sha>/`, points `~/.local/state/north/current` at it and runs
   `north runtime health` (the new binary is the current runtime, with its programs and tree) and `agents sync`, each bounded to 5 minutes, rolling back if either fails.
   `north agent-runtime-update` runs it before it updates the Claude, Codex and Antigravity runtimes, and the `north-runtime-follow` timer on every host every 2 minutes, so each host also promotes a landing itself when no push reached it. `north runtime deploy [HOST...]` (every other `[hosts.NAME]` with an `ssh` destination when none is named; a failed host does not stop the rest) builds on
   the machine it runs on, realises the binary's Nix store references on each HOST, copies the programs into that
   HOST's `runtimes/<sha>/` and runs `north runtime update` there, which finds them built and only promotes. A host that can neither pull nor build origin/main promotes the newest ancestor of it that a peer has promoted, and a build more than half done finishes and is promoted before the newer head is built, so fast landings never stall every host. After a landing on north's main, safe-push runs `north runtime follow`, which starts `north runtime promote` (update here, then deploy to every declared host) detached and logs to `runtime-follow.log`. The
   `north` command in [bin/north](../bin/north) launches that installed runtime, and scheduled jobs run
   harness code from `current/tree/bin`, so the code they run and the binary they call share a commit.
4. `agents sync` publishes policy. It fast-forwards the south and north main checkouts, runs the
   policy gate in [crates/north-core/src/agent_config_check/](../crates/north-core/src/agent_config_check/) (`north agent-config-check policy-gate session`),
   then `north config agents sync` builds a new activation generation under
   `~/.local/state/north/agents/` from South's `catalog-config.json` and links each allowed skill into
   `~/.claude/skills` and `~/.codex/skills`. It reads north's skills, scripts and hooks from the tree of the
   runtime `current` names, never from the checkout, so a deferred promotion leaves them in step with the
   binary; only promotion moves them. The generation's instructions and `provider-hooks` reach
   Claude Code and Codex through links Firn's profile module makes; `north agent-config-check` checks
   that wiring. `north agent-config-check --local` lints the shared hooks with `shellcheck` from PATH or else from the north flake's `shellcheck` package, and its second failing run in a row opens or updates one North issue, "Agent configuration check failing on HOST", that a passing run closes (north#351); worktrees under `~/.cache` and `~/.local/state` belong to their tools and are not layout drift. A hook script deleted from `harness/hooks/` must be named in `harness/retired-hooks.txt`
   (`north config agents check-owners` refuses the landing otherwise); the generation then carries a
   no-op stub under that name, so a host whose Firn profile still registers it is silent, not a
   "No such file or directory" error, until it rebuilds. The same check refuses a landing while
   nixos-config `origin/main` registers a hook the landing catalog does not project.

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
| Claude worker definitions and Codex prompts | [harness/claude/agents/](../harness/claude/agents/) and [harness/codex/prompts/](../harness/codex/prompts/) | not traced in this repository; `north agent-config-check` reads the installed copies in `~/.claude/agents` |
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
3. **Pick the tier and ETA.** `north worker-ledger --recommend` picks the cheapest tier whose measured
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
   A Claude launch that holds an issue (`--task REF`, or an `Item:` line) also starts on the tier
   its brief's `Category:` and facets earn, the routing `north dispatch` uses, so leave `--model`
   and `--effort` off; a `--model` of another family needs `--tier-reason "REASON"`, which is
   recorded with the other override reasons.
6. **Queue what waits.** A box that waits on another lane is queued with `threads queue follow`;
   `follows-release` starts it once its predecessor lands on main.
7. **Record.** Fleet receipts capture delivery and completion. `north worker-ledger` records each run's
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
   when `.safe-push` names a `land.workflow`, every check except the `land.local-only` ones
   (`REASON :: COMMAND`, the reason printed when it runs here) runs as one job of that workflow on
   GitHub Actions: `north land-actions` scans the lane for secrets and private values, pushes it to
   `land-check/<sha>`, dispatches the workflow, waits for its verdict and deletes the branch, while
   the local-only checks run here; when `.safe-push` sets `land.publish = PREFIX`, safe-push scans
   the lane the same way before the checks that run here, pushes it to `PREFIX<sha12>`, passes that
   branch as `SAFE_PUSH_LAND_REF` so a check can dispatch its own workflow on it, and deletes it after
   them; `bin/land-shell` runs a check in `depbox` on a host and directly on
   a runner, and `harness/machine-bound-tests` names the harness tests only a host can run;
   while this host is in game-reserve and another `[hosts.NAME]` with an `ssh` destination has room
   (6 free cores, the heavy memory request and 20 GiB of disk), the lane ships there by git bundle
   (`north capacity offload`), `landing.offloaded-capacity` replaces `landing.capacity` for the light
   part, each check runs in a persistent slot on that host and its exit status is the verdict; a
   `land.local-only` check keeps the landing on this host;
3. refuses the push unless every gate passes, among them a gitleaks secret scan, the dependency gate
   (`north dep-gate BASE HEAD`), a `Provenance:` record for each new
   gating number (`north provenance check`), a `Test-kind:` claim for each new test naming a recorded mutant
   only it kills (`north mutant verify` runs the named tests when `.safe-push` sets `mutant.run`; north's runs each claimed test on the change and on every mutant through `harness/scripts/mutant-cargo-tests.sh`, which runs a claim that is not a cargo test name with `bun test` on the `.test.ts` files naming it, and for cargo claims takes one of four slot directories under `~/.cache/north/mutant-cargo` by non-blocking lock so concurrent landings never queue behind each other, seeds a new slot by reflink from a snapshot, and builds each tree incrementally in its slot so a mutant recompiles only what its patch touches; a recorded kill is reused for a rebased lane whose diff, mutant patch, runner and test are unchanged), the stale-revert gate (`north stale-revert`, by git blame on main: a landing may not remove lines of a main commit the lane never saw, nor, per file, restore, in the hunks that touch its lines, half the lines a commit of the last 48 h removed, or delete outright four fifths of the lines it added from a file that stays without re-adding them elsewhere, so deleting a whole file undoes only commits the lane never saw; it runs before the land checks and again after the rebase; `linguist-generated` files are skipped and a `Reverts: SHA` trailer excuses the named commit), the bin-reference gate (`north bin-references`, north only: a landing may not delete `bin/NAME` while nixos-config or south `origin/main`, or this host's own `systemctl --user cat`, still names `tree/bin/NAME`; remote hosts' live units are not read), and the CI time budgets in `.safe-push`; it never force-pushes;
4. pushes to main, removes the clean lane and its branch, and runs `follows-release`.

North's land checks share that capacity scope and run with the landing lock released, on every
retry; the lock covers only the fetch, rebase, scans and push, so two landings' checks overlap. If main moved during the checks, a lane whose files
the new commits do not touch rebases and keeps its verdict; any other lane requeues and rechecks. Cancellation prints `safe-push: cancelled` and
exits 130; a landed receipt names the lane head verified on the destination.

A repository with `landing.queue = autoland` in its `.safe-push`, or an Autoland workflow and no `landing.queue`, instead
gets a `claude/land-*` branch that the workflow lands; North has neither. A worker runs `safe-push` in
the background and reports when it exits; a lead runs it in the foreground.
It runs the repository's pre-push hook to completion before it opens the push of that branch.

After the push, CI runs on main ([.github/workflows/](../.github/workflows/)). When a run fails,
the "Main is red" workflow re-runs the failed job once, and if the same step fails again after a
green parent it reverts the culprit commits and reports on a "main is red" issue
([harness/scripts/main-red.ts](../harness/scripts/main-red.ts)).
