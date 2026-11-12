# Eval X: Terminal-Bench 2 as an outside measure

Compares north with plain T3 Code on [Terminal-Bench 2.0](https://github.com/harbor-framework/terminal-bench-2), 89
container tasks run and graded by the benchmark's own harness, [Harbor](https://github.com/harbor-framework/harbor)
(south#107; the registered design, held-fixed conditions and pass line are in the assessment's `EVALS.md`, section
"Eval X"). Neither project is copied here: both are fetched by commit at run time, and their licence review is in
[docs/references.md](../../docs/references.md#terminal-bench-2).

## What is here

- `flake.nix`, `flake.lock`: a dev shell for this directory only, with rootless podman, podman-compose, uv and
  Python from the nixpkgs revision north's own flake locks. It changes no system configuration and needs no root.
- `host`: glue under the dev shell. `host fetch` checks out the tasks at `2fd12b88aafd` and Harbor at its release
  `v0.25.0` (`fdba28fc411e`) under `~/.local/state/north/eval-x` (or `EVAL_X_WORK`), installs Harbor from its own lock and prints
  its version. `host oracle TASK` runs the benchmark's reference solution for one task through Harbor and the
  task's own grader, and prints one graded row, the digests of the images pulled and the disk they take: it proves
  a host can run the benchmark before any agent is involved. `host caps IMAGE` starts one container with a cap of
  one CPU and 2 GB and prints the limits the kernel applied, `cpu.max 100000 100000` and `memory.max 2147483648`
  when the user's cgroup delegation lets a task's caps hold. `host hold IMAGE` keeps one capped container alive for
  four minutes and prints its cgroup and, at each of three minutes, `frozen 0` while north's containment sweep leaves
  it running. `host podman ARGS...` runs podman from the dev shell, which is how the runner reaches it. `host harbor
  ARGS...` runs Harbor with the same pins.
  Harbor's telemetry is off in every call and nothing is uploaded.

## Where it runs

`evals/x/run` and `evals/t/run` stay in the checkout they are called from, which must be a git checkout, and build their tools there once per run; a failed build stops the run with its own exit code. The scripts that start or drive a session come from the live runtime's own tree (`~/.local/state/north/current/tree`; `EVAL_X_RUNTIME` names another runtime-shaped directory) and only call the built binaries, so nothing is built or written in that tree. A runtime whose tree lacks one of them, or whose `evals/t/arm` still builds its own tool, stops the run before its first trial.

A container needs the full range of subordinate user ids, which a worker's sandbox cannot map, so `host oracle` and
`host harbor` are started as a user unit outside a worker session, from a checkout of main:

    systemd-run --user --collect --unit=eval-x-probe ~/code/north/main/evals/x/host oracle fix-git

A pass is a row with `"reward":{"reward":1}` and `"error":null`.

## How a harness reaches a task

Each harness and Claude Code stay on the host as in Eval A; only tool execution reaches the task. Shell commands run
in the task's container through the runtime's exec, and the file tools see the task's working directory through a
mount of it on the host. Model traffic never passes through the container and no credential enters it. Each part has
its own directory and README:

- `handoff/`: the stand-in agent Harbor starts in the container. It passes the task's instruction to the host and
  waits for the arm to end the trial.
- `workspace/`: copies a task's working directory out of its image to the host and prints the mounts for it.
- `relay/`: the shell both arms give Claude Code, and the broker that runs its commands in the container.
- `arm`: runs one arm on the host for one trial and writes the trial's result. The plain arm runs T3 Code to the end
  of its last run with no command of the session running; the north arm runs its session as a north host does, to its
  report (see below).
- `runner/`, `run`: plan and run the trials, one row per attempt; `run` builds the runner and starts it.
- `slice/podman`: stands in front of podman on the path of `host` and `relay/real-session` and gives every container
  they or Harbor start the cgroup parent `agent-capacity-evalx.slice`. north's containment sweep
  (`north agent-contain sweep`, each minute) freezes a container scope anywhere else under the user manager, which
  stalls a trial at its next exec; under `agent.slice` it leaves containers alone, and their load counts as agent
  load. A task's own CPU and memory caps are set on the container as before.

## When the north arm ends a trial

north tells a session to put a command longer than 30 seconds in the background and to read its result when it ends,
and north's session host keeps the session until then: the end of the command invokes the session again. So the north
arm does not end a trial at the first turn's end. `eval-x-runner session-end`, which the arm polls every three
seconds, reads the session host's stream and decides:

- the last turn ended on a report (it opens `Done`, `Not done`, `Split`, `Stop` or `Blocked`): the trial ends
  (`report`);
- a turn is running, or a command the session started has not ended: wait;
- the last turn ended with no report and nothing of the session's is running: send it one message, as the lead a
  north worker always has would, and wait for that turn. A second such end ends the trial (`no_report`);
- the session host exited or parked: the trial ends (`stopped`).

The wait is bounded by the task's agent timeout (`--limit`); a session still going then ends as `limit`. The arm
stops the session before it writes `done.json`, so no command of the session runs beside the grader. `arm.json` holds
the outcome under `session_end`: `ended`, `turns` (one per invocation of the session), `nudges` (0 or 1) and
`open_tasks`.

## When the plain T3 Code arm ends a trial

T3 Code keeps the Claude Code session open after a run completes and starts a run of its own when a background command
of the session ends (`apps/server/src/orchestration-v2/Adapters/ClaudeAdapterV2.ts:5240`, `:8504`;
`ProviderSessionManager.ts:1186`), but `thread start --wait` returns when the first run is terminal
(`apps/server/src/cli/thread.ts:446`, `:665`; T3 Code v0.1.9). So the plain arm does not end a trial there.
`eval-x-runner plain-end`, which the arm polls every three seconds, reads `thread show --json` and the Claude Code
session file:

- a run is executing, or a background command of the session has not ended (`toolUseResult.backgroundTaskId` without a
  `<task-notification>` for it): wait;
- the thread asks for an answer and no run is executing: the trial ends (`asking`);
- no run is executing, no command is open and the thread and session file are unchanged for two polls: the trial ends
  (`idle`).

Nothing is sent into the session: T3 Code has no lead that nudges it. The wait is bounded by `--limit`; a session still
going then ends as `limit`. `session_end` holds `ended`, `turns` (T3 Code runs), `nudges` (always 0) and `open_tasks`.
