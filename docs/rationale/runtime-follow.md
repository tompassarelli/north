# Runtime follow: every long-lived north process runs the promoted runtime

On 2026-11-10 the laptop's lander link moved to 9a896708b0 at 4:51 PM and the lander kept running cb929a45e8 until it
was restarted by hand at 5:05 PM. The landing changed no lander input, and the lander's own runtime watch restarted only
on a change to one. At 5:00 PM the session hosts and fleet watchdogs on the same host still ran 8917932b, two promotions
back, because nothing told them current had moved. Tom asked the same day that a landed update reach every process as
fast as the machine allows, and that a stale runtime be impossible by construction rather than caught by an alarm
(north#1081).

## One mechanism

The `north-runtime-follow` crate is the only place a process learns that its runtime was replaced. `Follow` records the
runtime a process started on: the `runtimes/SHA` directory its executable lies under, or for a build outside `runtimes/`
the runtime its link named when it started. The link is `NORTH_RUNTIME_LINK` when a unit sets it (the lander units name
the lander link), else `current`. At each safe point the process asks `replaced`, which reads the link and names the
runtime it points to when that is not the one the process runs. The process then moves in one of two ways:

- **re-exec in place**: `reexec` replaces the process image with the same program under the link's runtime and the same
  arguments. The pid, the children and every descriptor not marked close-on-exec stay. Use it for a process whose state
  is on disk or in its arguments and that no unit restarts, such as a fleet watchdog, which runs in a transient scope;
- **exit to its unit**: the process returns, and its unit, which runs the program through the link, starts it on the
  new runtime. Use it for a process whose unit's control group has to reap what it leaves behind. The lander is one:
  a check it abandons would keep writing to the worktree the restarted lander resets.

`due(idle)` is the bound for a process that waits for idle: once its runtime has been replaced for 45 s (`DEFER`), it
moves even while busy, so it moves within a minute of the flip.

## Safe points by process

- **lander** (`north land serve`): the serve loop polls every 2 s. On a move it stops taking rounds, gives a landing
  already on its way to main at most 30 s, and exits. A check in flight is rerun from the journal (north#948). It no
  longer compares the lander inputs of the two runtimes, so a landing that touches no lander code moves the lander too.
- **fleet watchdog** (`north fleet watchdog NAME`): re-execs between its 1 s polls of the session host it watches. The
  new image reads the same session record and watches the same host pid.
- **capacity watchdog sampler, worker-sandbox broker, offload taker**: these already exited when `current` moved. They
  now ask `Follow`, which replaces `north_binary::superseded`. The broker hands new requests to a successor at once.
  The taker still waits until the runs it serves have ended.
- **timers**: every north timer unit runs `current/tree/bin/north` as a oneshot, so each tick starts on the runtime
  current names.
- **session hosts**: not moved yet. A session host holds its provider's pipes, the socket clients write frames to, the
  queue of frames not yet written and the frame watcher's turn state. Moving one without dropping its session means
  handing all of that to the new image, or splitting the host. That choice is open on north#1081.

## Seeing it

`north runtime lag --processes` lists each long-lived north process on the host: its kind, its subject (session or
repository), the runtime it runs and `current`, or `behind` with the runtime its link names. It reads the runtime from
the program path in `/proc/PID/cmdline`, then from `/proc/PID/exe`. When neither names one, a process that started
through a link after the link last moved counts as running that link's runtime. Any other process runs an older one.
