---
name: production-hardening-distilled
description: >-
  Default distilled workflow for establishing a specific operational guarantee
  under realistic failure, load, attack, concurrency, persistence, migration,
  or deployment conditions. Use for explicit hardening or readiness work.
---

# Production hardening

Do not use this skill to discover whether hardening might be useful or to make
ordinary delivery feel production-ready. It activates only for one already
named operational guarantee whose failure mode, exposed boundary, consequence,
and consumer are concrete. Public source, a CLI, a Store, a daemon, a process,
durable local data, or hypothetical future users are not sufficient.

Start from the internally resolved `project-exposure-v1` profile. When no
profile or concrete exposure facts are supplied, silently use the validated
volatile owner-controlled research default; record no profile artifact,
sidecar, or form. Materialize a machine sidecar only at a boundary that needs
it. Harden only the scoped fact-backed boundary it admits. The default admits
no operational hardening: return to the shortest deliverable, artifact, or
rapid prototype unless a production fact or matching explicit action admits
this mechanism. Safety and exact bounded-claim correctness remain binding.
Eligibility is a ceiling, not a todo: the requested artifact must still need the
guarantee, and its result must change the immediate delivery decision.
Name one guarantee, scenario, degradation, hostility, assumptions, durable
state, recovery, and deciding evidence; do not broaden. Trace only its relevant
admission, resources, effects, cancellation, persistence, recovery, and
diagnostics.

Fix the weakest link minimally; preserve recovery and safe missing-config
behavior. Bound retries with timeout and suitable idempotence/deduplication. Use
`planning-distilled` before public/durable contract, security/concurrency, compatibility,
or rollout changes. This grants no credentials, deployment, production writes,
or outside contact.

Use nearest failure-exposing `verification-distilled`; happy path is not resilience.
Report guarantee, scenario, containment, assumptions, and risk. Stop when proved
or blocked on authority/design.

For the assurance worksheet and expanded failure-path checklist, run
`agents path production-hardening-reference` and read its `SKILL.md` completely.
