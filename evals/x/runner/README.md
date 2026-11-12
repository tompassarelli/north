# Eval X runner

Plans and runs the Terminal-Bench 2 trials of Eval X (south#107; the registered design is the assessment's `EVALS.md`,
section 11). It starts no model itself: each trial runs one arm (`evals/x/arm`) in the background, Harbor runs the
task through `evals/x/host harbor run` with the hand-off arguments from `evals/x/handoff/args`, and the runner
joins Harbor's result with the arm's record and a reading of the session's transcript into one row per attempt. The
arm gets `--limit` set to the task's agent timeout from `task.toml` (the fourth column of `tasks.tsv`). Before Harbor
starts, the runner runs `evals/x/relay/broker TRIAL_DIR` in the background, with the trial directory's canonical path,
and after the trial runs `evals/x/relay/broker stop TRIAL_DIR`. The container mounts `WORK/trials/JOB`, which holds
only the hand-off and the relay's files; everything a harness writes (its configuration, state, transcript and logs)
goes to `WORK/arms/JOB`, which no container mounts. When Harbor returns, an arm that took the hand-off gets
two minutes to end by itself; then its process group is stopped.

## Commands

Run from a checkout of main, on the host that runs the trials, as a user unit. `evals/x/run` builds the runner from
its lockfile in north's dev shell and runs it:

    evals/x/run plan
    evals/x/run smoke TASK --account A --token-file FILE
    evals/x/run noise --account A --token-file FILE --in-flight 1
    evals/x/run full --account A --token-file FILE --in-flight 1
    evals/x/run stop

- `plan` prints every planned trial with its seed and the two counts: 178 full-run trials (89 tasks, both arms of
  a task back to back, the first arm drawn per task from the seed) and 45 noise-floor trials (15 seeded tasks, the
  plain arm three times each), which run first.
- `--in-flight N` sets the trials in flight; two trials of a task with `memory_mb` of 8192 or more never run together.
- `--account A` names the one claude plan account every session runs on; a value with a colon is refused.
  `--token-file FILE` names a file holding that account's subscription OAuth token, which the plain arm logs in
  with; the runner passes the file's name on and never reads the token.
- `smoke TASK` runs both arms of one task and, like every run, prints each row as it is written.
- `stop` signals the run, stops every other `eval-x-*` unit, and ends every process whose command line, working
  directory or environment names `WORK/trials`, `WORK/arms`, `WORK/jobs` or `WORK/ws` (TERM, then KILL after five
  seconds) and every container of a task image or with a mount under those, reaching podman through
  `evals/x/host podman`. It then prints what remains of each and exits non-zero if anything does, or if containers
  or units could not be listed.

Other options: `--work DIR` (default `$EVAL_X_WORK`, else `/tmp/eval-x`), `--seed N`, `--max-load F` (default 2),
`--min-5h-headroom P` and `--min-week-headroom P` (default 20 and 10 percent left), `--max-reading-age S` (600),
`--max-reruns N` (2).

## Gates before each trial

- Load: the one-minute load from `/proc/loadavg` is under `--max-load`; the runner waits otherwise.
- Headroom: the plan line of the named account in `north accounts --json` must show a fresh reading (within
  `--max-reading-age` seconds of its `at`) with five-hour and weekly headroom at or above the thresholds; a missing,
  stale or unreadable reading waits for the reset. The runner never accepts or asks for extra usage.
- Environment: the run refuses to start while any `ANTHROPIC_` or API-key variable is set.
- Image: each task's image digest is recorded at its first pull in `WORK/images.tsv`; a later digest stops the run.

## Rows

`WORK/runs/RUN/rows.jsonl` and standard output, one line per attempt (`"kind":"trial"`), and one per unit
(`"kind":"unit"`) giving its passes and whether it is valid. A trial row holds, unchanged, what Harbor wrote after the
task's own grader ran (`harbor.reward`, `harbor.exception`, the agent's start and finish), beside the arm's record
(`arm_result`), the wall time, the seed, load, headroom reading, host, the image with its digest and size, the disk the
workspace, trial and job directories take, the label `HOST-TOOLS`, and under `stamps` the commits of the tasks, Harbor,
this checkout and the north runtime, and the versions and SHA-256 of the T3 Code and Claude Code binaries. The Claude Code binary is the installed client
itself (`EVAL_T_CLAUDE`, else `CLAUDE_RUNTIME`, else `~/.local/lib/claude/current/bin/claude`), which the runner hands
to both arms, because `claude` on a north host's path is a launcher that needs the caller's own home.

`container_calls` counts the shell-tool commands the relay ran in the task's container for this trial (one directory
each under `WORK/exec/JOB`); a trial whose session ran commands but shows none here ran them on the host.

`session` is read from the session's Claude Code transcript: the effort its requests ran at, the request count, the
tokens summed once per reply (input, output, cache creation, cache read), whether a reply was a rate-limit refusal,
and every north guard that fired: a tool call a hook refused (`refused`) or let through with a note or a rewrite
(`changed`), each with its time, the call and what the hook said.

`first_request` shows what instructions a session started under, read from its system text and the messages before
its first reply: the instruction files Claude Code names there (`Contents of PATH (LABEL)`), and of the lines of 60
characters or more in the policy file (`--policy FILE`, default `~/.claude/CLAUDE.md`), how many were searched for and
how many the first request holds. A plain-arm row that names an instruction file outside the task's workspace (or the session's link to it), holds
any policy line, or records no first request, is invalid (`policy_in_plain_arm`): that arm was not plain.

An attempt that met a rate limit is invalid and rerun. One with no Harbor result, no arm record or policy text in the
plain arm is recorded invalid and not rerun. A pair whose two arms ran at different efforts is rerun as a whole, and a unit with an effort that
could not be read is recorded invalid.

## Pins

The task table `tasks.tsv` is the 89 tasks of `2fd12b88aafd` with their memory, image and agent timeout in seconds;
Harbor is `fdba28fc411e`.
