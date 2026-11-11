# Subsystem ledger: what each north subsystem costs to repair against the decisions it feeds

North's breadth has to show it is necessary (north#797). This table counts, for each crate and harness unit family over the last 14 days, the repair it needed and the decisions it fed, from git history and the worker ledger only, with no agent text.

Rebuild it with `bin/subsystem-ledger [DAYS]` from a north checkout on a host that holds the ledger replica (`~/.local/state/north/ledger.db` and `~/.local/state/threads/snapshot.db`; `LEDGER_DB` and `THREADS_DB` override them). The table below was built on 2026-11-07 against origin/main cab65aaf1, window 14 days. `FIX_RULE=text bin/subsystem-ledger` rebuilds the same table with the earlier commit-text rule for the comparison below.

## Columns

- **lines**: lines of tracked files under the unit now. A path that moved between crates in the crate split (`docs/rationale/crate-split.md`) counts for the crate that holds its module now, so history before the move is not charged to `north-core` or `north-ops`.
- **commits**, **lines churned**: non-merge commits in the window that touched the unit, and lines added plus deleted in them.
- **fix commits**: commits of the window that repair, by two records and no commit text (`bin/subsystem-fix-commits`): a commit that changes or deletes a line another commit landed in the 14 days before it (`git blame --since`, 2,139 of the window's 2,370 commits), or a commit the lander landed after an earlier verdict `failed` for the same lane (the `landings` table, 373). A revert of a commit from the window is the first kind; 45 of the window's 47 `Revert` commits are counted by it, so no separate revert test remains (an exact patch-id inverse matched none of them, because later edits change the hunks). The rule is the one north#797 states, and on a repo this active it counts 91% of commits: the column now ranks units by how much of their change lands on fresh lines, which is the repair signal left once no commit wording is read.
- **harness-loss rows**: mechanical records of the harness failing, charged to the unit that owns the boundary that wrote them: `incidents` rows other than `lander-refused` and `lander-conflict` (the lander turning back a worker's own lane is the worker's loss) to north-landing, except `nix-*` to harness, `false-refusal` to north-diff-gates and `flaky-test` to north-landing-gates; `boundary_causes` rows whose cause is a harness class, by the writing boundary (lander, dispatch, capacity, provider, launch, else fleet); and lander candidate checks that timed out.
- **decisions**: rows the unit wrote that decided an outcome: north-landing's `conflict` and `failed` verdicts, north-guard hook refusals and rewrites, tier predictions and tier choices (north-dispatch, north-threads), answered gates (north-fleet).
- **repair per decision**: (fix commits + harness-loss rows) / (decisions + 1). The table is sorted by it, most repair per decision first; the last column is the ranking the bottom quarter is cut from.

## Limits

- A 0 in **decisions** means the unit writes no decision row the ledger holds, not that its decisions are worthless: a gate, a library or a placement helper decides inside other subsystems' rows. This ranks units with recorded decisions above units without, which is the point of the test (a subsystem that cannot show a decision it changed has not shown it earns its place) and also its bias: foundation units (`north-base`, `tests`, `north-bin`, `bin-lib`) rank last by construction.
- **fix commits** counts a commit once per unit, so a cross-unit repair counts for every unit it touched.
- Decisions "that changed an outcome" is not measured: a tier prediction or a hook rewrite is counted whether or not the choice differed from the default. north#797's second box and its filed issues name the units where that measurement is missing.

## Table

| subsystem | lines | commits | fix commits | lines churned | harness-loss rows | decisions | repair per decision |
|---|---:|---:|---:|---:|---:|---:|---:|
| north-bin | 12694 | 832 | 795 | 148406 | 0 | 0 | 795.00 |
| harness | 11901 | 536 | 495 | 56260 | 65 | 0 | 560.00 |
| tests | 8800 | 591 | 559 | 68367 | 0 | 0 | 559.00 |
| bin-lib | 1520 | 320 | 296 | 34808 | 0 | 0 | 296.00 |
| north-ops | 15970 | 295 | 282 | 50187 | 0 | 0 | 282.00 |
| north-runtime | 15842 | 198 | 191 | 16582 | 0 | 0 | 191.00 |
| north-core | 15918 | 187 | 185 | 21783 | 0 | 0 | 185.00 |
| north-base | 12725 | 177 | 173 | 11193 | 0 | 0 | 173.00 |
| north-fleet-supervisor | 5701 | 135 | 133 | 9308 | 0 | 0 | 133.00 |
| north-capacity-host | 20636 | 132 | 128 | 13878 | 0 | 0 | 128.00 |
| north-providers | 8502 | 129 | 127 | 8802 | 0 | 0 | 127.00 |
| north-landing-gates | 6754 | 94 | 93 | 6780 | 2 | 0 | 95.00 |
| north-pace | 10422 | 82 | 77 | 21366 | 0 | 0 | 77.00 |
| north-lanes | 5585 | 43 | 42 | 4426 | 0 | 0 | 42.00 |
| agent-machinery | 9325 | 38 | 32 | 984 | 0 | 0 | 32.00 |
| north-tui | 3159 | 30 | 30 | 2814 | 0 | 0 | 30.00 |
| north-safe-push | 4471 | 27 | 27 | 5110 | 0 | 0 | 27.00 |
| north-code-health | 4468 | 24 | 24 | 6515 | 0 | 0 | 24.00 |
| north-health | 1561 | 25 | 24 | 2239 | 0 | 0 | 24.00 |
| north-strategy-types | 724 | 21 | 21 | 772 | 0 | 0 | 21.00 |
| north-lander-doctor | 2284 | 21 | 20 | 2556 | 0 | 0 | 20.00 |
| north-diff-gates | 4102 | 16 | 16 | 2935 | 2 | 0 | 18.00 |
| north-replay | 1090 | 15 | 15 | 1250 | 0 | 0 | 15.00 |
| north-coverage | 1472 | 14 | 14 | 2562 | 0 | 0 | 14.00 |
| north-frontier | 1867 | 13 | 13 | 1995 | 0 | 0 | 13.00 |
| north-concerns | 951 | 13 | 13 | 1115 | 0 | 0 | 13.00 |
| north-flow | 8719 | 10 | 10 | 139 | 0 | 0 | 10.00 |
| north-climb | 656 | 9 | 9 | 836 | 0 | 0 | 9.00 |
| north-github-mirror | 773 | 8 | 8 | 1041 | 0 | 0 | 8.00 |
| north-graph-ground | 930 | 8 | 8 | 1038 | 0 | 0 | 8.00 |
| north-graph-ingest | 1403 | 7 | 7 | 1607 | 0 | 0 | 7.00 |
| north-land-scans | 3932 | 7 | 7 | 592 | 0 | 0 | 7.00 |
| north-graph | 3258 | 6 | 6 | 3364 | 0 | 0 | 6.00 |
| north-session-host | 1134 | 6 | 6 | 513 | 0 | 0 | 6.00 |
| north-fleet | 6502 | 134 | 133 | 10494 | 273 | 75 | 5.34 |
| north-fleet-store | 2348 | 5 | 5 | 290 | 0 | 0 | 5.00 |
| north-convo | 7340 | 4 | 4 | 459 | 0 | 0 | 4.00 |
| north-threads | 12624 | 139 | 139 | 15429 | 0 | 43 | 3.16 |
| north-check-environment | 258 | 2 | 2 | 260 | 0 | 0 | 2.00 |
| north-implicit-state | 952 | 2 | 2 | 954 | 0 | 0 | 2.00 |
| north-mutants | 1182 | 2 | 2 | 37 | 0 | 0 | 2.00 |
| north-resume | 130 | 2 | 2 | 132 | 0 | 0 | 2.00 |
| north-dispatch | 13627 | 138 | 136 | 29120 | 325 | 341 | 1.35 |
| north-claude-stream | 403 | 1 | 1 | 17 | 0 | 0 | 1.00 |
| north-landing | 20632 | 397 | 388 | 60122 | 107 | 1038 | 0.48 |
| north-capacity | 6579 | 107 | 104 | 8854 | 3 | 2554 | 0.04 |
| north-guard | 21276 | 144 | 142 | 24496 | 0 | 26524 | 0.01 |

## Mechanical rule against the commit-text rule

The earlier table counted a commit as a fix when its message carried an `Incident-test:` trailer, started with `Fix` or `Revert`, or named a dated incident (541 of 2,350 commits); the north#791 rule says no verdict reads such text. Both rules rebuilt on the same origin/main (cab65aaf1, 47 units, bottom quarter the first 11):

- The bottom quarter has the same 11 members under both rules: north-bin, harness, tests, bin-lib, north-ops, north-runtime, north-core, north-base, north-fleet-supervisor, north-capacity-host, north-providers. No unit enters or leaves it.
- Within it, bin-lib moves from 8th to 4th, north-core from 11th to 7th, north-runtime from 4th to 6th, north-base from 6th to 8th, north-fleet-supervisor from 7th to 9th, harness and north-bin swap first and second.
- Outside it, units that move three or more places are north-safe-push (29th to 17th), north-fleet (24th to 35th), north-session-host (22nd to 34th), north-graph (45th to 33rd), north-github-mirror (39th to 29th), north-lander-doctor (15th to 21st), north-fleet-store (26th to 36th), north-check-environment (30th to 39th), north-dispatch (37th to 43rd), north-guard (43rd to 47th), north-landing (41st to 45th), north-capacity (42nd to 46th), north-resume (36th to 42nd), north-mutants (47th to 41st), north-implicit-state (46th to 40th), north-replay (28th to 23rd) and the rest by fewer than three. 42 of 47 units change rank.

## Bottom quarter: keep, merge or delete

The bottom quarter is the first 11 of the 47 rows, most repair per decision first. Four of them are foundations with no decision rows by construction (north-bin, tests, north-base, bin-lib); they get a keep line with the reason, and the others are judged on their own records. North-capacity left the quarter because its admissions are now counted (north#798).

- **north-bin** (`src/`): keep. The root package wires every command; its repair follows the commands added (the most commits of any unit), and nothing below it can host a command. No merge target exists.
- **harness**: keep the hooks, defaults and policy tables; the 65 loss rows are `nix-*` incidents, 12 of them classes whose guard landed and none recurred (`north ledger incident report`). The repair sits in skills: the skills edited most per load are `workers` (97 commits in 14 days, 25 loads in the 5,600 runs of the window), `machine-capacity` (98, 121) and `agent-policy` (57, 11), against `rust-development` (0 commits, 591 loads). Merge: a skill edited more often than it is loaded folds its per-incident rules into the gate or hook that enforces them, or loses the lines; filed as north#799.
- **tests**: keep. The integration tests pin external contracts and the landing's own guarantees; their repair is the other side of every fix commit. The test-budget rule (`harness/skills/verification`) already refuses a lane that adds more tests than it deletes.
- **bin-lib**: keep for now; `docs/rationale/non-rust-inventory.md` already tracks each script's port to Rust, and a port deletes its row here.
- **north-ops**: keep. 30 modules, the largest `private_desktop` (1,661 lines), `gh_broker` (1,656) and `defaults_measure` (1,286); no single module is the cost. `private_desktop`, `warcraft_close` and `wc3_login_field` serve Warcraft playtests, which north runs for Wisp; moving them out of north would change its scope, so they stay until Tom says otherwise.
- **north-runtime**: keep `land_offload`, `build_offload`, `offload_cache`, `deploy` and `update` (they place every landing and build). Delete candidate `model_watch` (2,443 lines), `laptop_ntfy` (430), `outside_watch` (555) and `usage_web_read` (177): the replica holds 2 watch digests, 1 model event, 0 alerts sent and 7 candidates, so none feeds a decision it records. Removing the model watch changes what north offers, so its deletion is the scope question below.
- **north-core**: keep. Its repair sits in `agent_config_check` (30 commits in 14 days, 2,051 lines), `grounding` (29, 5,198) and `catalog` (25, 3,284): the config check is the gate that refuses a broken agent config before it reaches a host, `grounding` feeds the routing reads, and the catalog is the command registry. The lib file itself shows 21 commits because every new module registers there; no other unit can host them.
- **north-base**: keep. Every crate depends on it, and most of its repair is in `platform` (the defaults and host-fact readers), which is where a tuned number's reader lives.
- **north-fleet-supervisor**: keep. The crate holds the reaper, resume, handover and limit modules that decide whether a session lives or ends; its 135 commits are mostly the history of those modules before they moved into the crate (12 touch its own path), so most of its rank is inherited, and it is tiny at 5,701 lines for what it decides. Gate answers count under north-fleet, so the decisions it makes are not in this column; A recorded row per reaper verdict would let the next table weigh it, as north#798 does for admission.
- **north-capacity-host**: keep. It writes the admission queue that attribution reads for capacity losses (`queued_behind_capacity`). Merging it into north-capacity would undo the measured build-time split of `docs/rationale/crate-split.md`.
- **north-providers**: keep. It is the provider seam Tom's orders keep; Codex gets nothing new from it.

North-landing-gates is 12th, one place out of the quarter, and stays under the shadow-mode measurement of north#729 that decides which gates earn refusal back.

## Scope question: decided

The model watch (`model_watch` and `laptop_ntfy` in north-runtime, about 2,900 lines and the `watch_*` tables) was deleted, decided 2026-11-08 and landed under north#824; git keeps it.

## Filed

Merges and deletions inside north scope are filed: north#798 (record capacity admission decisions) and north#799 (fold or delete skills edited far more than loaded).
