---
name: orchestrating-codex
agents: [claude]
description: >-
  Run work through a Codex lead session that Claude supervises: start it, give it a goal, message it, watch its worker tree and keep it on a tight leash. Use when Tom asks to drive work through Codex, or for a Codex lead or orchestrator.
grounded: 2026-10-11
written: 2026-10-11
metadata:
  kind: playbook
---

# Supervise a Codex lead

- Use `codex-lead --help` for start/id/send/goal/status/workers syntax.
- Never type into its window or take Tom's focus.
- Assign SOL-starting bands to the Codex lead and Claude-starting bands to Claude workers.
- Read `workers`; choose tiers from the plan and `worker-ledger --summary`, never low/max or Astra without Tom naming it.
- Write `~/.local/state/agents/handoffs/codex-lead-brief.md` with scope, first reads, ordered queue, checks, files, ETA, the lead's staffing/landing/status role and every line of the next section, because the Codex lead cannot load this skill.
- Require worker reports beginning `Done:`, `Not done:` or `Blocked:` and brief fields `Item`, `Category`, plus `Follows` on retries/escalations/continuations.
- Start the lead with `agents lead start --provider codex --domain D --brief BRIEF`, which registers it in `agents org`.
- Use `CODEX_LEAD_THREAD=<id>` for an existing live lead.
- Give it a goal closing every ordered item on GitHub or naming a blocker with evidence and an owner.
- Mark a box `code-only` when it needs no local build or client, so its worker runs no builds and takes no leases; a named validator runs its checks.
- Start all independent items together; open/claim Claude-held items and tell the lead before staffing them.
- Harness landing follows RBAC: proxy, lead and sub-lead roles may review and land durable harness changes regardless of provider or model. Workers commit for their lead to review and land. Resolve the live role with `safe-push --harness-role`; assign or change registered roles with `agents org add` or `agents org set`.
- Share GitHub Pro's 40 concurrent Actions jobs and 5,000 API calls/hour through `github-actions`; confirm the exact revision before farm dispatch.
- Codex workers run local affected checks and push no branch CI; the repository's landing queue is the farm.
- Before a lane pushes, it runs the landing queue's checks for its touched modules, including those skipped locally by default (target-language compile, sweeps).
- Wait on farm/CI with one blocking repository command or `gh run watch RUN --exit-status`; never poll in a loop.
- Monitor `~/.local/state/agents/handoffs/codex-lead-peer.md` and the Codex lead's status file with `tail -n0 -F FILE | grep --line-buffered -E '^(QUESTION|PEER|HANDOFF)'`, with a 30-minute timeout rearmed on expiry, and answer each `QUESTION` through `codex-lead send` within minutes.
- Route questions up one rung at a time: Codex worker → Codex lead → Claude parent lead → proxy → Tom, and Tom only for items on the ask-Tom list.
- Escalate a worker past 2× ETA, a second `Not done` on one box, or an unavailable next plan tier with `PEER <HH:MM> <repo#N>: <tried by tiers>; <failure>; <next idea>`.
- Answer PEER within minutes through `codex-lead send` with smaller pieces and leads.
- After that attempt fails, append `HANDOFF <HH:MM> <repo#N>: <state, branch, evidence>` and transfer staffing to Claude.
- Keep `codex-lead-status.md` in the same handoffs directory current after every landing, failure and staffing change, with Updated/Running/Closed/Blocked/Next; a `DONE <item>` line finishes nothing, and only when its whole goal is done does the lead append `FINISHED <its org name>` as the last line, on which the parent's tick precheck runs `agents org finish`.
- Include item, worker path, model/effort and ETA under Running; numbers under Closed; evidence and owner under Blocked.
- Check `codex-lead status`, `codex-lead workers`, issue closures, `threads list` claims and capacity at most every 20 minutes until the queue empties.
- Correct idle workers at 10+ minutes, overruns at 2× ETA, wrong tiers, missing claims/report fields and serialized independent work.
- Map every open issue to a Codex worker idle under 10 minutes, a running Claude worker, or a named blocker; correct closed-issue workers and short staffing too.
- Give slots to main-red issues, correctness/gameplay, release gates, then polish.
- Ask owners to release blocking exclusive leases or over-pressure native pools at a safe phase.
- Send one correction message per check, naming each gap/fix and requesting one owner/blocker line per issue.
- Refresh the goal hourly and immediately on queue changes, and keep supervision scheduled for the goal's lifetime.
- Compare same-category Claude/Codex ledger rows only in this mixed-provider mode; use the ledger over the Opus 5.5 medium/Astra chart prior of 20–40% cost.
- Cross-review netcode, rollback/determinism, save/replay formats and balance gates before landing.
- Have the lead post `REVIEW <item> <branch> <commit>` to the peer file the moment a lane is ready, and list every REVIEW line without a later REVIEWED line each tick, because reviews left in a status file or scrolled past sat unlanded for hours; report concrete defects within 20 minutes and land after answers or 20 minutes without findings.
- Send Claude-landed commits to the lead for one SOL high reviewer per item; require file:line and failing case and fix forward.
- Never use `codex queue`; deliver mid-turn corrections through `codex-lead send`, and only to threads with live sessions; restart a finished exec session before expecting replies.
- Ask the lead for encrypted worker briefs rather than reading them from its log.
- Read [session diagnostics](references/session-diagnostics.md) only for socket, thread/log lookup or test-thread cleanup.
- Report a PEER line instead of additional checks, and report completion when the queue is empty.

## Codex lead and worker briefs

- Tell the lead and every worker brief that a question goes to its status file as a `QUESTION <HH:MM> <item>: <question>` line, never through the ask tool (`request_user_input`), which a managed hook refuses.
- Write briefs, status lines and lessons as normal sentences with spaces between words; the readability hook refuses any run of 25 or more joined letters.
- When staffing, queue each box that waits on another lane with its brief, carrying `Follows: <commit subject, SHA or Item>`, piped to `threads queue follow <repo#N> --tier <tier> --lead <your org name>`, so its predecessor's landing starts it instead of a lead noticing.
- Read the clock with `date -u` before writing an ETA, deadline or lease renewal; times estimated from the conversation ran late and let a lease expire.
- Register and start each worker in one step, and restaff a named one-line compiler fix before any other orchestration; split registration and late restaffs each cost minutes on the critical path.
- Run no builds, checks or raw worker-log reads as the lead; ask the owning worker for the fact, and set `ORCH_RUNS_BECAUSE` only for a staffing write or capacity estimate the hook misreads.
- Before starting a review-gated worker, run `touch "$(git -C LANE rev-parse --path-format=absolute --git-path lead-lands)"` so that safe-push refuses its push; brief wording alone did not stop Codex workers from landing.
- Write every path in full, including the common brief, the peer file, the result file and AGENTS.md; each shorthand name cost a clarification round.
- Paste the exact contract text, with one filled example of any template, instead of pointing at a plan, and bound each read by a selector anchored at a heading that stops at the next heading; guessed line ranges and broad `rg` spilled neighbouring tasks.
- State the exact expected text of every output the box changes (diagnostics, headings, verdicts, exit codes) and one example on each side of any boundary the brief draws, because general wording ended in review cuts; list no checks the requirement does not ask for.
- Name the existing control case (error path, exit code, stderr) that must stay byte-identical, the existing helper, API or scenario to reuse, the existing tests that cover each touched command, and every caller found by a repository-wide search of current main, including test corpora and module declarations.
- When the file list blocks the real fix, the worker stops and asks for a measured expansion instead of writing a workaround, compatibility wrapper or substitute.
- Name the invariant a requested property must check, or say no new test; an open "scenario or property" produced a tautology.
- Require the worker to reproduce the failure on current main before editing, run the whole public journey on real inputs before the affected suite, and run the full workspace after the final rebase; syntax-only and synthetic gates passed while the product failed.
- Before a brief names a comparison commit, source artifact, or test or case root, confirm on current main that it exists and holds every fact or file the box needs; a review brief without its comparison commit and a brief naming a retired setup artifact each cost a correction round. Keep probes, fixtures, scripts and backups inside the owned lane, never `/tmp` or the handoffs directory, which the build sandbox cannot see, and set `PYTHONDONTWRITEBYTECODE=1` so Python checks write no caches.
- Have the worker derive every SHA, column, filename, test location and command syntax from its source (`git rev-parse`, the schema, `--help`, a listing) instead of guessing it.
- Before ruling that a property or check changes, counting a mutant kill, queuing a generated script or diagnosing contention, read the property's input cases, the failure's assertion context, the script body or the current queue and admission state.
- Before each commit, the worker confirms `git diff --stat` against the merge base lists only its authorized paths; whole-tree scripts, `cargo fmt` and mutation runs leaked edits.
- Have the worker write its result to the named result file, take test totals from the runner output, report its native id from `CODEX_THREAD_ID`, and chain the final check and landing with `&&`, never `;`.
- Give a review-only worker the frozen gate results and corpus counts, and forbid rebuilding or rerunning suites.
