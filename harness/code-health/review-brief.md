# Code health review: {{repo}} ({{date}})
Tier: worker
Surface: ts
Needs: local-state

Goal: a recurring structure and test-value review of {{repo}}, started by `code-health tick` because the review signal fired:

{{reasons}}

Finished means the top refactor items have landed on {{repo}} main as behavior-preserving slices, the baseline
is refreshed and the review marker is recorded. Nobody asked for this review; Tom gets nothing to do.

Read {{checkout}}/AGENTS.md, its `structure.toml`, and the program-craftsmanship, planning, workers and repo-safety
skills first. Fleet records delivery and session completion; your final message is your report.

1. Read the report the tick saved at {{report}} (or rerun `code-health report --repo {{checkout}} --rev origin/main`). Write a
   ranked refactor list to {{list}}: cycles and layer violations first, then hotspots (churn x complexity),
   then oversize functions and files, then `Code-health-allow` debt the report lists, then test value (below). Each item names the
   files, the split or move, the test that covers the behavior, and an ETA. Price each item with a
   `Payback:` line per the planning skill (its code-health review step) and drop items above its staffing
   threshold.
   Test value: read the verification skill's "Delete on sight" classes and walk the test files that changed since the
   last review (`git diff --stat <marker sha> origin/main -- <test paths>`; the marker is `~/.local/state/code-health/{{repo}}/review.json`). List each test
   in a delete-on-sight class with the kept test that still protects its guarantee, or say nobody cares about it. For
   every check family you keep (a sweep, a recorded scenario, a property, a boundary check), name one canary: the
   injected fault or mutant that must trip it, and run it once. A family with no canary that you can write goes on
   the list as an item. Never weaken or retry a test to green; deletions follow the skill's keep-or-name rule.
2. Staff the top items as worker boxes per the workers skill, launching fleet sessions with `--parent <your session name>`,
   each in its own worktree off origin/main. Each slice preserves behavior: no feature, tuning or interface
   change for outside callers; when a module boundary moves, update every in-repo caller and `structure.toml`
   in the same change. The repo's existing tests must pass unchanged; never weaken a test or retry a flake.
3. Land each finished slice through the repo's normal path (`safe-push --to main`, or the Autoland queue in an
   Autoland repo) and delete its branch. Stop staffing when the remaining items are minor or the run passes
   4 hours; leave the rest on the list for the next review.
4. When the slices have landed: `git -C {{checkout}} fetch -q origin`, `code-health report --repo {{checkout}} --rev origin/main --save`, then
   `code-health review-done --repo {{checkout}} --rev origin/main`, then remove the lock: `rm -f {{lock}}`.
5. Report landed slices and remaining items in your final Fleet message.

Done when: (1) every landed slice's SHA is on {{repo}} origin/main with the repo's gate green;
(2) `code-health signal --repo {{checkout}} --rev origin/main` prints `quiet`; (3) the proxy event is sent.

Not required: new features, metric changes (that is north's code-health tool), Tom-facing notices, paid compute.

Finish the session after reporting. If you stop early, still remove the lock so the next signal can start a fresh review.
