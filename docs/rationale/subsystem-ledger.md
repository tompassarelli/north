# Subsystem ledger: what each north subsystem costs to repair against the decisions it feeds

North's breadth has to show it is necessary (north#797). This table counts, for each crate and harness unit family over the last 14 days, the repair it needed and the decisions it fed, from git history and the worker ledger only, with no agent text.

Rebuild it with `bin/subsystem-ledger [DAYS]` from a north checkout on a host that holds the ledger replica (`~/.local/state/north/ledger.db` and `~/.local/state/threads/snapshot.db`; `LEDGER_DB` and `THREADS_DB` override them). The table below was built on 2026-11-07 against origin/main 61f42e6bf8, window 14 days.

## Columns

- **lines**: lines of tracked files under the unit now. A path that moved between crates in the crate split (`docs/rationale/crate-split.md`) counts for the crate that holds its module now, so history before the move is not charged to `north-core` or `north-ops`.
- **commits**, **lines churned**: non-merge commits in the window that touched the unit, and lines added plus deleted in them.
- **fix commits**: those commits that carry an `Incident-test:` trailer, start with `Fix` or `Revert`, or state the dated incident they repair (`on 2026-1M-DD`); 541 of the window's 2,350 commits. The third rule is a pattern over commit text, which is a hint, not a record: it is the widest of the three and the harness-repair commits of 2026-11-07 are what it exists to catch.
- **harness-loss rows**: mechanical records of the harness failing, charged to the unit that owns the boundary that wrote them: `incidents` rows other than `lander-refused` and `lander-conflict` (the lander turning back a worker's own lane is the worker's loss) to north-landing, except `nix-*` to harness, `false-refusal` to north-diff-gates and `flaky-test` to north-landing-gates; `boundary_causes` rows whose cause is a harness class, by the writing boundary (lander, dispatch, capacity, else fleet); and lander candidate checks that timed out.
- **decisions**: rows the unit wrote that decided an outcome: north-landing's `conflict` and `failed` verdicts, north-guard hook refusals and rewrites, tier predictions and tier choices (north-dispatch, north-threads), answered gates (north-fleet).
- **repair per decision**: (fix commits + harness-loss rows) / (decisions + 1). The table is sorted by it, most repair per decision first; the last column is the ranking the bottom quarter is cut from.

## Limits

- A 0 in **decisions** means the unit writes no decision row the ledger holds, not that its decisions are worthless: a gate, a library or a placement helper decides inside other subsystems' rows. This ranks units with recorded decisions above units without, which is the point of the test (a subsystem that cannot show a decision it changed has not shown it earns its place) and also its bias: foundation units (`north-base`, `tests`, `north-bin`, `bin-lib`) rank last by construction.
- **fix commits** counts a commit once per unit, so a cross-unit repair counts for every unit it touched.
- Decisions "that changed an outcome" is not measured: a tier prediction or a hook rewrite is counted whether or not the choice differed from the default. north#797's second box and its filed issues name the units where that measurement is missing.

## Table

| subsystem | lines | commits | fix commits | lines churned | harness-loss rows | decisions | repair per decision |
|---|---:|---:|---:|---:|---:|---:|---:|
| harness | 11885 | 548 | 128 | 56931 | 65 | 0 | 193.00 |
| north-bin | 12188 | 837 | 134 | 157399 | 0 | 0 | 134.00 |
| tests | 8710 | 603 | 128 | 69164 | 0 | 0 | 128.00 |
| north-runtime | 15509 | 179 | 74 | 16041 | 0 | 0 | 74.00 |
| north-ops | 15911 | 279 | 64 | 50010 | 0 | 0 | 64.00 |
| north-base | 12658 | 168 | 58 | 11042 | 0 | 0 | 58.00 |
| bin-lib | 1492 | 339 | 50 | 36572 | 0 | 0 | 50.00 |
| north-capacity-host | 20428 | 123 | 38 | 13504 | 0 | 0 | 38.00 |
| north-providers | 8290 | 120 | 36 | 8416 | 0 | 0 | 36.00 |
| north-capacity | 6576 | 98 | 33 | 8749 | 1 | 0 | 34.00 |
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
| north-fleet | 14092 | 183 | 55 | 25012 | 60 | 149 | 0.77 |
| north-landing | 20349 | 384 | 119 | 59747 | 104 | 1013 | 0.22 |
| north-dispatch | 13541 | 125 | 31 | 28642 | 8 | 292 | 0.13 |
| north-guard | 21227 | 137 | 26 | 24387 | 0 | 25724 | 0.00 |
| north-claude-stream | 403 | 1 | 0 | 17 | 0 | 0 | 0.00 |
| north-flow | 8680 | 3 | 0 | 60 | 0 | 0 | 0.00 |
| north-graph | 3258 | 6 | 0 | 3364 | 0 | 0 | 0.00 |
| north-land-scans | 3649 | 2 | 0 | 269 | 0 | 0 | 0.00 |
| north-mutants | 1182 | 1 | 0 | 27 | 0 | 0 | 0.00 |

## Bottom quarter: keep, merge or delete

The bottom quarter is the first 11 of the 44 rows, most repair per decision first. Four of them are foundations with no decision rows by construction (north-bin, tests, north-base, bin-lib); they get a keep line with the reason, and the others are judged on their own records.

- **harness** (193; 128 fix commits, 65 harness-loss rows): keep the hooks, defaults and policy tables; the 65 loss rows are `nix-*` incidents, 12 of them classes whose guard landed and none recurred (`north ledger incident report`). The repair sits in skills: 81 of the 128 fix commits touch `harness/skills`, and the skills edited most per load are `workers` (97 commits in 14 days, 25 loads in the 5,600 runs of the window), `machine-capacity` (98, 121) and `agent-policy` (57, 11), against `rust-development` (0 commits, 591 loads). Merge: a skill edited more often than it is loaded folds its per-incident rules into the gate or hook that enforces them, or loses the lines; filed as north#799.
- **north-bin** (`src/`, 134): keep. The root package wires every command; its repair follows the commands added (837 commits, the most of any unit), and nothing below it can host a command. No merge target exists.
- **tests** (128): keep. The integration tests pin external contracts and the landing's own guarantees; their repair is the other side of every fix commit. The test-budget rule (`agent-machinery/skills/verification`) already refuses a lane that adds more tests than it deletes.
- **north-runtime** (74): keep `land_offload`, `build_offload`, `offload_cache`, `deploy` and `update` (36, 15 and 4 of the 74 fix commits sit in land_offload, build_offload and offload_cache; they place every landing and build). Delete candidate `model_watch` (2,443 lines), `laptop_ntfy` (430), `outside_watch` (555) and `usage_web_read` (177): the replica holds 2 watch digests, 1 model event, 0 alerts sent and 7 candidates, so none feeds a decision it records. Removing the model watch changes what north offers, so it goes to Tom as one decision (below).
- **north-ops** (64): keep. 30 modules, the largest `private_desktop` (1,661 lines), `gh_broker` (1,656) and `defaults_measure` (1,286); `gh_broker` holds 5 of the 64 fix commits, so no single module is the cost. `private_desktop`, `warcraft_close` and `wc3_login_field` serve Warcraft playtests, which north runs for Wisp; moving them out of north would change its scope, so they stay until Tom says otherwise.
- **north-base** (58): keep. Every crate depends on it; 38 of its 58 fix commits are in `platform` (the defaults and host-fact readers), which is where a tuned number's reader lives.
- **bin-lib** (50): keep for now; `docs/rationale/non-rust-inventory.md` already tracks each script's port to Rust, and a port deletes its row here.
- **north-capacity-host** (38): keep. It writes the admission queue that attribution reads for capacity losses (`queued_behind_capacity`), and 20 of its 38 fix commits sit in `host`. Merging it into north-capacity would undo the measured build-time split of `docs/rationale/crate-split.md`.
- **north-providers** (36): keep. It is the provider seam Tom's orders keep, and 21 of its 36 fix commits are in the Claude adapter; Codex gets nothing new.
- **north-capacity** (34): keep, with one measurement missing. Admission decides every build and lease, yet the replica's `capacity_runs` holds no row after 2026-10-10, so its decisions show as 0 here. North#798 records each admission as a `capacity_admissions` row in the threads ledger and `bin/subsystem-ledger` counts them, so the next table weighs it.
- **north-landing-gates** (21): keep. Its 87 commits are the gates the shadow mode of north#729 loosened, and the table cannot yet say which refused a real fault; north#729's 24 h shadow log is the measurement that decides which gates earn refusal back.

## Decisions for Tom

One scope decision: delete the model watch (`model_watch` and `laptop_ntfy` in north-runtime, about 2,900 lines and the `watch_*` tables) or keep it.

## Filed

Merges and deletions inside north scope are filed: north#798 (record capacity admission decisions) and north#799 (fold or delete skills edited far more than loaded).
