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
  `v0.25.0` (`fdba28fc411e`) under `/tmp/eval-x` (or `EVAL_X_WORK`), installs Harbor from its own lock and prints
  its version. `host oracle TASK` runs the benchmark's reference solution for one task through Harbor and the
  task's own grader, and prints one graded row, the digests of the images pulled and the disk they take: it proves
  a host can run the benchmark before any agent is involved. `host harbor ARGS...` runs Harbor with the same pins.
  Harbor's telemetry is off in every call and nothing is uploaded.

## Where it runs

A container needs the full range of subordinate user ids, which a worker's sandbox cannot map, so `host oracle` and
`host harbor` are started as a user unit outside a worker session, from a checkout of main:

    systemd-run --user --collect --unit=eval-x-probe ~/code/north/main/evals/x/host oracle fix-git

A pass is a row with `"reward":{"reward":1}` and `"error":null`.

## Not here yet

The two arms. Each harness stays on the host as in Eval A and only its tools reach the task: shell commands run in
the task's container through the runtime's exec, and the file tools see the task's working directory through a
mount of it on the host. Model traffic never passes through the container.
