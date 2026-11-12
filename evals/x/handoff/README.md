# Eval X trial hand-off

The part of an Eval X trial that runs inside the task's container. Harbor's shipped generic agent (`--agent acp`,
Harbor v0.25.0, `harbor/agents/installed/acp.py`) starts `stub.sh` with `sh` as a local distribution, and the stub
hands the instruction to the host through the trial directory. The host side (the arm) writes `done.json` when it
ends the trial; the stub then answers `end_turn` whatever `done.json` says, so the arm's outcome is read from its own files.

## Files

- `stub.sh`: POSIX sh, 48 lines. Uses shell builtins plus `mv` and `sleep`, which every task image has, so nothing is
  installed in the image and no binary is built.
- `constraints.txt`: pins the protocol library (see below).
- `instruction.j2`: the prompt template Harbor renders the task's instruction through, reversed (see below).
- `args`: copies `stub.sh`, `constraints.txt` and `instruction.j2` into `$EVAL_X_WORK/bin` (default `~/.local/state/north/eval-x/bin`) when missing or
  different, and prints the Harbor arguments that select the stub, one per line: `--agent acp`, four `--ak` lines
  (the inline registry entry with its `local` distribution, `distribution_preference=local`, `auth_policy=disabled`,
  `prompt_template_path` naming the copied `instruction.j2`)
  and two `--ae` lines (the constraints variables).

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
the instruction with `jq -j '[.params.prompt[] | select(.type == "text") | .text] | join("") | explode | reverse | implode' T/prompt.json`.

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

## Pinned protocol library

Harbor v0.25.0's container install runs `uv pip install agent-client-protocol` with no version. PyPI's newest release,
1.0.0 (uploaded 2026-11-09), lacks `text_block` and `AuthCapabilities`, which `acp_runner.py` imports, so the runner
fails before the stub starts. `args` therefore passes two agent environment variables, `UV_CONSTRAINT` and
`PIP_CONSTRAINT`, both pointing at `/eval-x-bin/constraints.txt`, which pins `agent-client-protocol==0.9.0`. Harbor's
agent environment reaches its install step (`BaseInstalledAgent` puts the agent's env sources on every exec), and uv
honours `UV_CONSTRAINT`: a local check installed 0.9.0 from an unpinned request. Harbor's source and the graders are
unchanged.

The constraints file is in the mount that already carries the stub, so no extra mount is needed.

## Why the instruction arrives reversed

Harbor runs its agent as `bash -c` with the instruction quoted on the command line of that shell and of its runner
(`acp.py`, `run`). A session command that kills by a word of the task, such as `pkill -f convert_masks.py` on
sam-cell-seg, matches that command line and ends Harbor's agent command with exit 143, and Harbor then grades while
the session works on (seen in all four sam-cell-seg trials of 2026-11-12 and 2026-11-13, each within a second of
such a command). `instruction.j2` renders the instruction reversed, so no word of the task stands on that command
line; the arm reverses it back by code point, which Jinja's `reverse` inverts exactly. A command that kills by the
runner's own name (`python`, `acp_runner`) still reaches it.
