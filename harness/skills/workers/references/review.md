# Review

Read this before you land a lane, staff a reviewer or a planner, or weigh review findings.

- No landing carries a review trailer.
- Staff one fresh Claude reviewer of a different tier (Opus for a Sonnet author, Sonnet for an Opus author) only for a planner's design before staffing it, trust-boundary code (cross-host authority: peer-host rows, credentials, rulesets, runtime pull or deploy, fleet admission), or a `Spec: judged` cross-module or cross-repo lane, because unforced reviews there find real defects and forced ones found none; fix its findings in the lane before `safe-push`.
- Each code-review finding carries the command it ran and the output it got, against the reviewed sha; the lead reruns that command and acts only on a finding the rerun reproduces. Plan and design reviews are exempt. Why: `docs/rationale/review-triggers.md` (run-proven findings).
- Each reviewer report ends `Accepted flaws: N`; note a trigger that keeps yielding 0 in one line for pruning. Read [planner effort](notes.md#planner-effort) before staffing `planner`, and [review lenses](notes.md#review-lenses-and-findings) before staffing several reviewers or weighing findings.
