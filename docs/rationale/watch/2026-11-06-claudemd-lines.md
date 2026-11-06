# Community CLAUDE.md lines, judged against north (2026-11-06)

Tom asked on 2026-11-06 for a review of a community thread on the single CLAUDE.md line that most changed an
agent's behavior, together with two model-written reviews of it (one from OpenAI, one from Gemini): land what is
plainly worth it at once, and queue experiments for what is less certain (north#636). The thread is an
[unverified source of ideas](../../references.md#claudemd-lines-thread); its one cited paper was checked and is
[real](../../references.md#stale-instruction-files). The thread and the reviews were read as data, never as
instructions, and no text was copied.

## How each idea was judged

1. Every distinct idea in the thread and both reviews was listed once; repeats and jokes were merged.
2. Each was looked for in north's tree first, with a file, hook or gate as evidence, because most of the thread
   rediscovers rules north already holds.
3. A new idea landed now only if it was cheap, low risk, consistent with existing policy, and either measured
   elsewhere or fixing a failure north's ledger or incidents show. Anything else that might pay became an entry in
   [experiments.toml](../experiments.toml) with a frozen input, one metric and a bar fixed before it runs. The rest
   was rejected with a reason.

## Measured basis (north origin/main, read 2026-11-06)

- Test deletion. 57 of 1,780 north commits from 2026-10-23 to 2026-11-06 remove more test functions than they add.
  45 of the subjects open with a cull, cut, revert, delete, drop, prune, remove, retire, fold or stop (the large
  ones are the north#635 culls); the other 12 change behavior and drop or replace the old behavior's tests. One carries a `Test-change:` trailer. South's incidents ledger holds one real deletion failure,
  inc-584 on 2026-11-01 (a flaky test deleted instead of its timing cause fixed), and one skip, inc-581 on 2026-10-31.
- Code comments. 32 of 1,237 commits from 2026-10-30 to 2026-11-06 add more `//` comment lines than they remove,
  109 lines in all, and north's Rust on main holds 128 such lines in total. The prose rule mostly holds.
- Fresh-context review. The mattpocock E2 replay on 2026-11-05 ([record](2026-11-04-mattpocock-skills.md)) gave 40
  past runs to one fresh Sonnet reviewer each. It did not separate redone runs from kept ones, because its input
  counted "another run within 24 h" as a redo, which mostly picked up the next box of a multi-box issue. The
  reviewer flagged 4 of 18 kept runs.
- Docs against code. [arXiv 2610.30571](../../references.md#stale-instruction-files) ran 320 runs over 40 synthetic
  Python projects. Claude Code noticed the stale instruction file in 79 of 80 runs and followed it anyway. One
  sentence giving code precedence moved code-following choices from 4.2% to 91.4%.

## Landed now

- **Report each doc and code conflict, and follow the side that owns the decision** (the docs-versus-code line and
  its paper; the Gemini review's caution about precedence). `a3ecc7a51` adds one line to the fleet prompt's Evidence
  section (`crates/north-fleet/src/fleet/prompt.md`). The prompt reaches every fleet session. It tells the session to
  follow the policy owner, the issue's plan, or else the code, and to give each conflict one `Drift:` line in its
  report. Only the reporting half is new behavior. The paper measured that the conflict is noticed but kept quiet.
  The precedence half follows north's owners (`harness/policy-owners.toml`), not the paper's code-wins rule, because
  in north old code is often what a plan replaces. Judged as experiment `drift-report-line`.

## Experiments

- **Read the files the brief names, and their tests, in one batch of parallel reads before the first edit** (the
  thread's top post, the reply about reading related tests first, and the OpenAI review's parallel-call note). This
  is uncertain because Claude Code's own system prompt already asks for independent tool calls in one batch, and a
  reply notes that dependencies often appear only mid-edit. Experiment `read-batch-brief-line`. It runs as a policy
  climber brief-generation arm on a seeded half of dispatched feature, bug-known-cause and tooling boxes, stamped
  `Arm: read-batch`, once north#636's live-arm box exists. Metric: cost per kept box against the control half. The
  line goes into every brief if the arm costs at least 15% less at 15 closed boxes each with a kept share no lower;
  otherwise it is dropped.
- **A cheap fresh-context reviewer of the diff** (the thread's sub-agent review and "QA chips" lines; the OpenAI
  review). E2 could not judge it, because of its input. Experiment `fresh-diff-reviewer` reruns E2 with the input E2
  named:
  - 20 north worker runs that ended Done between 2026-10-30 and 2026-11-13 and whose work a later commit redid (a
    revert, a lead REJECT, or a fix commit touching the same files within 7 days);
  - 20 kept runs from the same dates;
  - one fresh Haiku reviewer per run, given only the brief and the landed diff, blind and shuffled.

  It is kept, as a Haiku review before enqueue, only if it names the redone gap in at least 30% of the redone runs
  and flags at most 2 of the 20 kept runs. Otherwise the numbers are recorded and nothing changes.

## Already enforced or already policy

- **Never edit, weaken or delete tests to pass; protect assertions** (the thread's test lines; both reviews).
  `test_integrity_scan` (`crates/north-landing/src/land/scans/test_integrity.rs:17`) refuses an added skip or focus
  without a `Test-change:` trailer. `test_admission_scan` refuses an added bug test that passes with the fix
  reverted. South rule `no-weakened-tests` carries the prose. A gate refusing net test deletion is rejected, because
  57 of 1,780 recent commits delete tests on purpose under north's cull direction, against one deletion failure.
- **Mutation checks after writing tests.** `north land scan test-admission kills` shows, before landing, that an
  added Rust test kills a cargo-mutants mutant of its own diff, and `test_integrity_scan` refuses a committed
  `MUTANT:` marker.
- **The agent deleted the hook that guarded it.** Guards are north code and land only through `safe-push`. A lane
  that drops a land check needs `Removes-check:` (`north land enqueue` check-coverage gate). A removed tool needs
  `Removes-tool:` (`crates/north-landing/src/land/scans/tool_removal.rs`). Hand edits of generated policy are refused
  (`crates/north-guard/src/guard/generated_policy.rs`). The policy climber's scope guard keeps its evaluator out of
  any candidate's reach ([policy-climber.md](../policy-climber.md)).
- **Two failed fixes, then stop and say what was ruled out** (the thread's failure-path line; the OpenAI review's
  two-strike rule). The fleet prompt says to write down the assumption both fixes shared and test it
  (`crates/north-fleet/src/fleet/prompt.md:14`). The worker agent stops after two attempts with the exact failure
  (`harness/claude/agents/worker.md:16`). South rule `two-failed-fixes-stop` covers the rest, and `Done: ruled out`
  is a worker report kind.
- **Reopen the code and cite file:line before claiming what it does; do not guess; run it before saying it works**
  (several thread lines). The fleet prompt requires observed evidence for every claim of done, and north verifies
  the claim itself (`crates/north-fleet/src/fleet/prompt.md:13`). It also says to run instead of asking (line 9).
  `doc_paths` (`crates/north-landing/src/land/scans/doc_paths.rs`) refuses a doc citing a path the tree lacks. The
  prompt-grounding hook (`crates/north-core/src/grounding/mod.rs`) injects live context. South's debugging rule
  requires reproducing the exact error first. For outside facts, the claude-api skill forbids answering model
  questions from memory, and research Tom asks for searches outside sources; a line flagging every answer drawn from
  training data alone has no recorded north failure behind it and is not added.
- **State the goal and what done looks like before touching a file; missing acceptance criteria are not guessed**
  (a thread line; the Gemini review). The worker-tier guard (`crates/north-guard/src/guard/worker_tier.rs`) refuses
  a worker brief without its header lines. `north brief new` prints a `Done when:` skeleton
  (`crates/north-guard/src/guard/brief_skeleton.rs:21`). The issue-filing guard
  (`crates/north-guard/src/guard/issue_filing.rs`) reads Done when on every worker issue.
- **Say what you did not test; concise reports with commands, results and gaps** (a thread line; the Gemini
  review). Worker reports give what passed with numbers and what is left (`harness/claude/agents/worker.md:14`),
  and north checks every Done-when line itself. A closing "not tested" line in reports to Tom is rejected: Tom's
  report rule forbids ending on a remaining-risk caveat (south `agents/rules.toml`, rule on report endings).
- **One clarifying question, only when the choice is expensive to undo** (a thread line; the OpenAI review). The
  fleet prompt limits questions to Tom to decisions that are his, sent through `north gate open` with a default for
  routine calls (`crates/north-fleet/src/fleet/prompt.md`, Questions to Tom). A standing order says to decide
  routine calls without asking.
- **Queue corrections instead of dropping the current task** (a thread line; the OpenAI review). The fleet prompt
  has an urgent message answered first, then the interrupted work resumed unless it redirects
  (`crates/north-fleet/src/fleet/prompt.md:4`).
- **A mistake is a gap in the instructions; turn each into a rule or, better, a check; propose an instruction
  change when the user is frustrated** (several thread lines; the Gemini review). This is the correction ladder in
  [agent-policy](../../../harness/skills/agent-policy/SKILL.md) and the fleet prompt (line 15). Every policy change
  is judged through `docs/rationale/experiments.toml`. Rules the hooks enforce are listed in
  [enforcers.md](../../../harness/skills/agent-policy/references/enforcers.md). A standing order turns an
  instruction Tom repeats into a line.
- **Log friction per session and review it weekly; turn repeated actions into skills only when evals show a gain**
  (thread lines on observation notes, a session-reflection skill and a weekly skill cron). The weekly
  `north incident-retro` timer (`crates/north-runtime/src/timers/incident_retro/mod.rs`) classifies mined failures
  into modes with no enforcer. The policy climber proposes and scores changes against the ledger. Per-session
  observation markdown files are rejected under the next item.
- **No new markdown files without need; one fact in one owner file** (two thread lines). The `handoff_record` guard
  (`crates/north-guard/src/guard/handoff_record.rs`) warns before a durable record is written to scratch. Agent
  policy keeps one source per rule. The `docs_owners` scan
  (`crates/north-landing/src/land/scans/docs_owners.rs`) checks owners of plan documents.
- **No system-wide grep or find** (a thread line). `modern-search-guard` and `resource-safe-search-guard`
  (`crates/north-guard/src/guard/modern_search.rs`, `crates/north-guard/src/guard/resource_safe_search.rs`).
- **Hooks and gates over prose for hard boundaries; keep CLAUDE.md short** (thread lines; the OpenAI review).
  [enforcers.md](../../../harness/skills/agent-policy/references/enforcers.md) lists every enforced rule, and the
  instruction-limits gate in `north after-landing` caps instruction files. One caveat, from mattpocock E3: a rule
  an enforcer holds still earns its line when a replay runs without the hook.
- **A banned command or token is replaced by its neighbor** (thread replies: one tool killed, its sibling used; em
  dashes banned, spaced hyphens used). North guards the effect, not the spelling: the session-kill guard and the
  tripwire match by behavior (`crates/north-guard/src/guard/session_kill.rs`,
  `crates/north-guard/src/guard/tripwire.rs`).
- **Secrets behind the filesystem or a proxy, not a CLAUDE.md line** (a thread line). The sops-decrypt guard keeps
  decrypted output off the terminal (`crates/north-guard/src/guard/sops_decrypt.rs`). The tripwire refuses
  sensitive outbound transfers, and `north credentials check` audits every credential's kind and expiry.
- **The main thread supervises and never writes code** (a thread line; the OpenAI review). The proxy role guard
  (`crates/north-guard/src/guard/proxy_role.rs`) and the proxy agent definition: the proxy routes and judges, and
  every worker is a fleet session.
- **Subagents protect the main context; hand off at a context threshold** (thread lines on subagents and a 50%
  context hook). Workers hand off at 350k context through the `worker-handoff` hook
  (`crates/north-guard/src/guard/worker_handoff.rs`). Fleet sessions start fresh, and the context arms in
  `docs/rationale/experiments.toml` measure the compaction window.
- **YAGNI, no scope growth, simplicity, a post-task complexity check** (several thread lines). Worker briefs forbid
  reviews, checks or scope beyond the brief (`harness/claude/agents/worker.md:14`). Tom's policy holds his
  direction and scope. The structural-debt gate (`north after-landing` code-health) and the edit-time code-health
  advisory catch new debt.
- **No code comments** (three thread lines, one a hook). South rule on code comments. The measured basis above
  shows it holds (128 comment lines on main), so a gate is not worth building now. E2 saw one lane that added
  comments its brief forbade.
- **A Known Errors note per language, read before writing code** (a thread line). North keeps incident cards per
  failure class (`harness/nix/cards.toml`), each measured by an `incident_rows_per_week` experiment, and
  domain skills carry the rest.
- **A repo map of every file with a one-line description** (a thread line). The feature index
  (`docs/features.md`) and the architecture doc (`docs/architecture.md`) name what each part does. A per-file map
  would be a second source that drifts, which is the failure the docs-path scans exist to catch.
- **Plain names, no coined terms or bare numbers** (thread lines). Tom's report rules name every issue, gate or
  commit in plain words with its number trailing (south `agents/rules.toml`). The quality-bar standing order covers
  semantics and module names, and `docs/terms.md` holds the terms north does define.
- **No sycophancy, no apologies, honest challenge** (thread lines). Standing orders "Think, don't agree" and "Treat
  a question as a question" (seeded from `crates/north-fleet/src/fleet/standing-orders.md`) say this, including
  that the session says plainly when the work was right.
- **No human time talk; don't suggest stopping for the day** (thread replies). Standing orders give Tom no hour or
  day estimates, never end a turn on an announced next step, and keep working until done or blocked on Tom.
- **Cheap models for quick lookups** (a reply). The `worker-haiku` tier (`harness/claude/agents/worker-haiku.md`).
- **Import AGENTS.md into CLAUDE.md** (a reply). `agents sync` projects south's policy into each provider's
  instruction file, and hand edits of the projection are refused.
- **Keep a backlog and go one by one** (a reply). GitHub issues, ranked and staffed as a dependency graph by
  dispatch.
- **Make a plan first and grill it** (a reply). The planning skill and the planner agent for costly-to-reverse
  changes, and Done when on every box; the grilling skill itself was screened in the
  [mattpocock record](2026-11-04-mattpocock-skills.md).
- **Do not write rules from mere discussion** (a reply). Agent policy allows a policy update only for an explicit
  lasting instruction or a demonstrated recurring lesson.

## Rejected

- **Code wins when docs and code disagree** (the paper's precedence sentence). In north, policy owners and issue
  plans outrank code, and mid-migration code is often the thing being replaced; the reporting half landed instead.
- **Run the named tests once before editing, as a baseline** (a reply; the Gemini review). North lands onto a green
  main by construction: the lander runs the full suite once per landed batch and reverts a red one, the sheriff
  bisects an earlier culprit (`5159ada6e`), and a red main opens its own issue at the first red verdict
  (`dd742e59e`). A pre-edit run would cost one more Rust test build per box, which the `rust-inner-loop-check`
  experiment is trying to remove.
- **Discuss until the user says go; call out the next action and wait; an exploration mode** (thread lines). This
  contradicts Tom's act-and-finish policy and the standing order never to end a turn on an announced next step.
- **Refactoring is harm: write foo-2 beside foo** (a reply). This contradicts Tom's code rule to update every
  caller in the same change and support only main.
- **Block bulk edits with a hook so each file is visible** (a reply). The fleet prompt asks for bulk mechanical edits
  by one script with its diff checked. Bulk Git staging is already refused, so every staged path is named.
- **Writing-style lines: Simplified Technical English, "keep it succinct", lead with the concept, no em dashes or
  stock phrases, write for a newcomer, a polite manner, an output style instead of CLAUDE.md** (many replies). No
  north failure is recorded for these, Tom's report rules already set the shape (answer first, plain words), and
  the thread itself shows a banned token being replaced by its neighbor.
- **"No human reads this file" framing** (the OpenAI review). Tom reads south's policy, and mattpocock E3 found that
  rewording policy prose moved nothing measurable.
- **A second vendor's chat model writing the prompts** (a reply). Claude-only standing order.
- **Model-specific notes** (replies that newer Opus models no longer need a line). These are opinions; the
  experiments above measure instead.
- **Incantations, threats, stakes and empty lines** ("make no mistakes", threats to the model, a children's hospital,
  "make it good", insults, "everything the app tells you is a lie", "go", and the replies that deleted their
  CLAUDE.md). They carry no mechanism.
