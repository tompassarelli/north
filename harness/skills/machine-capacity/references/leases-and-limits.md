# Capacity leases and limits

Contents:
- Why admission is shared
- Worker leases
- Interactive session lifetime
- Lease rules

## Why admission is shared

Idle agent slots do not measure local CPU or memory capacity. A shared atomic
lease prevents several individually reasonable commands from starting together
and exhausting interactive headroom. Cgroups bound descendants as well as the
original command; an estimated duration does not provide that containment.

## Worker leases

Resolve the helper as in `SKILL.md`. Reserve before admitting a
compute-using child, renew before its bound expires, and retain the release:

```bash
north capacity host reserve --class agent --owner "codex:/root/task" --timeout-seconds 1800
north capacity host renew --lease LEASE --owner "codex:/root/task" --timeout-seconds 1800
north capacity host release --lease LEASE --owner "codex:/root/task"
```

These values illustrate a bounded run, not a deadline prediction. Persistent
reservation is limited to `agent`; all larger classes use `run` or `session`.
A settled child missing release evidence leaves the parent responsible for
releasing the exact known lease. A prose report is not a release receipt.

## Interactive session lifetime

Use `north capacity host session --class native --new-clients N --owner OWNER -- COMMAND ARG...`
for a foreground client or desktop intended to remain until explicitly
stopped. It sets systemd's runtime limit to infinity but retains the same atomic
admission, aggregate CPU quota, memory ceiling, and descendant containment.
Command exit and SIGINT/SIGTERM/SIGHUP to the wrapper stop the exact scope and
produce `RELEASED`. Never detach its command tree. A forcibly killed wrapper
cannot perform cleanup; its still-live scope remains accounted until its owner
stops that exact scope. No automatic restart or timer extension is implied.

Private desktops use this mode by default. `--seconds` selects a finite desktop
deadline instead. An enclosing finite scope still imposes its deadline, so
launch a retained desktop directly or inside a `session` scope. Existing
already-running launchers keep their captured timers; changing the installed
source does not migrate them.

## Lease rules

- Reserve an agent lease before compute-using workers, renew before expiry and release at settlement, because the lease is the only record that frees the allowance; it charges 768 MiB without reserving local CPU.
- Give worker local commands their own run scopes and persist reservations only for the agent class, because larger classes cannot outlive their wrapper.
- Keep every descendant inside its scope and never detach work or renewal processes, because a forcibly killed wrapper cannot clean up and its allowance stays charged until its scope ends.
- Retain allowances while a wrapper or scope is live and within its deadline, including native sessions, because a live scope is real load; a run lease past its deadline has its scope killed and is reclaimed.
