---
name: verification-distilled
description: >-
  Default distilled workflow for selecting, running, supervising, and
  interpreting proportionate tests, checks, builds, debugging reproductions,
  CI, release preflight, or performance evidence for claims of readiness.
---

# Verification

Verification requires an action fork. Before a check, name internally:
`pass -> A; fail -> B`. If A and B are the same immediate action, do not run the
check. Evidence is not a second deliverable unless the operator requested it or
a named external boundary requires it. Verification consumes at most one
decision node on the current shortest-path DAG; it never creates a parallel
assurance path.

Start from the internally resolved `project-exposure-v1` profile. When no
profile or concrete exposure facts are supplied, silently use the validated
volatile owner-controlled research default; record no profile artifact,
sidecar, or form. Materialize a machine sidecar only at a boundary that needs
it. Its scoped fact-backed budget controls generalized assurance, release, and
provenance/immutability.
For default research, check only the exact bounded claim needed for the
deliverable, artifact, or rapid prototype and the one decision-changing check.
Safety and exact bounded-claim correctness remain binding.
Name claim, resolved context, and pass/fail action. Skip checks unable to
change action; stop when decided. Price every loop's time, remaining uses,
cheapest optimization, and break-even. Optimize only when it pays, never by
weakening evidence. At 2x
expectation stop, preserve, classify, and reprice before retry/timeout change.
Never retry into proof.

The nearest existing relevant check is the default and is sufficient when it
passes. Do not add an auditor, reviewer, verifier, independent reproduction,
broader suite, attestation, provenance exercise, or second end-to-end journey
to confirm ordinary owner judgment. A passed decision-changing check closes the
decision. Report unobserved dimensions as residual uncertainty; do not turn
them into work. A smoke check is a cheap falsification attempt, never a back
door to broader assurance.

Use the lowest deterministic layer and narrowest profile; never silently
escalate. Control verdict-sensitive inputs only. Require provenance only when
producer substitution changes the decision; identity gaps block only consumers.

Measure directly first. Build a harness only for a named missing observation,
bounded to one planned run; after its second defect/exhausted budget, simplify
or report the gap. A canary or smoke run that repeatedly fails before its
advertised boundary is integration debugging, not release evidence. Its first
failure seeds one bounded failure family across the reachable vertical slice:
find and fix every occurrence of that cause before paying for the next run. One
owner continues `run → failure-family sweep → batch fix → rerun` without a new
approval or handoff for each exception; a genuinely different cause begins the
next wave. Stop only at the requested boundary or a real outside decision. Do
not widen the sweep beyond the delivery path, invent a generic harness, or turn
the wave into broader assurance. Non-product failure stays diagnostic.

A harness or launch-environment failure does not falsify the product. Use an
already-sanctioned direct route when immediately available; otherwise report
the evidence gap. Do not start infrastructure, Store, daemon, subscription,
observability, recovery, or hermetic-sealing work merely to increase confidence
in an artifact whose delivery decision is already resolved.

Batch before expensive checks; one supervisor owns/reaps each run. Reject
reflexive suites, inflated oracles/attestation, excess isolation/compatibility,
mega-journeys, remote-CI waits, reassurance, and post-decision checks. Never
weaken gates.

Define a canary or smoke check from the smallest explicit behavioral slice that
makes the artifact useful, normally the operator's 80/20 or roughly 95% path.
Enumerate that slice and its fixed pass boundary before running; do not redesign
the check around each exception or serially invent more checks after every
repair. Batch independent required probes, and run them concurrently when their
setup and results do not interfere. Prefer the actual requested journey over a
proxy canary once that journey is safely runnable.

After a bounded failure wave, preserve verification tactics that materially
improved the loop: sweeping one cause family before rerun, repairing a generator
that allowed stale projections, or replacing a proxy with the real acceptance
journey. When the operator asks for a durable lesson or repeated evidence makes
the rule stable, route that win through `skill-maintenance-distilled` without
adding another release gate.

For profile/instrument tables, pricing fields, provenance guidance, harness
budgets, and the full anti-theatre catalog, run
`agents path verification-reference` and read its `SKILL.md` completely.
