# Verification: notes

Detail behind `SKILL.md`. Read a section only when its question comes up.

## Reference grounding

When a project picks or revisits its test posture, read [domain profiles](testing-domains.md), use landing throughput to find 2-3 high-throughput projects in the domain, newer and AI-heavy first, study their test and landing architecture and accepted trade-offs, and record how we could get there; numbers only select.

## Landing-tool trailers and CI budget

`safe-push` enforces these, so a refusal names the missing trailer.

- The landing tool refuses an added `skip`, `only`, `#[ignore]` or `xfail` without a `Test-change:` trailer naming the changed requirement, and an added test without a `Test-kind: <name> <kind> kills <mutant>` trailer.
- A CI budget may name a ceiling after its target; a CI wall median between them lands and is logged as debt. While the median exceeds the ceiling, or the target when none is set, the landing tool refuses a landing unless a speed-up carries `CI-budget: <workflow> <before>s -> <after>s` or, while main is red, every commit names the failing run in `Main-red: <run id>`.
