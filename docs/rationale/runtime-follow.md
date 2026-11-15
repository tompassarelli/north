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
- **brokers**: an on-demand `gh-broker` or user credential broker exits at its next moment with no call open, as it
  does after idling, and the next client starts the new runtime's. The resident `gh-broker` keeps answering every call
  that reaches its socket until a moment with none open and none waiting (at most 60 s), then exits, and its unit, which always restarts, starts it on the link.
- **timers**: every north timer unit runs `current/tree/bin/north` as a oneshot, so each tick starts on the runtime
  current names.
- **session hosts** (`north session-host`): re-exec in place, so the session they hold survives the move. Every 2 s
  the host asks `replaced`. On a move it stops taking clients and resting, gives clients already connected `DEFER` to
  finish, writes the frames still queued to the provider, and stops reading the provider's output at a line boundary,
  leaving anything unread in the pipe. It then writes its frame counter, idle
  time and the turn watch's state (open turns, replies read so far, quiet children, checkpoints and unaccepted prompts) to
  `handover.json` in its state directory. It passes the provider's three pipes and its listening socket over a socket pair
  that becomes its standard input, the one descriptor the new image can claim without unsafe code, and re-execs with
  `--adopt`. The provider stays its child under the same pid and never notices. Clients that connect meanwhile wait in
  the socket's backlog for the new image. If the exec fails, the host serves again on its old runtime and says why.
  Before it winds down, the host checks that the next build's `session-host` usage names `--adopt`, so a rollback to a
  build that predates adoption leaves the session serving on its runtime instead of ending it.
- **tool servers** (`north --tool-server VERBS`): the process a session's Claude Code starts to serve `finish` and `hand-in`, which runs each call as `north VERB` from its own binary, so a server on an old runtime refused a verb the promoted one accepted. Between calls, while no request is read, it asks `replaced` before it waits for the next request and every 2 s while it waits, and on a move re-execs in place. The pid and the stdin and stdout pipes stay; a request sent meanwhile waits in the pipe and the new image answers it once. A call in flight runs to its end on the runtime it started on, and the server moves after it. `north runtime lag` lists a tool server as kind `tool-server`.
- **on-demand host services** (the user broker, the worker-sandbox broker, an on-demand `gh-broker`): they serve every client of the host, so a client starts the user broker from the runtime `current` names and each follows `Follow::service`, which reads the host's `current` whatever link its starter followed. A worker-sandbox broker on a socket of its own (its socket named by `NORTH_OUTSIDE_BROKER`, as a canary probe starts it) follows the link its starter followed, `Follow::current`, so the build a canary judges stays judged; a successor it starts for the promoted north runs on the same socket and follows the same link. On 2026-11-14 a user broker the lander started from its own runtime followed the lander link; the walk's canary found it on that older runtime as a host fault, so no runtime walked and the lander link, the one move that would retire it, never moved.
- **canary probes**: each probe runs with `NORTH_RUNTIME_LINK` set to `north/judged/SHA`, a link that names the runtime
  being judged. A probe worker's session host therefore stays on that build while `current` or the lander link still
  names another one. On 2026-11-11 the lander's canary rejected the first version of this change because a verify-north
  worker's host followed the old link, exec'd a build that has no `--adopt`, and exited before its first turn.

## Seeing it

`north runtime lag --processes` lists each long-lived north process on the host: its kind, its subject (session or
repository), the runtime it runs and `current`, or `behind` with the runtime its link names. It reads the runtime from
the program path in `/proc/PID/cmdline`, then from `/proc/PID/exe`. When neither names one, a process that started
through a link after the link last moved counts as running that link's runtime. Any other process runs an older one.

The canary reads the same list on every promotion (`north runtime probe services`): a host service (a broker, a lander,
a watchdog, the offload taker) on a runtime this host ran before the one the promotion replaces follows no promotion, so
the probe names it as a host fault and the runtime is neither passed nor rolled back. One on the replaced runtime is on
its way, and a session host, session link or tool server behind is counted and left to its session (north#1022).

A unit restarts the service it keeps, but nothing restarts one a client started: the user broker and a GitHub broker
started on demand exit by themselves only if the runtime they run already knew to. So a promotion retires them: before
the probe judges, and again once the runtime is live, each such service on a runtime that is neither live nor judged
loses its socket path, which sends its next client to start one on the live runtime, and gets a TERM once no client has
been connected for 5 s. Its log is no witness of that, since the one its next client starts writes to the same file.
One still serving a client after 15 s keeps running without new clients and is asked again by the next promotion; a
request is never cut. The probe names such a draining service and passes: holding the host on its old runtime for a
process no new client reaches only widens the lag. It fails for one that still takes new clients (north#1022).

The code that judges a runtime is that runtime's too. A promotion is run by the runtime that was live before it, so
its probe list and its walk were one runtime behind what they judged: a probe retired on purpose kept failing the
commit that retired it, and a fix to the walk first ran one promotion late. So the promoting runtime asks the judged
one for its probes (`north runtime probe list HOST`), and after its swap it runs `north runtime walk` from the runtime
now live and waits for it. A runtime that knows neither verb is judged and walked as before, by the one promoting it,
and so is one that dies in its own walk (a signal or a panic), since nobody else would walk it or roll it back.
What a candidate drops from the list is the reviewer's to read: an order prints the probes added and removed against
the live runtime's list before any probe runs, and the probes it keeps by name with another script, each script read
with its own commit left out, so a probe emptied under its old name shows as changed (north#1022).

`north runtime lag --json` answers the fleet-wide question (north#1081): a JSON array with one object per long-lived
process on every declared host, `host`, `process`, `runtime`, `linked` and `seconds`. The settling host asks each other host for
its runtime link, the link's mtime and its own list with `north runtime lag --processes --json` over ssh, and `seconds`
is how long the process has stayed behind since its host's runtime link swapped: a process on the linked runtime or one
built on it reads 0, any other process, one whose runtime cannot be named too, reads the seconds since the swap, so a
promotion pages nothing while services restart onto the new link (north#1540). A host a standing review hold keeps off
the pin reads 0 with the hold in `hold`, and a host that did not answer is one
object with a null `seconds` and its reason in `unread`. The settling host's fleet tick starts `north runtime lag --sample` in a scope of its own, at most once a minute, so the
reading does not stop with the stable pin. Each sample appends the worst reading to `runtime-lag.jsonl`, which the
`ready-runtime-lag` bar reads (at most 5 min). A session process more than `runtime_promotion_slot_sec` behind since its host's swap (it keeps the runtime it started on by design) pages the coordinator once, keyed
`runtime-lag`, and the episode ends when no session process does. A host service (a broker, lander, watchdog or the
offload taker; a worker sandbox broker is a session's own) pages the coordinator by name, keyed `runtime-lag:HOST:PROCESS`, each
time it has stayed behind past 10 min since its host took the new runtime, and a cleared notice ends its episode once it is back under 10 min; an unreadable host keeps its episodes open. Open episodes live in `runtime-lag-episodes.json`.

## The lander's landing-speed watch

After the lander link moves to a new runtime, the lander judges its next landings (north#1631). It points the link back to the
runtime it held before, restarts its serving units and alerts the coordinator once when the first three landings each took more than
`SLOW_SECONDS` from queue head to main, or when a ticket waited `STALL_SECONDS` with no landing while it was queued. A ticket whose
checks main also fails does not count toward the stall, since a red main is no runtime's doing. The record lives beside the link
hold, and a record whose held runtime lies outside `runtimes/` is not followed.

The three bounds are guesses, not measured: `SLOW_SECONDS` = 120, `LANDINGS` = 3 and `STALL_SECONDS` = 600 were chosen from the 2026-11-15 slow
lander and its 25-minute outage. What would settle each: the head-to-main latency of the landings in the ledger for a week after promotions
(`SLOW_SECONDS`, `LANDINGS`), and the longest gap between landings on healthy days with tickets queued (`STALL_SECONDS`).
