# Eval X trial hand-off

The part of an Eval X trial that runs inside the task's container. Harbor's shipped generic agent (`--agent acp`,
Harbor v0.25.0, `harbor/agents/installed/acp.py`) starts `stub.sh` with `sh` as a local distribution, and the stub
hands the instruction to the host through the trial directory. The host side (the arm) writes `done.json` at the turn
end; the stub then answers `end_turn` whatever `done.json` says, so the arm's outcome is read from its own files.

## Files

- `stub.sh`: POSIX sh, 48 lines. Uses shell builtins plus `mv` and `sleep`, which every task image has, so nothing is
  installed in the image and no binary is built.
- `args`: copies `stub.sh` into `$EVAL_X_WORK/bin` (default `/tmp/eval-x/bin`) when it is missing or different, and
  prints the Harbor arguments that select the stub, one per line: `--agent acp` and three `--ak` lines (the inline
  registry entry with its `local` distribution, `distribution_preference=local`, `auth_policy=disabled`).

## Mounts the runner adds

The stub reaches the container through two bind mounts that the runner passes with `--mounts-json`:

- `$EVAL_X_WORK/bin` read-only at `/eval-x-bin`, so the stub is at `/eval-x-bin/stub.sh`.
- the trial directory `T` read-write at `/eval-x`.

`T` is owned by the host user, which is container root under rootless podman, so no permission change is needed.

## Files in the trial directory

The stub writes each file whole to a dot-file and renames it, in this order:

1. `session.json`: the raw `session/new` request line, which holds the working directory.
2. `container`: the content of `/etc/hostname`.
3. `prompt.json`: the raw `session/prompt` request line, last, so its presence means the other two are complete.

Then it polls `/eval-x/done.json` once a second and returns `{"stopReason":"end_turn"}` when it exists. The host reads
the instruction with `jq -r '.params.prompt[] | select(.type == "text") | .text' T/prompt.json`.

## JSON-RPC shapes assumed from Harbor v0.25.0

Harbor's runner is `acp_runner.py` with the `agent-client-protocol` library. Messages are newline-delimited JSON on
stdio.

- `initialize`: the request carries `params.protocolVersion`. The stub echoes it and returns `agentCapabilities: {}`.
  No `authMethods` are returned, so the runner never calls `authenticate`.
- `session/new`: `params.cwd` and `params.mcpServers`. The stub returns `sessionId`. MCP servers are ignored.
- `session/prompt`: `params.sessionId` and `params.prompt`, a list of blocks each with `type` and `text`. The stub
  returns `stopReason`.
- Any other request with an `id` gets `{}`. Notifications (no `id`) are ignored.

The stub reads a method and an id by searching the line for its key, so it accepts compact or spaced JSON. It matches
the first occurrence of each key, and the first key of a request is its id.

## Launch and pins

`args` prints the arguments; Harbor's runner in the container also needs `agent-client-protocol` at a version that has
the names `acp_runner.py` imports. Version 0.9.0 has them. The container install of Harbor v0.25.0 does not pin the
library, and the current release on PyPI, 1.0.0, lacks `text_block`, so the runner fails at import before the stub
starts.
