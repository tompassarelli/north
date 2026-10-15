# Codex leads and Codex briefs

Read this when a Codex lead runs a workstream, when you write a brief a Codex worker will run, or when north runs on Codex alone.

## Running a Codex lead

- Start it like any lead (`north fleet launch --provider codex --account A --parent <your name> --system-prompt BRIEF GOAL`), message it with `north fleet send NAME TEXT`, and read its state and final report with `north fleet show NAME`, because fleet records delivery and completion and no window or relay is needed. Pick its model and effort from `agents plan` and `worker-ledger --summary`, never low, max or Astra unless Tom names it.
- Give it a goal that closes every ordered item on GitHub or names a blocker with evidence and an owner, so it cannot finish early on a partial queue. Its brief lists scope, first reads, the ordered queue as absolute paths to brief files, checks and ETA.
- In mixed-provider mode the Claude lead writes every Codex box brief and the Codex lead registers it with `threads register-spawn` and spawns it verbatim, because Codex-lead-split boxes landed 71% against 94% under Claude leads. On Codex alone, the Codex lead writes and staffs every box itself under the same brief rules below.
- Route questions up one rung at a time (Codex worker, Codex lead, parent lead, proxy, Tom), and Tom only for items on the ask-Tom list, so each rung answers what it can.
- Map every open issue to a running worker or a named blocker at most every 20 minutes until the queue empties, and give slots to main-red issues first, then correctness and gameplay, release gates, then polish, because an unmapped issue sits unstaffed.
- Ask owners to release blocking exclusive leases or over-pressure native pools at a safe phase, rather than waiting on them.
- The lead runs no builds, checks or raw worker-log reads itself; it asks the owning worker for the fact, because the orchestration hook refuses them (`ORCH_RUNS_BECAUSE` only for a staffing write or capacity estimate the hook misreads).
- Ask the lead for encrypted worker briefs rather than reading them from its log, because logs may hold secrets.
- Before starting a review-gated worker, run `touch "$(git -C LANE rev-parse --path-format=absolute --git-path lead-lands)"` so safe-push refuses its push, because brief wording alone did not stop Codex workers from landing.
- Harness landing follows roles: proxy, lead and sub-lead may review and land durable harness changes whatever their provider; workers commit for their lead to land. Resolve the live role with `safe-push --harness-role`.
- Codex workers run local affected checks and push no branch CI, because the landing queue is the farm; before a lane pushes it runs the landing queue's checks for its touched modules, including those skipped locally by default. Wait on farm or CI with one blocking command or `gh run watch RUN --exit-status`, never a polling loop, and share Actions jobs and API calls through `github`.
- Cross-review netcode, rollback and determinism, save and replay formats and balance gates before landing, and give each Claude-landed commit in mixed mode one Sol high reviewer that reports file:line and a failing case, because those defects surface only in play. Compare Claude and Codex ledger rows for the same category only in mixed mode.
- [codex-sessions.md](codex-sessions.md) covers Codex thread and log lookup.

## Writing a brief a Codex worker runs

- Write it verbosely: the goal and why, explicit anti-goals (no unrequested formal machinery, scope, tests or abstractions), exact files, the behavior each box may change, Done when and ETA, because Codex fills unstated gaps with machinery. Review every diff for behavior drift as well as path drift before it lands.
- Use normal sentences with spaces between words, because the readability hook refuses any run of 25 or more joined letters.
- Read the clock with `date -u` before writing an ETA, deadline or lease renewal, because times estimated from the conversation ran late and let a lease expire.
- Register and start each worker in one step, and restaff a named one-line compiler fix before any other orchestration, because split registration and late restaffs each cost minutes on the critical path.
- Write every path in full, including the brief, the result file and AGENTS.md, because each shorthand name cost a clarification round.
- Paste the exact contract text, with one filled example of any template, instead of pointing at a plan, and bound each read by a selector anchored at a heading, because guessed line ranges and broad `rg` spilled neighbouring tasks.
- State the exact expected text of every output the box changes (diagnostics, headings, verdicts, exit codes) and one example on each side of any boundary the brief draws, because general wording ended in review cuts; list no checks the requirement does not ask for.
- Name the control case that must stay byte-identical, the helper, API or scenario to reuse, the tests covering each touched command, and every caller found by a repository-wide search of current main, including test corpora and module declarations, because Codex otherwise rebuilds what exists.
- When the file list blocks the real fix, the worker stops and asks for a measured expansion, because workarounds and compatibility wrappers were cut in review.
- Name the invariant a requested property must check, or say no new test, because an open "scenario or property" produced a tautology.
- Require the worker to reproduce the failure on current main before editing, run the whole public journey on real inputs before the affected suite, and run the full workspace after the final rebase, because syntax-only and synthetic gates passed while the product failed.
- Before naming a comparison commit, source artifact or test root, confirm on current main that it exists and holds what the box needs, because a missing comparison commit and a retired setup artifact each cost a correction round. Keep probes, fixtures, scripts and backups inside the owned lane, never `/tmp` or the handoffs directory, which the build sandbox cannot see, and set `PYTHONDONTWRITEBYTECODE=1` so Python checks write no caches.
- Have the worker derive every SHA, column, filename, test location and command syntax from its source (`git rev-parse`, the schema, `--help`, a listing), because guessed values were wrong.
- Before ruling that a property or check changes, counting a mutant kill, queuing a generated script or diagnosing contention, the worker reads the property's input cases, the assertion context, the script body or the current queue and admission state, because verdicts made without them were wrong.
- Before each commit the worker confirms `git diff --stat` against the merge base lists only its authorized paths, because whole-tree scripts, `cargo fmt` and mutation runs leaked edits.
- The worker writes its result to the named result file, takes test totals from the runner output, reports its native id from `CODEX_THREAD_ID`, and chains the final check and landing with `&&`, never `;`, so a failed check never lands.
- Give a review-only worker the frozen gate results and corpus counts and forbid rebuilding or rerunning suites, because reruns burn the box's time.
- Name tests and assertions the worker must not change as "pre-existing on origin/main", never "existing", because Codex stopped on its own new tests.
