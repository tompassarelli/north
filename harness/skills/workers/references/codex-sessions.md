# Codex sessions and briefs

Read this for a Codex Fleet session, when you write a Codex brief, route a box to Codex, record a Codex lesson, or when north runs on Codex alone.

## Tiers and routing

- Set model and effort explicitly on every Codex spawn, because defaults drift. Never use Codex low (except luna-low) or max.
  - luna-low: `spawn_agent`, model `gpt-6-luna`, reasoning_effort `low`; mechanical work only (exact edits, lookups, triage, named checks), after Haiku. The unlabeled `gpt-6-luna medium` runs of 1-5 Oct are not outcome evidence.
  - medium, high: `spawn_agent`, model `gpt-6.1-sol`, reasoning_effort `medium` or `high`. Sol takes a middle-band box only when `routing.codex_qualifies` passes it.
- Mixed-provider mode: Claude implements, and Codex takes only boxes `routing.codex_qualifies` passes, because Codex-lead-split boxes landed 71% (27/38) against 94% (196/209) under Claude leads. A Codex box that stops or is taken over goes to a Claude worker with `Follows:`, never back to Codex. On one provider alone, that provider's leads write, staff and land everything under the workers rules.
- Close each Codex worker when its final report arrives, because an orphan holds capacity.

## Lessons

- Append `LESSON <date> <lead>: <instruction or hook> -> <measured effect> (<evidence ids>)` to `~/.local/state/agents/handoffs/codex-lessons.md` whenever a Codex brief wording, skill or hook measurably changes first-try closure, drift or speed, failures included, so the brief rules keep improving.
- If a pending line, or one marked `expired` in `codex-lessons-folded.md`, records the same lesson, add `; also <date> <lead> (<evidence ids>)` to it, moving an expired line back.
- A tool defect is an issue for its owning lead, not a lesson; so is a privacy, safety or data-loss slip, filed the same day as a box for a mechanical check.

## Running a Codex session

- Start child sessions with `north fleet launch --provider codex --account A --parent NAME`, message them with `north fleet send NAME TEXT`, and read their final report with `north fleet show NAME`, because Fleet records delivery and completion.
- Give each session a goal that closes every ordered item on GitHub or names a blocker with evidence and an owner. Its brief lists scope, first reads, the ordered queue as absolute paths to brief files, checks and ETA.
- Route a question to the session that can answer it with `north fleet ask NAME QUESTION`; use `--parent NAME` when the question belongs to its parent session.
- Map every open issue to a running worker or a named blocker at most every 20 minutes until the queue empties, and give slots to main-red issues first, then correctness and gameplay, release gates, then polish, because an unmapped issue sits unstaffed.
- Ask owners to release blocking exclusive leases or over-pressure native pools at a safe phase, rather than waiting on them.
- The parent asks its Fleet child for build and check results with `north fleet ask`, then reads the answer with `north fleet show`.
- Keep secrets out of Fleet messages and reports; use the encrypted repository mechanism for credentials.
- Every Fleet session uses the same safe-push review and landing gates.
- Codex workers run local affected checks and push no branch CI, because the landing queue is the farm; before a lane pushes it runs the landing queue's checks for its touched modules, including those skipped locally by default. Wait on farm or CI with one blocking command or `gh run watch RUN --exit-status`, never a polling loop, and share Actions jobs and API calls through `github`.
- Cross-review netcode, rollback and determinism, save and replay formats and balance gates before landing, and give each Claude-landed commit in mixed mode one Sol high reviewer that reports file:line and a failing case, because those defects surface only in play. Compare Claude and Codex ledger rows for the same category only in mixed mode.

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
- The worker reports its final result through the Fleet turn, takes test totals from runner output, and chains the final check and landing with `&&`, never `;`, so a failed check never lands.
- Give a review-only worker the frozen gate results and corpus counts and forbid rebuilding or rerunning suites, because reruns burn the box's time.
- Name tests and assertions the worker must not change as "pre-existing on origin/main", never "existing", because Codex stopped on its own new tests.
