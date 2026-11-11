# Command usage, 30 days to 2026-11-10

Measured for north#970 cut 13 (docs/rationale/leaner-north.md): one 30-day use count for every command line in
`src/cli/reference.txt` (308 commands at commit 14871e01b3), read before any command is proposed for retirement.

## How the counts were taken

- Window: transcript lines stamped within 30 days of the read at 2026-11-10 04:38 (+07), on the laptop
  (whiterabbit), gex45 and nexus.
- Transcripts ("convo" columns): every `Bash` tool call in every `*.jsonl` under `~/.claude/projects` and
  `~/.local/state/north/profiles/*/projects` (subagent files included) whose `command` text holds `north` or
  `bin/north` followed by lowercase words. Each such invocation counts once for the longest command in
  `reference.txt` that is a word-prefix of it, so `north fleet send x` counts for `fleet` unless a longer line such as
  `fleet laptop` matches. A command text is searched line by line, so one call that chains three `north` commands counts three.
- Unit Exec columns: every `Exec*=` line from `systemctl cat` of all system and user unit files on the host, plus each
  `north` call in a script file those lines name, one level deep. A unit line counts once as a standing caller; a
  timer that fires 288 times a day still adds 1, so these columns show whether a caller exists, not how often it fires.
- Hook column: `north` calls in the `command` fields of every Claude Code `settings.json` on the laptop, gex45 and nexus.
- Source refs: files outside docs, tests and the two reference texts that contain the literal `north COMMAND`; it
  shows commands that other north code or a script may still call although no transcript or unit does.
- Uses = the three convo columns + the three unit columns + hook refs. Sorted by uses, then name. A row reads
  `ZERO` and a verdict when uses is 0.
- Verdicts follow the rule Tom gave on 2026-11-10 that zero use alone never retires a command: KEEP means the code runs
  in process, answers a protocol or guards a guarantee, so the missing call is a blind spot of the count; TRY means
  the command would give something the project wants and is worth using before any judgement; RETIRE means it
  duplicates a used command, is a finished one-time migration, or serves a vendor Tom does not run. A RETIRE verdict
  is a candidate that Tom still approves; it removes only the command line named, not its crate.

## What the counts cannot see

- Bare group commands (`fleet`, `threads`, `gate`, `graph`, `ledger`, `land`, `lane`, `work`, `config`) also collect
  every verb that has no line of its own in `reference.txt` (for example `fleet launch` and `fleet send` count under `fleet`).
- A call made by north itself with an argv array (not the text `north X`), by git (`git-credential store` and
  `erase` are credential-helper verbs git calls), or by a Claude Code hook script outside the repository, appears in
  no column; check the source-refs column and the crate's callers before retiring a ZERO row.
- Transcripts older than 30 days or deleted by retention are absent; a host without a transcript tree shows 0.
- The crate column names the crate whose code the dispatcher in `src/cli/tools.rs`, `src/cli/tools/subverbs.rs` and
  `src/main.rs` calls for the command; `north (root)` is the binary's own `src/` (15,603 Rust lines, 13,900 non-test).

## Zero-use commands (46)

- `agent-contain sweep` (north-runtime; source refs 2): KEEP. Keeps agent children out of Tom's slices under load; north-runtime runs the same code in process when a launch is confined, so the CLI line is a manual entry that costs one function.
- `capacity host release` (north-capacity-host; source refs 1): TRY. The machine-capacity skill tells agents to reserve and release leases; zero calls in 30 days means they skip leases and admit-job/run carry the load. Gives bounded builds with an owner; costs nothing beyond keeping the verb. Decide whether the skill or the verb is wrong, not whether to delete.
- `capacity host reserve` (north-capacity-host; source refs 1): TRY. Same lease pair as release: the skill names it, no session called it. Worth wiring into the launch brief before judging it dead.
- `capacity mode tick` (north-capacity-host; source refs 0): KEEP. The mode_tick module is called in process by laptop-auto (55 uses) and by build and land offload; this line only re-runs it by hand.
- `check experiments judge` (north-flow; source refs 3): TRY. Closes the experiment registry by judging due entries; no unit or session ran it. Gives the review-rule and footgun cards an expiry; costs one timer line to wire.
- `check harness policy-gate` (north-core; source refs 1): KEEP. Called by agents sync (64 uses) as the skill-budget and startup-token gate; the argv call is invisible to transcripts.
- `check nix` (north (root); source refs 2): KEEP. Guards the offline, rooted dev-shell guarantee named in harness/nix/cards.toml; run it when flake.lock changes. Cost is one crate function.
- `check provenance audit` (north-landing; source refs 1): RETIRE. Audits grounding events; no caller in 30 days, and check provenance debt (6 uses) is the provenance view in use. Retire unless Tom wants a grounding audit.
- `check provenance check` (north-landing; source refs 1): KEEP. The land gate runs the same code in process; the CLI form is the way to rehearse a gate before pushing.
- `check provenance inventory` (north-landing; source refs 2): RETIRE. A one-off census of records nothing names; no caller in 30 days, and check provenance debt (6 uses) lists the same gaps per entry.
- `config agents routines list` (north (root); source refs 0): TRY. Lists the operator routines that cloud scheduling relies on; nothing calls it. Keep until routines are used or Tom retires scheduled routines.
- `config agents routines pointer` (north (root); source refs 0): TRY. Same routines group as list.
- `config agents routines show` (north (root); source refs 0): TRY. Same routines group as list.
- `config agents usage` (north (root); source refs 0): RETIRE. Same handler as fleet usage (194 uses; both call agents::usage in tools.rs) with the same flags, so the second spelling is pure duplication.
- `desktop private capture` (north-ops; source refs 1): TRY. Screenshot of a private desktop is how game-client and Warcraft playtests are checked (start has 21 uses); sessions screenshot through other means today. Gives visual proof; costs nothing to keep with start.
- `desktop private clipboard` (north-ops; source refs 1): TRY. Same private-desktop group as capture; needed to paste into a client.
- `desktop private control` (north-ops; source refs 1): TRY. Same group; VNC input is the only way to drive a client in a private desktop.
- `desktop private type` (north-ops; source refs 1): TRY. Same group; types text into a client.
- `fleet laptop close` (north-dispatch; source refs 0): KEEP. Pair of open (6 uses) and drain; closing the laptop to placement is the safe default when Tom plays.
- `fleet orders list` (north (root); source refs 1): KEEP. Reads Tom's standing orders, which every launch renders; add has 1 use. A list verb is the only way to audit them.
- `fleet pace place` (north-dispatch; source refs 0): KEEP. Launch placement runs in process during fleet launch; this line shows the decision without launching, which the placement standing order wants.
- `git-credential erase` (north-core; source refs 0): KEEP. Git calls it as a credential helper verb; it must exist and answer nothing.
- `git-credential store` (north-core; source refs 0): KEEP. Same credential helper contract.
- `graph ground distill capture` (north-graph-ground; source refs 1): TRY. Turns Tom's corrections into grounding records so he does not repeat himself; zero runs shows the learning loop is not running. Worth starting before judging.
- `graph ground distill run` (north-graph-ground; source refs 1): TRY. Same loop; no timer runs it, so records stop at the last manual run.
- `graph ground distill status` (north-graph-ground; source refs 1): TRY. Same loop.
- `graph ground hook` (north-graph-ground; source refs 0): RETIRE. Duplicates north hook prompt-grounding (9 hook refs), which is what the prompt hook runs.
- `graph ground import` (north-graph-ground; source refs 0): RETIRE. One-time move of the retired grounding/index.db into the graph; done, and no users remain to migrate.
- `graph ground refresh` (north-graph-ground; source refs 0): TRY. Refreshes grounding docs from their sources; without it the grounding index ages. Wire to a timer or retire the whole grounding group.
- `graph retract` (north-graph-ingest; source refs 0): KEEP. Withdraws a wrong fact; graph tell has 14 uses and a tell with no retract cannot be corrected.
- `land actions` (north-landing; source refs 2): KEEP. Run by the land check runner in process (crates/north-land-checks execute.rs); invisible to transcripts.
- `land fails-first` (north-landing; source refs 1): KEEP. Enforces the test-admission rule that a new bug test fails on base; the landing gate calls the same code.
- `land train record` (north-lander; source refs 0): TRY. Records a lander verdict in the threads ledger; check has 1 use. Gives landing history for tempo; costs little.
- `land train red` (north-lander; source refs 0): TRY. Records a red-main interval in the threads ledger; no caller in 30 days. Decide with land train record.
- `ledger incident import` (north-providers; source refs 1): RETIRE. Bulk import of incident rows from jsonl, a one-time migration; record (7) and report (26) are the live verbs.
- `ops strategy log check` (north (root); source refs 1): TRY. Parses every decision-log line back into its record type; the check that placement logs stay parseable. Costs one pass over the log.
- `ops strategy refusals` (north (root); source refs 2): TRY. The oracle of the launch-refusals-per-hour goal; nothing reads it, so the goal is unmeasured. Use it or drop the goal.
- `outside-watch status` (north-runtime; source refs 0): KEEP. Shows the last run of the outside watch timer; a read-only status line.
- `provider-capacity status` (north-capacity-host; source refs 0): TRY. Reads provider capacity for accounts a, b and c; pacing admission should read it, and no caller does. Wire it before judging.
- `runtime agy` (north-runtime; source refs 1): KEEP. Installs and updates the Agy vendor runtime under ~/.local/lib/agy that bin/agy launches; no session ran it in 30 days, but Tom runs agy by hand and the usage reader and privacy judge call it, so north#1045 restored the installer and its twice-daily step (2026-11-11).
- `test coverage miss` (north-coverage; source refs 3): TRY. Names after-push failures the test selection missed and refreshes the map; the feedback that keeps selection honest. Costs a call per red push.
- `test frontier oracle` (north-frontier; source refs 0): RETIRE. Part of the frontier scoreboard that cut 7 may retire; grade and run have 1 and 4 uses; oracle has none. Retire with the group if cut 7 passes.
- `test mutant record` (north-mutants; source refs 1): TRY. Records a scratch-copy mutant that kills a test; the test-admission rule asks for a recorded mutant, and sweep (4) is the only user.
- `work need` (north (root); source refs 1): TRY. Records that an item waits on another; GitHub blocked-by does the same on the issue, so use one. Gives dispatch the dependency graph; costs a second place to update.
- `work review` (north (root); source refs 1): TRY. Splits a design review into fact, scenario and architecture items and routes them by tier; planned for design reviews, none run in 30 days.
- `work unneed` (north (root); source refs 1): TRY. Pairs with work need.

## Crates holding the zero-use commands

Lines are Rust totals of `git ls-files 'crates/CRATE/*.rs'` at 14871e01b3, all lines then non-test lines; they cover the whole crate, not only its zero-use commands.

- north-ops: 4 zero-use commands, 17426 lines (15652 non-test)
- north-runtime: 3 zero-use commands, 15780 lines (13188 non-test)
- north (root): 11 zero-use commands, 15603 lines (13900 non-test)
- north-core: 3 zero-use commands, 15275 lines (14612 non-test)
- north-providers: 1 zero-use commands, 10641 lines (7898 non-test)
- north-capacity-host: 4 zero-use commands, 9943 lines (8745 non-test)
- north-flow: 1 zero-use commands, 9429 lines (8647 non-test)
- north-dispatch: 2 zero-use commands, 8998 lines (6877 non-test)
- north-landing: 5 zero-use commands, 7389 lines (6383 non-test)
- north-lander: 2 zero-use commands, 7194 lines (4515 non-test)
- north-frontier: 1 zero-use commands, 2786 lines (2596 non-test)
- north-graph-ingest: 1 zero-use commands, 1900 lines (1560 non-test)
- north-coverage: 1 zero-use commands, 1297 lines (1096 non-test)
- north-mutants: 1 zero-use commands, 1162 lines (1114 non-test)
- north-graph-ground: 6 zero-use commands, 1046 lines (895 non-test)

## All commands

| Command | Crate | Uses | Laptop convo | gex45 convo | nexus convo | Laptop units | gex45 units | nexus units | Hook refs | Source refs | Judgement |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `fleet` | north (root) | 35496 | 29025 | 6460 | 7 | 2 | 1 | 1 | 0 | 172 |  |
| `gate` | north (root) | 2307 | 1446 | 833 | 28 | 0 | 0 | 0 | 0 | 16 |  |
| `capacity host run` | north-capacity-host | 1642 | 974 | 663 | 0 | 4 | 1 | 0 | 0 | 10 |  |
| `threads` | north-calibration | 1344 | 709 | 632 | 3 | 0 | 0 | 0 | 0 | 27 |  |
| `convo` | north-convo | 1104 | 688 | 414 | 0 | 2 | 0 | 0 | 0 | 15 |  |
| `runtime update` | north-runtime | 1065 | 787 | 274 | 0 | 2 | 1 | 1 | 0 | 14 |  |
| `capacity host status` | north-capacity-host | 984 | 524 | 460 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `hook` | north (root) | 918 | 334 | 239 | 0 | 0 | 0 | 0 | 345 | 14 |  |
| `docs features-index` | north-ops | 663 | 449 | 214 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `land status` | north-lander | 505 | 294 | 211 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `task` | north (root) | 417 | 362 | 55 | 0 | 0 | 0 | 0 | 0 | 8 |  |
| `runtime health` | north-runtime | 410 | 313 | 97 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `tune` | north-tune | 406 | 165 | 240 | 0 | 0 | 0 | 1 | 0 | 16 |  |
| `health` | north-health | 338 | 205 | 132 | 0 | 1 | 0 | 0 | 0 | 17 |  |
| `capacity bounded` | north-ops | 318 | 305 | 13 | 0 | 0 | 0 | 0 | 0 | 7 |  |
| `job-run` | north-ops | 318 | 75 | 171 | 0 | 42 | 10 | 20 | 0 | 7 |  |
| `runtime deploy` | north-runtime | 294 | 206 | 88 | 0 | 0 | 0 | 0 | 0 | 6 |  |
| `dash` | north (root) | 265 | 242 | 23 | 0 | 0 | 0 | 0 | 0 | 21 |  |
| `land promote` | north-lander | 242 | 235 | 7 | 0 | 0 | 0 | 0 | 0 | 14 |  |
| `capacity offload choose` | north-capacity-host | 224 | 168 | 56 | 0 | 0 | 0 | 0 | 0 | 7 |  |
| `land withdraw` | north-lander | 224 | 160 | 64 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `land serve` | north-lander | 208 | 139 | 57 | 0 | 12 | 0 | 0 | 0 | 6 |  |
| `lane remove` | north-lanes | 205 | 122 | 83 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `safe-push` | north (root) | 204 | 138 | 66 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `fleet usage` | north (root) | 194 | 161 | 28 | 0 | 2 | 2 | 1 | 0 | 12 |  |
| `cloud launch` | north (root) | 193 | 170 | 23 | 0 | 0 | 0 | 0 | 0 | 9 |  |
| `retain` | north-ops | 189 | 84 | 104 | 0 | 1 | 0 | 0 | 0 | 5 |  |
| `lane new` | north-lanes | 182 | 134 | 48 | 0 | 0 | 0 | 0 | 0 | 5 |  |
| `fleet dispatch` | north-dispatch | 179 | 134 | 44 | 0 | 0 | 0 | 1 | 0 | 24 |  |
| `ledger flow` | north-flow | 176 | 112 | 63 | 0 | 1 | 0 | 0 | 0 | 26 |  |
| `code-health gate` | north-code-health | 175 | 99 | 76 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `capacity dev-shell run` | north-capacity-host | 159 | 144 | 14 | 0 | 1 | 0 | 0 | 0 | 8 |  |
| `graph ask` | north-graph-change | 158 | 123 | 35 | 0 | 0 | 0 | 0 | 0 | 7 |  |
| `code-health crate-lines` | north-code-health | 147 | 109 | 38 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `cloud list` | north (root) | 145 | 74 | 4 | 67 | 0 | 0 | 0 | 0 | 5 |  |
| `cloud wait` | north (root) | 142 | 108 | 12 | 22 | 0 | 0 | 0 | 0 | 6 |  |
| `code-health report` | north-code-health | 142 | 89 | 53 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan test-admission` | north-land-scans | 142 | 117 | 25 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `help` | north (root) | 129 | 63 | 66 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `docs rationale-index` | north-ops | 123 | 89 | 34 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `loops check` | north-tune | 114 | 74 | 39 | 0 | 1 | 0 | 0 | 0 | 4 |  |
| `ledger runs` | north-providers | 107 | 47 | 60 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `hook prompt-grounding` | north (root) | 98 | 54 | 21 | 0 | 0 | 0 | 0 | 23 | 2 |  |
| `gh-session` | north-core | 97 | 47 | 50 | 0 | 0 | 0 | 0 | 0 | 5 |  |
| `stale-revert` | north-landing | 96 | 26 | 70 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `code-health habits` | north-code-health | 94 | 52 | 42 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `ledger worker` | north (root) | 93 | 69 | 21 | 0 | 1 | 1 | 1 | 0 | 18 |  |
| `capacity gaming status` | north-capacity-host | 92 | 91 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `hardcoded-repo-path-check` | north-lanes | 92 | 24 | 68 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ops dividend` | north-ops | 92 | 47 | 45 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `lane gc` | north-lanes | 89 | 64 | 22 | 0 | 1 | 1 | 1 | 0 | 13 |  |
| `check harness` | north-core | 87 | 77 | 10 | 0 | 0 | 0 | 0 | 0 | 9 |  |
| `language-gate` | north-landing | 86 | 64 | 22 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `cloud send` | north (root) | 85 | 83 | 0 | 2 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity mode status` | north-capacity-host | 84 | 76 | 8 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `graph ingest` | north-graph-ingest | 83 | 65 | 17 | 0 | 0 | 0 | 1 | 0 | 5 |  |
| `lane unlock` | north-lanes | 82 | 73 | 9 | 0 | 0 | 0 | 0 | 0 | 6 |  |
| `dep-gate` | north-landing | 81 | 55 | 26 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `check privacy judge` | north-runtime | 80 | 55 | 25 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `code-health build-time` | north-code-health | 76 | 51 | 24 | 0 | 0 | 1 | 0 | 0 | 4 |  |
| `ops check` | north-ops | 76 | 40 | 33 | 0 | 1 | 0 | 2 | 0 | 2 |  |
| `agent-policy-contract` | north-core | 75 | 47 | 28 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `autoland-status` | north-core | 73 | 22 | 51 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `denylist attest` | north-landing | 70 | 47 | 23 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `denylist scan` | north-landing | 70 | 52 | 18 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `lane seed` | north-lanes | 69 | 53 | 16 | 0 | 0 | 0 | 0 | 0 | 5 |  |
| `capacity admission` | north-capacity-host | 68 | 48 | 20 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `lane stray` | north-lanes | 66 | 54 | 12 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `config agents sync` | north (root) | 64 | 55 | 9 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `evidence start` | north-ops | 64 | 34 | 30 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `graph ready` | north-graph-ground | 63 | 50 | 13 | 0 | 0 | 0 | 0 | 0 | 9 |  |
| `cloud events` | north (root) | 62 | 58 | 1 | 3 | 0 | 0 | 0 | 0 | 1 |  |
| `hook stats` | north (root) | 56 | 36 | 20 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `capacity laptop-auto` | north-capacity-host | 55 | 48 | 6 | 0 | 1 | 0 | 0 | 0 | 2 |  |
| `land gate-stats` | north-lander-doctor | 53 | 21 | 32 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `capacity watchdog sample` | north-capacity-host | 49 | 28 | 18 | 0 | 1 | 1 | 1 | 0 | 1 |  |
| `follows-release` | north-landing | 49 | 38 | 11 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `land scan dep-gate` | north-land-scans | 47 | 24 | 23 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `session-host` | north-session-host | 47 | 37 | 6 | 4 | 0 | 0 | 0 | 0 | 6 |  |
| `git-credential get` | north-core | 46 | 6 | 40 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `landed-tree` | north-landing | 46 | 42 | 4 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `after-landing` | north-landing | 45 | 45 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `ops watch` | north-land-watch | 45 | 33 | 9 | 0 | 1 | 1 | 1 | 0 | 10 |  |
| `work brief new` | north-context-pack | 45 | 44 | 1 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `lane reclaim` | north-ops | 42 | 39 | 3 | 0 | 0 | 0 | 0 | 0 | 5 |  |
| `ops climb run` | north-climb | 41 | 32 | 9 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `defaults-measure` | north-ops | 40 | 16 | 24 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `lane rescue` | north-lanes | 40 | 16 | 24 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `merge-driver continue` | north-landing | 37 | 20 | 17 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `config agents status` | north (root) | 36 | 19 | 17 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `land missed` | north-landing | 36 | 21 | 15 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `build-cache-gate` | north-landing | 35 | 8 | 27 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `check harness tree-policy` | north-core | 35 | 22 | 13 | 0 | 0 | 0 | 0 | 0 | 6 |  |
| `config agents off` | north (root) | 34 | 30 | 4 | 0 | 0 | 0 | 0 | 0 | 7 |  |
| `config agents on` | north (root) | 34 | 26 | 8 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `lane exec` | north-lanes | 34 | 29 | 5 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `broker github-token` | north-broker | 33 | 4 | 28 | 0 | 1 | 0 | 0 | 0 | 2 |  |
| `capacity game-reserve on` | north-capacity-host | 33 | 32 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `graph show` | north-graph-ingest | 32 | 25 | 7 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `tool-mine` | north-flow | 32 | 26 | 5 | 0 | 1 | 0 | 0 | 0 | 2 |  |
| `work brief pack` | north-context-pack | 30 | 30 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `work list` | north (root) | 30 | 12 | 18 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `cloud collect` | north (root) | 29 | 18 | 11 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `runtime agents` | north-runtime | 29 | 11 | 15 | 0 | 1 | 1 | 1 | 0 | 1 |  |
| `capacity host session` | north-capacity-host | 28 | 9 | 19 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `check citations` | north-ops | 28 | 22 | 6 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `config agents check-owners` | north (root) | 27 | 22 | 5 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `fleet laptop status` | north-dispatch | 27 | 18 | 9 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `nix explain` | north (root) | 27 | 10 | 17 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `pin-retire` | north-lanes | 27 | 18 | 9 | 0 | 0 | 0 | 0 | 0 | 6 |  |
| `remote-serve` | north (root) | 27 | 15 | 12 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `cloud land` | north (root) | 26 | 15 | 11 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `graph validate` | north-graph-ingest | 26 | 15 | 11 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ledger incident report` | north-providers | 26 | 12 | 14 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `concerns check` | north-concerns | 25 | 16 | 9 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `fleet digest` | north (root) | 24 | 24 | 0 | 0 | 0 | 0 | 0 | 0 | 5 |  |
| `broker serve` | north-broker | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity host backlog` | north-capacity-host | 23 | 20 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `check experiments` | north-flow | 23 | 17 | 6 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `lane layout` | north-lanes | 23 | 21 | 2 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `check calls` | north (root) | 22 | 21 | 1 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `land release` | north-safe-push | 22 | 9 | 13 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `runtime opencode` | north-runtime | 22 | 22 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `ssh-session` | north-core | 22 | 11 | 11 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `desktop private start` | north-ops | 21 | 21 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `ledger opencode` | north-providers | 21 | 19 | 2 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `config proxy calls` | north (root) | 20 | 3 | 17 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `land scan denylist` | north-land-scans | 20 | 8 | 12 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `laptop-runner` | north-runtime | 20 | 7 | 10 | 0 | 1 | 1 | 1 | 0 | 2 |  |
| `work show` | north (root) | 20 | 10 | 10 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity host probe` | north-capacity-host | 19 | 3 | 16 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `nix gc` | north (root) | 19 | 13 | 6 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `private-check` | north-ops | 19 | 11 | 8 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `code-health signal` | north-code-health | 18 | 15 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan test-integrity` | north-land-scans | 18 | 6 | 12 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `watch-review` | north-runtime | 18 | 17 | 0 | 0 | 1 | 0 | 0 | 0 | 2 |  |
| `guard language` | north (root) | 17 | 15 | 2 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `ops github budget` | north-ops | 17 | 15 | 2 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `config agents live-refs` | north (root) | 16 | 7 | 9 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `bin-references` | north-landing | 15 | 3 | 12 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `incident-mine` | north-ops | 15 | 4 | 11 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `merge-driver set` | north-landing | 15 | 9 | 6 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `runtime pi` | north-runtime | 15 | 15 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `broker inspect` | north-broker | 14 | 3 | 11 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ci-diagnose` | north-ops | 14 | 8 | 6 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `desktop-idle-state` | north-guard | 14 | 12 | 0 | 0 | 2 | 0 | 0 | 0 | 0 |  |
| `github-mirror sync` | north-github-mirror | 14 | 12 | 0 | 0 | 1 | 0 | 1 | 0 | 1 |  |
| `graph tell` | north-graph-ingest | 14 | 12 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `work search` | north (root) | 14 | 8 | 6 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity gaming on` | north-capacity-host | 13 | 13 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `graph ground query` | north-graph-ground | 13 | 13 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `ops credentials check` | north-ops | 13 | 10 | 3 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `work claim` | north (root) | 13 | 10 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity host hand-over` | north-capacity-host | 12 | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity mode hold` | north-capacity-host | 12 | 5 | 7 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `contract-probe` | north-core | 12 | 9 | 0 | 3 | 0 | 0 | 0 | 0 | 1 |  |
| `incident-retro` | north-runtime | 12 | 2 | 9 | 0 | 1 | 0 | 0 | 0 | 1 |  |
| `land clear-withdrawal` | north-lander | 12 | 4 | 8 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `ssh-master probe` | north-core | 12 | 9 | 1 | 0 | 1 | 1 | 0 | 0 | 1 |  |
| `capacity host mode` | north-capacity-host | 11 | 4 | 4 | 0 | 2 | 1 | 0 | 0 | 0 |  |
| `capacity watchdog report` | north-capacity-host | 11 | 9 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `cloud sweep` | north (root) | 11 | 11 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `code-health check` | north-code-health | 11 | 3 | 8 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `eta-calibration` | north-calibration | 11 | 2 | 8 | 0 | 1 | 0 | 0 | 0 | 0 |  |
| `gh-broker serve` | north-ops | 11 | 8 | 0 | 0 | 1 | 1 | 1 | 0 | 1 |  |
| `land scan tool-removal` | north-land-scans | 11 | 6 | 5 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `nexus-ram record` | north-ops | 11 | 10 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ops strategy replay` | north (root) | 11 | 11 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `session-unit` | north-core | 11 | 7 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `capacity host estimate` | north-capacity-host | 10 | 4 | 6 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land abandon` | north-landing | 10 | 9 | 1 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `land scan gate-shadow` | north-land-scans | 10 | 4 | 6 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `lane reap` | north-lanes | 10 | 6 | 4 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `ledger tempo` | north-flow | 10 | 5 | 4 | 0 | 1 | 0 | 0 | 0 | 3 |  |
| `resident-cap status` | north-dispatch | 10 | 6 | 4 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `test coverage refresh` | north-coverage | 10 | 6 | 3 | 0 | 1 | 0 | 0 | 0 | 3 |  |
| `capacity host admit-job` | north-capacity-host | 9 | 6 | 3 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `capacity watchdog step` | north-capacity-host | 9 | 7 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan docs-owners` | north-land-scans | 9 | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan test-admission run` | north-land-scans | 9 | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `red-main-notify` | north-ops | 9 | 6 | 3 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `test frontier report` | north-frontier | 9 | 8 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `capacity seat-idle-hint` | north-capacity-host | 8 | 7 | 0 | 0 | 1 | 0 | 0 | 0 | 1 |  |
| `config check guides` | north (root) | 8 | 7 | 1 | 0 | 0 | 0 | 0 | 0 | 10 |  |
| `capacity gaming off` | north-capacity-host | 7 | 7 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `capacity mode release` | north-capacity-host | 7 | 5 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `code-health review-done` | north-code-health | 7 | 7 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `config agents check-hook-paths` | north (root) | 7 | 3 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `land scan doc-links` | north-land-scans | 7 | 0 | 7 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan secrets` | north-land-scans | 7 | 2 | 5 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ledger incident record` | north-providers | 7 | 0 | 7 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `main-red` | north-ops | 7 | 3 | 4 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `policy-replay promote` | north-convo | 7 | 2 | 5 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `session-link` | north-session-host | 7 | 7 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `check issue-boxes` | north-landing | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `check provenance debt` | north-landing | 6 | 5 | 1 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `config agents plan` | north (root) | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 4 |  |
| `fleet laptop open` | north-dispatch | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan publication` | north-land-scans | 6 | 2 | 4 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan test-admission kills` | north-land-scans | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `skill-review-queue` | north-runtime | 6 | 4 | 1 | 0 | 0 | 0 | 1 | 0 | 0 |  |
| `usage-web-read` | north-runtime | 6 | 2 | 4 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `config agents path` | north (root) | 5 | 1 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `land tests` | north-landing | 5 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `lane squash` | north-landing | 5 | 4 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `test coverage select` | north-coverage | 5 | 5 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `work release` | north (root) | 5 | 3 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `alarm stats` | north (root) | 4 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `capacity watchdog rest-check` | north-capacity-host | 4 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `codex-runtime-gcroots` | north-runtime | 4 | 0 | 1 | 0 | 1 | 1 | 1 | 0 | 0 |  |
| `config agents inspect` | north (root) | 4 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `desktop warcraft close` | north-ops | 4 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `fleet laptop tick` | north-dispatch | 4 | 1 | 3 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `fleet report` | north-ops | 4 | 3 | 1 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `git-maintenance-nightly` | north-runtime | 4 | 1 | 0 | 0 | 1 | 1 | 1 | 0 | 0 |  |
| `job-watch run` | north-runtime | 4 | 1 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ledger duplicate` | north-providers | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `provider-capacity events` | north-capacity-host | 4 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `remote-serve audit` | north (root) | 4 | 3 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `skill-prune` | north-runtime | 4 | 0 | 3 | 0 | 1 | 0 | 0 | 0 | 1 |  |
| `test frontier run` | north-frontier | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `test mutant sweep` | north-mutants | 4 | 1 | 2 | 0 | 1 | 0 | 0 | 0 | 1 |  |
| `capacity host offload-cost` | north-capacity-host | 3 | 1 | 2 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `capacity host renew` | north-capacity-host | 3 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity mode extend` | north-capacity-host | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `capacity nexus` | north-runtime | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `check experiments defer` | north-flow | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `config agents busy` | north (root) | 3 | 1 | 0 | 0 | 2 | 0 | 0 | 0 | 0 |  |
| `config context audit` | north (root) | 3 | 2 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `cross-project-scan` | north-runtime | 3 | 1 | 1 | 0 | 1 | 0 | 0 | 0 | 0 |  |
| `desktop private reap` | north-ops | 3 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `fleet pace gate` | north-dispatch | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `fleet pace sample` | north-dispatch | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan commit-email` | north-land-scans | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land scan native-ref` | north-land-scans | 3 | 0 | 3 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `lane sweep` | north-ops | 3 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `runtime claude` | north-runtime | 3 | 3 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `capacity host land-waiting` | north-capacity-host | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `cloud reap` | north (root) | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 6 |  |
| `desktop capture` | north-ops | 2 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `desktop warcraft login-field` | north-ops | 2 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `fleet laptop observe` | north-dispatch | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `land red-check` | north-landing | 2 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `nexus-ram report` | north-ops | 2 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `outside-watch run` | north-runtime | 2 | 0 | 1 | 0 | 1 | 0 | 0 | 0 | 1 |  |
| `resident-cap gate` | north-dispatch | 2 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `supply-chain-scan` | north-runtime | 2 | 0 | 1 | 0 | 1 | 0 | 0 | 0 | 0 |  |
| `test coverage record` | north-coverage | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 3 |  |
| `work close` | north (root) | 2 | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `code-health split move-module` | north-code-health | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `config agents stale` | north (root) | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `fleet laptop drain` | north-dispatch | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `fleet laptop toggle` | north-dispatch | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `fleet orders add` | north (root) | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `graph ground score` | north-graph-ground | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `land clean` | north-landing | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 2 |  |
| `land train check` | north-lander | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `lane audit` | north-lanes | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ledger incident guard` | north-providers | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `ssh-argv` | north-core | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `test frontier grade` | north-frontier | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |  |
| `test frontier tasks` | north-frontier | 1 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 1 |  |
| `agent-contain sweep` | north-runtime | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | ZERO KEEP |
| `capacity host release` | north-capacity-host | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `capacity host reserve` | north-capacity-host | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `capacity mode tick` | north-capacity-host | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `check experiments judge` | north-flow | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | ZERO TRY |
| `check harness policy-gate` | north-core | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO KEEP |
| `check nix` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | ZERO KEEP |
| `check provenance audit` | north-landing | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO RETIRE |
| `check provenance check` | north-landing | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO KEEP |
| `check provenance inventory` | north-landing | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | ZERO RETIRE |
| `config agents routines list` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `config agents routines pointer` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `config agents routines show` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `config agents usage` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO RETIRE |
| `desktop private capture` | north-ops | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `desktop private clipboard` | north-ops | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `desktop private control` | north-ops | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `desktop private type` | north-ops | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `fleet laptop close` | north-dispatch | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `fleet orders list` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO KEEP |
| `fleet pace place` | north-dispatch | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `git-credential erase` | north-core | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `git-credential store` | north-core | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `graph ground distill capture` | north-graph-ground | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `graph ground distill run` | north-graph-ground | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `graph ground distill status` | north-graph-ground | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `graph ground hook` | north-graph-ground | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO RETIRE |
| `graph ground import` | north-graph-ground | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO RETIRE |
| `graph ground refresh` | north-graph-ground | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `graph retract` | north-graph-ingest | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `land actions` | north-landing | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | ZERO KEEP |
| `land fails-first` | north-landing | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO KEEP |
| `land train record` | north-lander | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `land train red` | north-lander | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `ledger incident import` | north-providers | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO RETIRE |
| `ops strategy log check` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `ops strategy refusals` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 | ZERO TRY |
| `outside-watch status` | north-runtime | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO KEEP |
| `provider-capacity status` | north-capacity-host | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO TRY |
| `runtime agy` | north-runtime | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO KEEP |
| `test coverage miss` | north-coverage | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3 | ZERO TRY |
| `test frontier oracle` | north-frontier | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ZERO RETIRE |
| `test mutant record` | north-mutants | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `work need` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `work review` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
| `work unneed` | north (root) | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 1 | ZERO TRY |
