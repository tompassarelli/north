---
name: machine-capacity
description: >-
  Run builds, full suites, renders, Warcraft clients or other multi-core or >1 GiB work under capacity admission; DEFER or resource pressure. Skip small checks.
grounded: 2026-11-12
written: 2026-11-12
metadata:
  kind: playbook
---

# Machine capacity

## Wrap heavy work

- Invoke the helper as `north capacity host`, because every caller must reach the one shared admission state.
- Admit sustained multi-core or >1 GiB work through it, never through worker slots, because idle slots measure neither CPU nor memory.
- Run `north capacity host run --class CLASS --owner OWNER --timeout-seconds N -- COMMAND ARG...`, the timeout covering setup and downloads. Run `north capacity host estimate -- CMD` before staffing local work, because leases size from it.
- Supervise the wrapper, queued or running, through terminal `RELEASED` and never detach its work, because a dropped wrapper drops its ticket and a killed one leaves its allowance charged.
- Retry a DEFER after a known release or 30 seconds and never busy-poll, because polling adds the load the queue exists to avoid.
- Treat RUN/RESERVED as permission to continue, NESTED as a run under the batch lease it started from, and RECLAIMED as expired leases, never as permission to kill peers, because peers belong to their owners.
- Never lower correctness requirements because of pressure, because slower correct work beats fast wrong work.

## Classes

Choose the smallest sufficient class, because oversized claims queue peers for nothing.

- moderate = 2 CPUs/2 GiB; heavy = 6 CPUs/8 GiB (all cores below 8, 3 at 8); exclusive = all allowed cores, no peer batch, 15 minutes at most; gpu = 1 CPU/2 GiB for headless Chrome and wisp renders (not grim captures), as many at a time as the host's measured `gpu_slots` (two on the laptop) while its free VRAM keeps Warcraft clients room; native = 2 CPUs/4 GiB for Warcraft clients and private desktops.
- Omit `--class` to size from the usage log (p90 of the last 20 runs plus 25%; moderate until three runs exist), because measured sizing beats guessing.
- critical only for the landing-train holder or a run with an explicit `--critical` release-blocker diagnostic, because it jumps the queue and ignores pressure.
- Add `--priority tom-ask` to a moderate, heavy or gpu `run` or `session` whose output Tom is waiting on: it takes the place of the oldest ticket of its class and still obeys pressure, and each use is recorded.
- runtime only for `north runtime update` building a landed north runtime, because every landing waits on that build (north#308).
- Leave the game reserve and gaming hold to the automatic modes: `north capacity mode status` names away, desktop or play (a running game, or Tom saying he is testing or playing until he says he is done or two hours pass) and why.

## Read when

- Read [native and GPU](references/native-and-gpu.md) before starting Warcraft clients, a private desktop, a native evidence run (accept, gate runs, capture batches, soaks), or GPU work.
- Read [diagnosing pressure](references/diagnosing-pressure.md) when admission defers longer than you expect, a build ran on another host, the desktop is slow, or you must find who holds the machine.
- Read [leases and limits](references/leases-and-limits.md) before reserving an agent lease for a compute-using worker, or when a scope or lease outlives its wrapper.
- Read [admission details](references/admission-details.md) for exact thresholds, charging and queue rules, or before changing admission.
- Read [testing on this machine](references/testing-on-this-machine.md) for the local test runners, timing checks and review paths the verification skill leaves unnamed.
