# Lander doctor: every known stuck state has a bound and a remedy

On 2026-11-06 the lander was stuck or degraded five ways, each found by a person or the proxy: gex45's disk at 99% queueing builds, offload slots held by hung jobs, the lander link held for three hours, a missing registered worktree failing checks, and runtime switches failing checks. The lander's own watchdog runs inside its rounds, so it cannot see a lander that stopped running rounds, and it repairs only its own worktree and unit. Tom asked for a lander that never gets stuck (north#634). `north land doctor` reads the lander from outside, every 2 minutes on the lander host, as a pre-step of the `north-land-shadow` timer.

## Bounds

Each run prints every bound with its live value and limit:

- oldest queued lane: 20 minutes, the watchdog's stale-lane line;
- minutes since the last landing while lanes wait: 30;
- round silence while lanes wait: 15 minutes since the newest write of `<queue>/watchdog/rounds.json` or any file in `<queue>/logs`, which a lander writes every round and while a check streams its log;
- lander link lag: 2 hours between the commit the lander link names and main's newest commit;
- held minutes: 15, from `lander-held`, or from when current moved when there is none;
- each held land-offload slot: the land-check ceiling plus 15 minutes, the grace the lander's own offload deadline adds; background slots hold mutant sweeps for hours and are left out;
- free disk: 10% on the lander and each host `north capacity offload hosts` names;
- stale worktree registrations: 0 under `~/code/*/main` on each of those hosts;
- land checks refused in the last 2 hours whose verdict or check log names an infrastructure signature (a missing registered worktree, a runtime missing on the check host, a host lacking the commits, a full disk): 0.

## Remedies

A known state has one remedy, applied once per episode, the run of rounds a bound stays over:

- missing registered worktree: `git worktree prune` in that repository on that host;
- slot past its deadline: end the process group its `.pgid` names, only while the slot's lock is still held and `.pgid` still names that group, and remove its holder files;
- infrastructure failure: requeue the ticket once, marked in `<queue>/doctor/reran/`, unless the lane was queued again;
- disk pressure: `north capacity reap-orphans` on that host, which evicts unlocked offload caches;
- round silence: restart `north-land@PROJECT.service`; still silent 5 minutes later, roll the lander link back to the newest build that landed a batch green, name the bad build in `lander-refused` so `north land promote` leaves it off until current moves on, and restart the lander units.

A build counts as good once an after-landing run that started after the lander link moved to it was green; `~/.local/state/north/land-doctor/book.json` keeps the newest five with each open episode.

## Unknown states

A bound over with no remedy (rounds run but nothing is decided, the runtime stops following main, a host cannot be read, a slot names no process group), or a remedy that left its bound over 5 minutes after it ran, is an unknown state: the doctor sends the proxy one fleet message per episode and records one `lander-unknown` incident row. A held link is a known state: `north land promote` already records it and alarms the proxy once per hold, so the doctor only reports it.

## Incidents

Every remedy writes one incident row, `noticed_by = lander-doctor`, with a class per state (`lander-stale-worktree`, `lander-slot-overdue`, `lander-infrastructure-failure`, `lander-disk-pressure`, `lander-wedged`, `lander-rollback`), so the policy loop counts recurrences by class and a recurring remedy points at a cause to fix.
