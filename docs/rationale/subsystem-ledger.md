# Subsystem ledger: what each north subsystem costs to repair against the decisions it feeds

North's breadth has to show it is necessary (north#797). This table counts, for each crate and harness unit family over the last 14 days, the repair it needed and the decisions it fed, from git history and the worker ledger only, with no agent text.

Rebuild it with `bin/subsystem-ledger [DAYS]` from a north checkout on a host that holds the ledger replica (`~/.local/state/north/ledger.db` and `~/.local/state/threads/snapshot.db`; `LEDGER_DB` and `THREADS_DB` override them). The table below was built on 2026-11-07 against origin/main 61f42e6bf8, window 14 days.

## Columns

- **lines**: lines of tracked files under the unit now. A path that moved between crates in the crate split (`docs/rationale/crate-split.md`) counts for the crate that holds its module now, so history before the move is not charged to `north-core` or `north-ops`.
- **commits**, **lines churned**: non-merge commits in the window that touched the unit, and lines added plus deleted in them.
- **fix commits**: those commits that carry an `Incident-test:` trailer, start with `Fix` or `Revert`, or state the dated incident they repair (`on 2026-1M-DD`); 541 of the window's 2,350 commits. The third rule is a pattern over commit text, which is a hint, not a record: it is the widest of the three and the harness-repair commits of 2026-11-07 are what it exists to catch.
- **harness-loss rows**: mechanical records of the harness failing, charged to the unit that owns the boundary that wrote them: `incidents` rows other than `lander-refused` and `lander-conflict` (the lander turning back a worker's own lane is the worker's loss) to north-landing, except `nix-*` to harness, `false-refusal` to north-diff-gates and `flaky-test` to north-landing-gates; `boundary_causes` rows whose cause is a harness class, by the writing boundary (lander, dispatch, capacity, else fleet); and lander candidate checks that timed out.
- **decisions**: rows the unit wrote that decided an outcome: north-landing's `conflict` and `failed` verdicts, harness hook refusals and rewrites, tier predictions and tier choices (north-dispatch, north-threads), answered gates (north-fleet).
- **repair per decision**: (fix commits + harness-loss rows) / (decisions + 1). The table is sorted by it, most repair per decision first; the last column is the ranking the bottom quarter is cut from.

## Limits

- A 0 in **decisions** means the unit writes no decision row the ledger holds, not that its decisions are worthless: a gate, a library or a placement helper decides inside other subsystems' rows. This ranks units with recorded decisions above units without, which is the point of the test (a subsystem that cannot show a decision it changed has not shown it earns its place) and also its bias: foundation units (`north-base`, `tests`, `north-bin`, `bin-lib`) rank last by construction.
- **fix commits** counts a commit once per unit, so a cross-unit repair counts for every unit it touched.
- Decisions "that changed an outcome" is not measured: a tier prediction or a hook rewrite is counted whether or not the choice differed from the default. north#797's second box and its filed issues name the units where that measurement is missing.

## Table

| subsystem | lines | commits | fix commits | lines churned | harness-loss rows | decisions | repair per decision |
|---|---:|---:|---:|---:|---:|---:|---:|
| north-bin | 12188 | 837 | 134 | 157399 | 0 | 0 | 134.00 |
| tests | 8710 | 603 | 128 | 69164 | 0 | 0 | 128.00 |
| north-runtime | 15509 | 179 | 74 | 16041 | 0 | 0 | 74.00 |
| north-ops | 15911 | 279 | 64 | 50010 | 0 | 0 | 64.00 |
| north-base | 12658 | 168 | 58 | 11042 | 0 | 0 | 58.00 |
| bin-lib | 1448 | 338 | 50 | 36528 | 0 | 0 | 50.00 |
| north-capacity-host | 20428 | 123 | 38 | 13504 | 0 | 0 | 38.00 |
| north-providers | 8290 | 120 | 36 | 8416 | 0 | 0 | 36.00 |
| north-capacity | 6576 | 98 | 33 | 8749 | 1 | 0 | 34.00 |
| north-guard | 21227 | 137 | 26 | 24387 | 0 | 0 | 26.00 |
| north-landing-gates | 6591 | 87 | 19 | 6329 | 2 | 0 | 21.00 |
| north-core | 15862 | 129 | 19 | 15747 | 0 | 0 | 19.00 |
| north-pace | 10332 | 78 | 15 | 21228 | 0 | 0 | 15.00 |
| agent-machinery | 9325 | 40 | 13 | 1000 | 0 | 0 | 13.00 |
| north-lander-doctor | 2274 | 20 | 10 | 2524 | 0 | 0 | 10.00 |
| north-health | 1561 | 24 | 9 | 2237 | 0 | 0 | 9.00 |
| north-lanes | 5391 | 39 | 9 | 4016 | 0 | 0 | 9.00 |
| north-tui | 3163 | 28 | 7 | 2790 | 0 | 0 | 7.00 |
| north-code-health | 4441 | 23 | 5 | 6358 | 0 | 0 | 5.00 |
| north-coverage | 1472 | 13 | 5 | 2488 | 0 | 0 | 5.00 |
| north-strategy-types | 722 | 20 | 5 | 770 | 0 | 0 | 5.00 |
| north-diff-gates | 4102 | 16 | 3 | 2935 | 2 | 0 | 5.00 |
| north-goals | 951 | 13 | 4 | 1115 | 0 | 0 | 4.00 |
| north-session-host | 1071 | 5 | 4 | 450 | 0 | 0 | 4.00 |
| north-frontier | 1633 | 9 | 3 | 1701 | 0 | 0 | 3.00 |
| north-graph-ingest | 1403 | 7 | 2 | 1607 | 0 | 0 | 2.00 |
| north-replay | 1078 | 13 | 2 | 1202 | 0 | 0 | 2.00 |
| north-safe-push | 4470 | 23 | 2 | 5065 | 0 | 0 | 2.00 |
| north-check-environment | 225 | 1 | 1 | 225 | 0 | 0 | 1.00 |
| north-climb | 656 | 7 | 1 | 826 | 0 | 0 | 1.00 |
| north-convo | 7340 | 2 | 1 | 445 | 0 | 0 | 1.00 |
| north-github-mirror | 773 | 8 | 1 | 1041 | 0 | 0 | 1.00 |
| north-graph-ground | 930 | 7 | 1 | 1034 | 0 | 0 | 1.00 |
| north-implicit-state | 952 | 4 | 1 | 1006 | 0 | 0 | 1.00 |
| north-threads | 12215 | 129 | 37 | 14900 | 0 | 43 | 0.84 |
| north-fleet | 14092 | 183 | 55 | 25012 | 52 | 149 | 0.71 |
| north-landing | 20349 | 384 | 119 | 59747 | 104 | 1013 | 0.22 |
| north-dispatch | 13541 | 125 | 31 | 28642 | 8 | 292 | 0.13 |
| harness | 11885 | 548 | 128 | 56931 | 65 | 25663 | 0.01 |
| north-claude-stream | 403 | 1 | 0 | 17 | 0 | 0 | 0.00 |
| north-flow | 8680 | 3 | 0 | 60 | 0 | 0 | 0.00 |
| north-graph | 3258 | 6 | 0 | 3364 | 0 | 0 | 0.00 |
| north-land-scans | 3649 | 2 | 0 | 269 | 0 | 0 | 0.00 |
| north-mutants | 1182 | 1 | 0 | 27 | 0 | 0 | 0.00 |
