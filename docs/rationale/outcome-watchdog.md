# Outcome watchdog

On 2026-11-07 four flows stopped for hours while their own checks passed: finished runs stopped reaching the ledger
host while the recorder exited 0 and `north health` printed `snapshot: ok`; every host's policy generation was held
from 6:16 AM with an alert that named no cause; a gex45 profile launched sessions with no settings.json; and a host
deploy restarted the dispatch timer the proxy had stopped. Each writer reported success, so nothing read the outcome.

`north health` now judges each critical flow by its recorded outcome (`crates/north-health/src/health/flows.rs`):

- `run-recording`: the ledger host's newest recorded run against the newest fleet stream write on any host, and each
  host's oldest unsent spooled ledger write.
- `snapshot`: the laptop's ledger snapshot age (pulled every 15 minutes) and its last pull failure.
- `policy`: each host's held-generation record (`agents-sync-unpublished`) with the gate's refusal line.
- `hooks`: every config directory a launch on each host runs in registers hooks in its settings.json.
- `dispatch`: the dispatch timer's state against the pause the proxy records with `north dispatch pause --reason`;
  a paused dispatch also launches nothing, whatever the timer does.
- `landing`: how long the oldest origin/main commit missing from each host's runtime has been on main.
- `host-probes`: each host answered `north health facts` over ssh.

Each host prints its own facts with `north health facts`; the laptop's `north health` (every 2 minutes) reads them,
prints every flow with its age, and the existing episode alerts page the proxy after two failing ticks with the flow,
the age and the cause. One table of flows replaces a script per flow, and its thresholds sit beside the judgement.
