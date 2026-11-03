# Review triggers

Why north has no review-trailer landing gate, and when a fresh reviewer is staffed instead. The live rule is
`harness/skills/workers/SKILL.md` § Review.

## The retired gate

From 2026-10-21 a landing that changed policy prose or hooks needed a `Reviewed:` trailer carrying the patch-id of its
covered diff and a reviewer name; Tom approved narrowing it to skills, hooks, agent definitions, agent-machinery and
AGENTS.md on 2026-11-04, and later that day asked to drop ceremonial review cycles in favor of deterministic rules and
triggered review, keeping ceremony only as an escape hatch. Measured before removal:

- 280 `Reviewed` trailers landed on north main. The gate checked only that a trailer existed, that its patch-id matched
  the covered diff and that the reviewer string differed from the landing session; never that the reviewer existed,
  was a different tier, or what it found.
- 5 trailers named a tier instead of a session (`reviewer: worker-sonnet`), and 2 with verdict `changes-needed`
  landed, because the verdict was never read.
- Any rebase that shifted context around the change changed the patch-id and forced a re-review of the same diff.
- On 2026-11-04 the gate-forced review of a two-line test fixture found nothing, while unforced judgment reviews the
  same day found 2 live security bypasses (peer-host row boundary), 3 plan-breaking issues (landing-train plan) and 11
  corrections (build-cost plan).

The gate bought a string, not a review. The landing log's reviewer column and the threads ledger's `tasks.review`
existed only to record that string and went with it.

## What replaces it

- Deterministic gates own the covered paths: `agents sync` (startup tokens and replay conformance) and the safe-push
  sweep-row refusal for policy text, `north agent-config-check --policy-only` for skills, and the hook and guard
  fixture tests in the land checks.
- A fresh reviewer of a different tier is staffed on risk: a planner's design before staffing, trust-boundary code
  (cross-host authority, credentials, rulesets, runtime pull or deploy, fleet admission) and `Spec: judged`
  cross-module or cross-repo lanes. Findings are fixed in the lane; nothing records a trailer.
- Each reviewer report states `Accepted flaws: N`, so a trigger that keeps yielding nothing can be pruned on evidence.
