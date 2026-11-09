# Outcome watchdog

On 2026-11-07 four flows stopped for hours while their own checks passed: finished runs stopped reaching the ledger
host while the recorder exited 0 and `north health` printed `snapshot: ok`; every host's policy generation was held
from 6:16 AM with an alert that named no cause; a gex45 profile launched sessions with no settings.json; and a host
deploy restarted the dispatch timer the proxy had stopped. Each writer reported success, so nothing read the outcome.

`north health` now judges each critical flow by its recorded outcome (`crates/north-health/src/health/flows.rs`):

- `run-recording`: the ledger host's newest recorded run against the newest stream write of a session whose host
  has exited on any host (a worker still running has no run to record yet, north#963). A session host records its
  conversation's run as it exits; the worker-ledger timer is the backstop, so the allowed lag is that timer's interval,
  read from systemd on the ledger host, plus five minutes (a flap at 12 to 16 minutes on 2026-11-08 came from a fixed
  10-minute limit against a 15-minute timer).
- `snapshot`: the laptop's ledger snapshot age (pulled every 15 minutes) and its last pull failure.
- `policy`: each host's held-generation record (`agents-sync-unpublished`) with the gate's refusal line.
- `hooks`: every config directory a launch on each host runs in registers hooks in its settings.json.
- `dispatch`: the dispatch timer's state against the pause the proxy records with `north fleet dispatch pause --reason`;
  a paused dispatch also launches nothing, whatever the timer does: the code-health review tick, the timers that launch
  a fleet session and the dispatch launches waiting in a laptop queue (`north fleet dispatch queued`, which also drops
  a launch whose issue closed or whose box was ticked while it waited) each check the pause when they start. The
  south dispatch timer carries `ConditionPathExists=!~/.local/state/north/dispatch-paused`, so a deploy starting it
  under a pause leaves it inactive (north#960).
- `landing`: how long the oldest origin/main commit missing from each host's runtime has been on main.
- `state-machines`: every explicit state machine's record on every host: a state past its dwell, one whose holder
  died, and a move the machine does not have. A host whose threads spool the ledger host refuses for good
  (`remote-serve`'s exit 126) gets one finding per refused verb, naming the verb and the host's reason, however many
  writes wait behind it (north#957).
- `host-probes`: each host answered `north health facts` over ssh.

Each host prints its own facts with `north health facts`; the laptop's `north health` (every 2 minutes) reads them,
prints every flow with its age, and the existing episode alerts page the proxy after two failing ticks with the flow,
the age and the cause. One table of flows replaces a script per flow, and its thresholds sit beside the judgement.
