# Learning loops: the registry of every automatic decision north makes

The loop registry, `harness/loops.toml`, lists each decision north makes without a person (the tier a box gets, the ETA and budget its brief states, the cores a lease charges, the deadline a land check gets, which machine a launch runs on) and states for each what it learns from, what updates it and how often. `north loops check` reads the registry against the machines' state and fails when a learning loop has stopped learning. Decided 2026-11-05 under north#112.

## What a loop is

A `[[loop]]` entry names the decision, the source files that make it (`sites`), the outcome it would learn from (`signal`), the program that updates it (`updated_by`), how often (`cadence_hours`), and the record whose newest write is its last update (`record`, a path under the state root, `XDG_STATE_HOME` or `~/.local/state`). A record that is a directory counts as written when any entry in it is.

## Closed and open

- A **closed** loop has a `signal` and a `record`: an automatic program measures an outcome and rewrites the decision. `north loops check` fails it when its record is older than twice its cadence (STALE), or when the record is missing (never updated).
- An **open** loop has no `signal`: its decision is a constant, a table or a live reading that nothing yet adjusts from outcomes. It must name an `owner`, the box that closes it. An open loop prints as `open` and does not fail the check, so the registry can list what is still hand-set without raising an alarm every day. An open loop with no owner does not parse.

The rule is that nothing learns silently and nothing is hand-set without a box that will change that. Closing a loop means turning its `owner` into a `signal` and a `record`.

## Where a record is read

State lives on more than one machine: the ledger and the tune cells on the ledger host, the lander's queue and check timings on the lander host. Each closed loop therefore names a `host` role:

- `ledger`: the host in `~/.config/threads/remote`, or this host when that file is absent.
- `lander`: the host `git config -f <harness>/.safe-push --get landing.lander-host` names.
- `here` (the default): the host running the check.

No host reads another: the gex45 and nexus access decisions keep those hosts pull-only, with no keys to reach the laptop, so an ssh read of the lander host from nexus could never succeed (2026-11-05). `north loops check` reads from the local files only the closed loops whose role this host holds, and lists every other closed loop as `checked on ROLE`, which does not fail. An unresolvable lander host means this host does not hold the lander role. `north loops check --role ROLE` checks only the loops of that role, read locally, whatever this host is named.

Each role is checked where its records live. The daily `north tune` on the ledger host checks the ledger and here loops. On the laptop, the lander host, the hourly north-tempo user timer in nixos-config (modules/north-tempo) runs `north loops check --role lander` after recording tempo, so a stale lander loop fails that unit.

## The tune tick fails on a stale loop

`north tune` runs the check after its own work and appends the registry's lines to its output. Its exit code is 1 when any closed loop it holds is stale or never updated, or when the registry cannot be parsed, which fires the nexus unit's failure alert. The same code that fails `north tune` fails `north loops check`.

## Completeness

The registry is kept complete by a test in `crates/north-dispatch/src/loops/inventory_tests.rs`, not by comments at the decision sites. It scans the dispatch, fleet-launch, placement, capacity-admission and lander source directories (leaving out test files and the text after `#[cfg(test)]`) for numeric `const` items and for the keys of `default(...)`-style reads of `harness/defaults.toml`. Each must be named, as `path::NAME` or as the key, in some loop's `constants` or `defaults`, or in the `[plumbing]` lists at the end of the registry, which hold values that do not decide anything (protocol limits, buffer sizes, wait intervals). A name the scan no longer finds also fails, so the lists cannot go stale.

A new constant under those directories therefore stops the build until someone says which decision it belongs to or that it is plumbing. Placing it on a loop makes it visible as a hand-set input of that loop. Rejected: marker comments at the sites (the repository allows no code comments), a count ratchet (it hides which constant was added), and a hand-kept function list alone (a new constant would pass unseen).

## Adding a loop

1. Add a `[[loop]]` with `name`, `decision`, `sites` (files that exist), `updated_by` and `cadence_hours`.
2. If the decision already learns, add `signal` and `record` and the `host` that holds the record, and make the updating program write the record on every run, including a run that changes nothing, since the record's age is the proof the loop ran. If it does not, add `owner` naming the box that will close it.
3. Name its constants and defaults keys in `constants` and `defaults`.
4. Run `cargo test --locked --offline -p north-dispatch loops`, then `north loops check`.
