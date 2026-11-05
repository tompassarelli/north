---
name: machine-capacity
description: >-
  Run builds, full suites, renders, Warcraft clients or other multi-core or >1 GiB work under capacity admission; DEFER or resource pressure. Skip small checks.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Machine capacity

## Wrap heavy work

- Invoke the helper with Bun at `$(dirname "$(agents path machine-capacity)")/scripts/machine-capacity.mjs` (written `machine-capacity` below), because every caller must reach the one shared admission state.
- Admit sustained multi-core or >1 GiB work through it, never through worker slots, because idle slots measure neither CPU nor memory.
- Run cargo build, full `bun test` suites (inside the landing, never as a pre-landing step), `bun wisp map build` and headless wisp renders (`view`, `headless --render`) through `run`; cargo test, tsc --build and multi-file bun test route through admission automatically; cargo check and single test files stay local.
- Use `machine-capacity run --class CLASS --owner OWNER --timeout-seconds N -- COMMAND ARG...`, with the timeout covering setup and downloads, because every run needs a finite bound. Run `machine-capacity estimate -- CMD` before staffing local work, because leases size from it.
- Supervise the wrapper, queued or running, through terminal `RELEASED` and never detach its work, because a dropped wrapper drops its ticket and a killed one leaves its allowance charged.
