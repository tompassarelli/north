# Worker sandbox: removal confined by the kernel

North#706, 2026-11-08. A fleet worker runs under bypass permissions, so north's own guards are the only check on what its commands delete. A delete whose target is computed at run time is invisible to a command-text check. Examples: a path split at a space by `xargs`, a `dirname` applied to the wrong line, an unset variable inside a script, or a `remove_dir_all` in a test fixture or build script. The kernel sees the real path at `unlinkat`, `rmdir` and `rename`, so the check belongs there.

## Prior art

- Claude Code's sandboxing runs each Bash command under bubblewrap on Linux and Seatbelt on macOS, with writes allowed in the working directory, plus a network proxy (Anthropic's engineering post "Making Claude Code more secure and autonomous", 2025, and the `sandbox` settings).
- Bubblewrap builds a mount namespace, where a path is either writable or read-only. A worker commits from its lane into the main checkout's `.git`, so that directory must be writable, and a mount cannot allow writing a file while refusing its removal.
- Landlock is an unprivileged Linux security module (since 5.13). Its rights for removing a directory, removing a file and reparenting (`REMOVE_DIR`, `REMOVE_FILE`, `REFER`) are separate from its rights for writing and creating. Once applied, a domain cannot be lifted, and every child process inherits it. Codex's Linux sandbox uses Landlock together with seccomp.

## Design

Each Bash command, hook and MCP server of a worker runs through `CLAUDE_CODE_SHELL_PREFIX="<north> --worker-sandbox"`; Claude Code passes the prefix the whole command string. `north --worker-sandbox` builds a Landlock ruleset that handles only `REMOVE_DIR`, `REMOVE_FILE` and `REFER`, grants `REFER` on `/`, grants all three beneath the worker's own trees, then execs `$SHELL -c COMMAND`. Writing and creating files stay open everywhere: the domain guards removal, not edits. The Claude Code process itself is never confined, and its native Write and Edit tools cannot remove a directory.

The worker's own trees (crates/north-runtime/src/worker_sandbox/removable.rs) are the lane the command runs in, the git directory of that project, scratch and build caches, and agent state other than the account profiles. Rules are taken on real paths, since a Landlock rule holds an inode. Only directories become rules, because removing a file is a right of the directory that holds it. A symlink is followed only toward a protected path inside it, and is never granted itself.

The project's whole git directory has to stay removable. Measured under the sandbox: `git worktree add -b` renames `config.lock` at the top of `.git`, `git branch -D` renames `packed-refs.lock`, fetch renames `objects/maintenance.lock` and its packs, and a loose object's temporary file is unlinked after linking. Each fails with EACCES if refused.

A kernel without Landlock ABI 2 or later runs the command unconfined and says so on stderr. `no_new_privs`, which Landlock requires, also stops setuid programs inside a worker command.

## Rollout

Landlock has no permissive mode, so a shadow run cannot log a would-be refusal without refusing. The `worker-sandbox` gate in harness/gates.toml lands in shadow, where only a canary launch made with `NORTH_WORKER_SANDBOX=1` carries the prefix. In enforce, `claude_env` adds it to every worker launch (crates/north-providers/src/providers/claude/session_env.rs). The review counts canary runs and the "Permission denied" removals in their Bash tool results.

Landing this change under the sandbox showed one blocker to fix before enforcing. Landlock denies mount operations inside its domain, so bubblewrap fails ("Failed to make / slave: Operation not permitted"), and with it safe-push's depbox build and `north capacity sandbox`. Those runs have to move outside the worker's domain, for example by handing them to a user service, as the lander already is.

The red team, per-host recovery and canary readings behind this design are Tom-specific and live in south (agents/decisions/harness/worker-sandbox-2026-11-08.md).
