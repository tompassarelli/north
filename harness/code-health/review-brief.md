Delegation: role=lead budget=1
# Code health review: {{repo}} ({{date}})

Goal: a recurring structure review of {{repo}}, started by `code-health tick` because the review signal fired:

{{reasons}}

Finished means the top refactor items have landed on {{repo}} main as behavior-preserving slices, the baseline
is refreshed and the review marker is recorded. Nobody asked for this review; Tom gets nothing to do.

Read {{checkout}}/AGENTS.md, its `structure.toml`, and the program-craftsmanship, iteration-economics, workers and repo-safety
skills first. Status: append one line per step to your status file beside this brief.

1. Read the report the tick saved at {{report}} (or rerun `code-health report --repo {{checkout}} --rev origin/main`). Write a
   ranked refactor list to {{list}}: cycles and layer violations first, then hotspots (churn x complexity),
   then oversize functions and files, then `Code-health-allow` debt the report lists. Each item names the
   files, the split or move, the test that covers the behavior, and an ETA. Price each item with a
   `Payback:` line per the iteration-economics skill (its code-health review step) and drop items above its staffing
   threshold.
2. Staff the top items as worker boxes per the workers skill (briefs start `Delegation: role=worker budget=0`),
   each in its own worktree off origin/main. Each slice preserves behavior: no feature, tuning or interface
   change for outside callers; when a module boundary moves, update every in-repo caller and `structure.toml`
   in the same change. The repo's existing tests must pass unchanged; never weaken a test or retry a flake.
3. Land each finished slice through the repo's normal path (`safe-push --to main`, or the Autoland queue in an
   Autoland repo) and delete its branch. Stop staffing when the remaining items are minor or the run passes
   4 hours; leave the rest on the list for the next review.
4. When the slices have landed: `git -C {{checkout}} fetch -q origin`, `code-health report --repo {{checkout}} --rev origin/main --save`, then
   `code-health review-done --repo {{checkout}} --rev origin/main`, then remove the lock: `rm -f {{lock}}`.
5. Send the proxy one line: `agents event LANDED code-health-{{repo}} --from-node $AGENT_ORG_NAME --to-node proxy
   --repo {{repo}} --body '<n> slices landed (<shas>), <top items left>'`.

Done when: (1) every landed slice's SHA is on {{repo}} origin/main with the repo's gate green;
(2) `code-health signal --repo {{checkout}} --rev origin/main` prints `quiet`; (3) the proxy event is sent.

Not required: new features, metric changes (that is north's code-health tool), Tom-facing notices, paid compute.

Finish: append `FINISHED $AGENT_ORG_NAME` to your status file, run `agents org remove $AGENT_ORG_NAME` and end
the session. If you stop early, still remove the lock so the next signal can start a fresh review.
