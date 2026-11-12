# Eval X: the shell relay

Both harnesses and Claude Code stay on the host; only the shell tool's commands reach the task's container. No
credential and no model traffic enters a container.

## How Claude Code calls its shell

Read from Claude Code 2.1.322 and seen in a live north session:

- The shell tool runs `SHELL -c [-l] STRING`. `STRING` is a preamble (the shell snapshot and a few `unalias`
  lines), then `eval 'COMMAND' < /dev/null`, then `&& pwd -P >| FILE`, where `FILE` is a file under the host's
  temporary directory that Claude Code reads afterwards to follow a `cd`.
- A replaced shell is taken only when its path contains `bash` or `zsh`; any other path is skipped without a word.
- Hooks run through the same shell, as `SHELL -c HOOK-COMMAND`.
- With `CLAUDE_CODE_SHELL_PREFIX` set, as north sets it to its worker sandbox, `STRING` is `PREFIX 'STRING'`, and the
  sandbox then runs `SHELL -c STRING` itself. So one replaced shell serves plain Claude Code and north alike.
- Some commands reach the shell in a variable named `__CLAUDE_CC_...` instead of in `STRING`.

## What is here

- `container-bash`: the shell both arms give Claude Code, as `SHELL=.../evals/x/relay/container-bash` with
  `EVAL_X_TRIAL` naming the trial directory. A call whose string ends in the `pwd -P >| FILE` clause, and does not
  start with a program path, is a shell-tool command: it goes to the broker and runs in the trial's container, in
  the directory Claude Code started it in, with `GIT_EDITOR`, `CLAUDECODE` and every `__CLAUDE_CC_` variable carried
  over and nothing else of the host's environment. Its output, exit status and final directory come back. Every
  other call (hooks, north's sandbox prefix, the shell snapshot) runs on the host in the real shell. Without
  `EVAL_X_TRIAL` everything runs on the host. With `EVAL_X_SHELL_LOG=FILE` each call is appended to that file with
  where it ran.
- `broker TRIAL`: a separate process the runner starts, one per trial, before Harbor, and stops with
  `broker stop TRIAL`. It serves the relay's requests through `podman exec`, so both arms reach the container the
  same way whatever each harness wraps its commands in. It finds the container as the running one that mounts the
  trial directory, so nothing a task can write chooses it, and looks again at each start, so a trial directory used
  twice never meets the container of its first use. When the relay dies (Claude Code kills it at a time limit
  or on a stop), the broker kills the command's process group inside the container.
- `check`: the relay and broker against a stand-in for podman, with no container: output, exit status, working
  directory, a carried variable, host-side calls and a kill.
- `real-session TOKEN-FILE [MODEL]`: one `claude -p` session on the subscription token in that file, with the relay
  as its shell, one hook and a real container. It needs a login and podman, so it is started outside a worker
  session. A pass prints `relay-check-box` for step 1, the same directory for steps 2 and 3, the note read back on
  the host, hook calls counted on the host, a time-limit ending for step 6 (a loop, because Claude Code refuses a bare
  long sleep itself), and nothing but `sleep 900` left in the container.

## Where things live

With `T = W/trials/JOB` (mounted in the container at `/eval-x`): requests go through `W/exec/JOB/`, which no
container mounts; the container writes each command's process id and final directory to `T/exec/`. Neither is
removed by the relay or the broker; both go with the trial.

## Known difference from a shell on the host

A `cd` to a directory that exists only inside the container, such as the task's own `/app`, does not carry over to
the next call: Claude Code resets to the session's directory, as it does for any directory it cannot see. Both arms
get the same sentence naming the session's directory as the task's directory.
