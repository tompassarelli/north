# Worker sandbox: removal confined by the kernel

North#706, 2026-11-08. A fleet worker runs under bypass permissions, so north's own guards are the only check on what its commands delete. A delete whose target is computed at run time is invisible to a command-text check. Examples: a path split at a space by `xargs`, a `dirname` applied to the wrong line, an unset variable inside a script, or a `remove_dir_all` in a test fixture or build script. The kernel sees the real path at `unlinkat`, `rmdir` and `rename`, so the check belongs there.

## Prior art

- Claude Code's sandboxing runs each Bash command under bubblewrap on Linux and Seatbelt on macOS, with writes allowed in the working directory, plus a network proxy (Anthropic's engineering post "Making Claude Code more secure and autonomous", 2025, and the `sandbox` settings).
- Bubblewrap builds a mount namespace, where a path is either writable or read-only. A worker commits from its lane into the main checkout's `.git`, so that directory must be writable, and a mount cannot allow writing a file while refusing its removal.
- Landlock is an unprivileged Linux security module (since 5.13). Its rights for removing a directory, removing a file and reparenting (`REMOVE_DIR`, `REMOVE_FILE`, `REFER`) are separate from its rights for writing and creating. Once applied, a domain cannot be lifted, and every child process inherits it. Codex's Linux sandbox uses Landlock together with seccomp.

## Design

Each Bash command, hook and MCP server of a worker runs through `CLAUDE_CODE_SHELL_PREFIX="<north> --worker-sandbox"`; Claude Code passes the prefix the whole command string. `north --worker-sandbox` builds a Landlock ruleset that handles only `REMOVE_DIR`, `REMOVE_FILE` and `REFER`, grants `REFER` on `/`, grants all three beneath the worker's own trees, then execs `$SHELL -c COMMAND`. Writing and creating files stay open everywhere: the domain guards removal, not edits. The Claude Code process itself is never confined, and its native Write and Edit tools cannot remove a directory.

The worker's own trees (crates/north-base/src/platform/worker_sandbox/removable.rs) are the lane the command runs in or names by absolute path, the git common directory of that project and of every project the command names by absolute path, scratch and build caches, and agent state other than the account profiles. Rules are taken on real paths, since a Landlock rule holds an inode. Only directories become rules, because removing a file is a right of the directory that holds it. A symlink is followed only toward a protected path inside it, and is never granted itself. Claude Code starts each command in the session's first directory, so a worker reaches its lane with `cd LANE && ...`, and a lane named in the command counts as its own.

The project's whole git common directory has to stay removable. The grant is the directory `git rev-parse --path-format=absolute --git-common-dir` prints, resolved from the checkout's `.git` (a gitfile leads through the worktree's `commondir` file): where `main/.git` is a gitfile its own path grants nothing, and a project the sandbox did not grant stranded the same way, since git creates a lock file and then cannot rename or remove it. A replayed `git fetch` left `refs/remotes/origin/HEAD.lock` and `objects/maintenance.lock` behind before the fix. A command that reaches another project's git by a computed path still gets no grant. Measured under the sandbox: `git worktree add -b` renames `config.lock` at the top of `.git`, `git branch -D` renames `packed-refs.lock`, fetch renames `objects/maintenance.lock` and its packs, and a loose object's temporary file is unlinked after linking. Each fails with EACCES if refused.

`north lane unlock`, `north lane stray` and `north lane remove` run outside the domain, since each verb enforces its own removal rule: a git lock no process holds that is older than 60 s, an untracked file a tool call in the session's own transcript named before the file was born (north#937), or a lane directory under `worktrees/` that is clean and holds no unlanded work (north#954). A worker's `git worktree remove` of a lane there made git unregister the worktree and then fail to delete its directory, which left 4 trees, 8.0 GB, on 2026-11-08; the guard now refuses that command and names the verb.

## Hidden files (north#969)

Beside the three removal rights the ruleset handles `READ_FILE`, so it can hide named files from a worker command (crates/north-base/src/platform/worker_sandbox/readable.rs). Landlock only grants access, so the domain grants `READ_FILE` beneath every entry of `/` except the entries that hold a hidden file, walks into those, and leaves the file itself out. Entries are matched by device and inode, because a rule holds an inode and one directory can sit at two paths (two mounts of one btrfs subvolume). A symlink to a hidden file reads nothing, and a hard link out of its directory fails with `EXDEV`, because Landlock refuses a link that would gain read access. The list is taken when each command starts.

## Build output (south#93)

A lane's build output (`target/`, `node_modules/`) runs through the toolchain shims, which enter depbox, or through `north lane exec [--lane CHECKOUT] -- PROGRAM`, which runs a lane's own binary when the lane's lock files equal origin/main's; a lane whose lock files differ in registry versions reruns PROGRAM under `north depbox run -- PROGRAM`, where the dependency code is confined, instead of refusing it. head-north and verify-north use it for a north built from the tree (crates/north-base/src/platform/worker_sandbox/executable.rs, crates/north-lanes/src/lanes/exec.rs).

## Lanes created after the session starts (north#1033)

A Landlock rule holds an inode, so a domain built at the start of a turn or session cannot grant a lane created later. Claude Code starts a fresh domain for every command, so a lane created by an earlier command is already there when the next one runs. OpenCode and Pi keep one lane-only domain around the whole turn, so their shell tool runs each command through a `shell` wrapper in the scratch directory (crates/north-providers/src/providers/opencode/turns.rs): the wrapper asks the outside broker for `north --worker-sandbox --lane`, and the broker (crates/north-runtime/src/worker_sandbox/broker.rs) starts that in a fresh lane-only domain outside the turn's. The fresh domain keeps the lane the command runs in, every lane its text names by absolute path, the checker's done-when lane, and safe-push's base trees readable, so a lane a worker creates mid-session can run its own scripts. The outer domain still confines the harness process and its native file tools, which never leave it.

`north gh-session` runs `gh` with a private config directory that links no `hosts.yml`, since `gh` exits when a config file it finds cannot be read.

A kernel without Landlock ABI 2 or later runs the command unconfined and says so on stderr. `no_new_privs`, which Landlock requires, also stops setuid programs inside a worker command.

## Rollout

Landlock has no permissive mode, so a shadow run cannot log a would-be refusal without refusing. The `worker-sandbox` gate in harness/gates.toml lands in shadow, where only a canary launch made with `NORTH_WORKER_SANDBOX=1` carries the prefix. In enforce, `claude_env` adds it to every launch (crates/north-providers/src/providers/claude/session_env.rs). The review counts canary runs and the "Permission denied" removals in their Bash tool results. The gate enforces from 2026-11-08, on the canary review recorded in harness/gates.toml.


The red team, per-host recovery and canary readings behind this design are Tom-specific and live in south (agents/decisions/harness/worker-sandbox-2026-11-08.md).
