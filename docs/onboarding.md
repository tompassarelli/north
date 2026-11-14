# Install north and land your first change

This page takes a new account with Nix and a Claude Code login to one change landed on a scratch
repository. The one command is here; the operator repository that a long-lived fleet reads is
not needed, and neither is a second machine. The whole path is meant to take at most 30 minutes.

The change is made the way north makes every change: a model writes it in a fleet session, the
session hands it in, and a lander checks the commit and lands it on main. The scratch repository is
thrown away after the run, so nothing outside this page is changed.

## Before you start

You need:

- x86_64 Linux with Nix and flakes enabled.
- Claude Code installed and signed in. Run `claude` once, finish its sign-in, and quit; the login is
  the `~/.claude` directory and the `~/.claude.json` file that north will read.
- `git`, and about 10 GB of free disk for the Nix store and the Rust build.

Check the three tools:

```sh
nix --version
claude --version
git --version
```

If `nix` is missing, install it from [nixos.org/download](https://nixos.org/download/) and enable
flakes in `~/.config/nix/nix.conf` with `experimental-features = nix-command flakes`.

## 1. Run setup

```sh
nix run github:tompassarelli/north#setup
```

The command prints each of its three steps as it starts it and, when one fails, what to do next.
Run it again after you fix the cause; it reuses whatever the last run left in place.

1. It clones north to `~/code/north/main`, the place the pages after this one expect, or uses the
   checkout already there.
2. It builds `target/debug/north`, the fleet binary, and `target/debug/verify-north`, the end-to-end
   check, with `nix develop -c cargo build --locked`. The flake pins the Rust toolchain and every
   other build input; the first build downloads them and takes several minutes, and later builds reuse
   the Nix store.
3. It runs `verify-north --claude-account main --from-claude-login`. North runs every provider in an
   account profile of its own under `~/.local/state/north/profiles/`, so a fleet session never writes
   the login you use by hand. When `claude-main` has no profile yet, verify-north makes it: it links
   `~/.claude/.credentials.json`, so Claude Code renews the same login, copies `~/.claude.json` when
   it exists, so the profile gets its own conversation state, and writes an empty `settings.json`.
   Then it lands the change:
   1. It creates a scratch repository with a bare origin under `$TMPDIR` and a lane on a `worker`
      branch.
   2. It starts `north land serve` for that origin and the worker sandbox that keeps the worker's
      tools on that scratch world.
   3. It launches one Claude haiku worker with a one-file brief. The worker writes `worker.txt` and
      calls the `hand-in` tool, which queues the lane and waits for the landing.
   4. The lander checks the commit and pushes it to the scratch origin's `main`; `north ledger worker`
      then records the run, and the walk cleans every process and directory it made.

A passing run prints a `step 0 ok` line for the profile and one for the binary it uses, one
`step N ok` line for each of the four spine steps, and ends:

```
step 4 ok     ... ms  `north ledger worker` records the run done and landed at that commit
verify-north passed  total ... ms
setup: done: a worker's change landed; run ~/code/north/main/target/debug/north to open the fleet screen
```

Exit 0 is the proof that the worker's change landed on `main`.

## What just happened

The **model** is the Claude request inside the worker. The **harness** is north and the Claude Code
CLI that serves it. The **agent** is the worker session north launched with the brief. The **fleet**
is the whole run: the account, the worker, the lander and the ledger that records the landing. The
four words and the rule that a model is never called an agent are in [terms.md](terms.md).

The lane, the brief and the hand-in tool are the same path every later change takes; they are
described in [briefing.md](briefing.md) and [landing.md](landing.md).

## Install north on a host

The check above uses the two binaries from the checkout. A host that runs a fleet promotes a runtime
instead, so every north command and every scheduled job share one commit.

1. **Lay out the code.** Each repository is a container `~/code/<project>/` with `main/`,
   `worktrees/<slug>/` and `pins/<object-id>/` ([harness/code/AGENTS.md](../harness/code/AGENTS.md)).
   North's commands expect `~/code/north/main` and, for an operator, the policy repository at
   `~/code/south/main`; [operator-repository.md](operator-repository.md) says what that holds.
2. **Put `north` on `PATH`.** [bin/north](../bin/north) launches the installed runtime, and the tools
   in [bin/](../bin/) are toolchain shims and a few helpers. The runtime installer links `safe-push`
   and the other commands to the north binary under `~/.local/share/north/bin`
   ([harness/policy-owners.toml](../harness/policy-owners.toml),
   [crates/north-core/src/agent_policy_contract/wiring.rs](../crates/north-core/src/agent_policy_contract/wiring.rs)).
3. **Promote a runtime.** `north runtime update` builds `origin/main`, installs the binary and the
   commit's own tree under `~/.local/state/north/runtimes/<sha>/`, points
   `~/.local/state/north/current` at it, runs `north runtime health`, and then runs
   `north config agents sync`; a sync failure alarms the coordinator once per cause and retries every
   tick but never rolls the runtime back. A build in progress always finishes and promotes before the
   next one starts on the newest main. Scheduled jobs run harness code from `current/tree/bin`, so the
   code they run and the binary they call share a commit.

   ```sh
   target/debug/north runtime update
   ```

4. **Keep hosts current.** Each host promotes landings itself: a timer runs `north runtime update`,
   and `north runtime deploy` carries a build to the other declared hosts. The "Runtime promotion"
   entry in [features/08-machines-capacity-and-scheduled-jobs/](features/08-machines-capacity-and-scheduled-jobs/)
   gives the order, the fallbacks and the schedule.
5. **Publish policy.** `north config agents sync` fast-forwards the operator repository and north main
   checkouts, runs the policy gate in
   [crates/north-core/src/agent_config_check/](../crates/north-core/src/agent_config_check/)
   (`north check harness policy-gate session`), builds a new activation generation under
   `~/.local/state/north/agents/` from the operator repository's `catalog-config.json`, and links each
   allowed skill into `~/.claude/skills` and `~/.codex/skills`. It reads north's skills, scripts and
   hooks from the tree of the runtime `current` names, never from the checkout, so only promotion
   moves them.
6. **Wire the providers.** The generation's instructions and `provider-hooks` reach Claude Code and
   Codex through links the machine configuration makes; `north check harness` checks that wiring and
   `north check harness --local` checks the live links. A hook script deleted from the hooks directory
   of the north tree must be named in `harness/retired-hooks.txt`, which
   `north config agents check-owners` enforces at landing; the generation then carries a no-op stub
   under that name until each host stops registering it.
7. **Register guards by id.** The operator repository registers a north-owned guard by its id; sync,
   `check-owners` and the policy checks resolve it through the `[[registered]]` entry for that id in
   north's [structure/](../structure/) tree (the lander writes `structure.toml` from it), so moving the
   guard file edits that `path` and nothing in the operator repository.

To try only the `north` app, follow the [README](../README.md#quickstart); it needs neither the operator
repository nor the machine configuration. Once the check above passes, [briefing.md](briefing.md) and
[landing.md](landing.md) show how work moves.

## If the check fails

Setup names the step that failed and what to do. A failed clone or build prints the git or cargo
output that names the cause. A failed `verify-north` prints a `FAIL` line naming its step and cause and
keeps its evidence under `~/.local/state/north/verify-north/failed/<stamp>/`. When Claude Code has no
login, setup stops after the build with a line that says to run `claude`, finish its sign-in and quit. Run the same command
again after fixing the cause: it reuses the checkout, the build and the profile, and verify-north
creates a fresh scratch world each time and never reuses a lane or a ledger.
