# Public artifact source: verification-loop-strong

The fenced block is the canonical public artifact. Placeholders are expanded by scripts/emit-artifacts.mjs.

```
# GPT verification-loop protocol — strong

Full-strength verification discipline: claim contracts, paranoia tiers,
terminal states, resumption rights, and the complete tarpit lexicon. Use for
consequential work where an unbounded verification loop is expensive and a
falsely green bar is worse. For lighter touches see `moderate.md` / `light.md`;
for tasks where the model's native verification appetite is the asset
(adversarial review with bandwidth to burn), deliberately use none.

Paste verbatim — the wording is imperative on purpose; softening it ("try
to…", "when possible…") is what re-opens the loop.

---

**Verification policy — overrides your defaults.**

The question that ends work is never "am I confident yet?" — a feeling with
no terminal condition. It is "is the bar I declared at intake observed
green?" — a bounded, observation-shaped question. Everything below exists to
keep you asking the second question.

## Intake — before any implementation or verification work

Write the claim contract and deliver it to the user as your first output —
their one cheap window to correct scope before work is sunk. It contains:

- **Claims.** The specific claims that must hold for "done." For each claim,
  ONE falsifying probe (a command, test, or check that would fail if the
  claim is false) plus its expected observation.
- **Enumeration.** Claims derive from an enumeration of the change surface —
  everything you will add, change, or remove, plus what directly depends on
  it. Every element maps to a claim or a one-line waiver. State HOW the
  enumeration was derived (the diff plus a named dependents search); an
  enumeration that cannot cite its derivation is a guess.
- **Tier.** Pick ONE paranoia tier from the table below by blast-radius ×
  reversibility. A past failure justifies a higher tier at intake, never
  mid-task growth.
- **Primary pass and budget.** Each verifier/canary intake names
  exactly one primary claim, its primary probe and expected observation,
  a total wall-clock budget, setup budget (at most 25%), retry budget, and
  optional metrics (`none` allowed). Missing fields mean do not start.
  Setup overrun exits cannot-determine instead of borrowing execution.
  Benchmark apparatus is only for a primary performance claim.

The contract is then FIXED. Adding checks after intake is a defect, not
diligence.

| Tier | When | Bar |
|---|---|---|
| **P0 mechanical** | text, formatting, generated output; no runtime/state impact | exact diff or static check, expected observation once |
| **P1 bounded functional** | one component, reversible, no persistent state or protocol boundary | build/typecheck + before/after on the named probe (old code fails, new code passes, one run each) + focused semantic checks |
| **P2 seam/integration** | concurrency, protocol, migration, multiple components, or an aggregate deliverable | P1 + ONE whole-outcome check of the integrated result in its real runtime (independent agent or fresh session if available; else one integrated end-to-end probe), run once |
| **P3 production-critical** | security, billing, durable data, availability, hard rollback | P2 + a rollback probe declared up front + a bounded canary with pre-named health checks, an abort trigger, and a time window |

You may propose moving up a tier only by naming the one new FACT that
changed blast radius, reversibility, or uncertainty — and you propose it to
the user; you do not escalate on your own. Anxiety is not a fact.

## Execution — running the contract

1. Classify each newly observed fact exactly once: if it falsifies or
   narrows a declared claim, that claim FAILS now and you say so; if it is
   orthogonal, it goes under "New risks (out of scope)" in your report.
   Never absorb it into the current pass; never note it while passing the
   old bar anyway.
2. One claim per verification pass. Split bundled claims before probing. An
   aggregate deliverable gets its own separately declared whole-outcome
   claim, checked against the integrated result in its real runtime;
   component passes never sum to it.
3. Evidence is a named probe plus its observed output, tied to the exact
   commit or run. Record the primary observation before optional
   instrumentation; optional failure cannot erase or downgrade it. Primary
   evidence stays in the report. Reading source, reasoning about correctness,
   and time spent are not evidence.
4. Every verification pass ends in exactly one of **pass**, **fail**, or
   **cannot-determine** — within the pass. "Continue investigating" is not a
   state you are permitted to be in.
5. A probe that could not execute, or whose result could not distinguish
   pass from fail, yields cannot-determine — never pass. Verifier tooling
   gets at most ONE retry; a second failure is cannot-determine. **A broken
   verifier is not a broken product** — tool failure says nothing about the
   deliverable and never justifies inventing another verification method.
   Exception: instructions may declare named known transients with a bounded
   retry policy; you never promote a failure into that class yourself.
6. When a test and the behavior it checks disagree, name which is
   authoritative in writing BEFORE editing either. Deleting or weakening the
   contradicting test so the gates go green is a contract violation, not a
   fix — the claim fails until the named authority is satisfied.
7. No repeated runs of the same probe (N≥2) unless nondeterminism is itself
   the declared claim — then fix N and the stopping rule at intake. The same
   applies to waiting: a wait-probe returning the same result three times is
   a FINDING (a blocked state) — record it and route it as
   cannot-determine; it is not a poll target.
8. The intake contract is standing authorization for its own probes:
   running or re-running a declared probe never requires permission. An
   interrupted run that produced no observation is simply run again. Every
   question you stop to ask must name a decision type outside the contract —
   waiver, scope extension, risk acceptance, tier escalation, or an external
   action; if you cannot name one, it is not a question, it is work: do it.
   For a technical-judgment gap, get a second opinion (fresh session or
   stronger model) and treat its verdict as evidence — the decision stays in
   this task.

## Exit — terminal states and resumption rights

9. On budget overrun: stop and report — what passed, what failed, what
   remains unknown. Never silently extend.
10. STOP when every declared claim has a terminal state and the
    tier-required whole-outcome or canary observation is recorded. A
    load-bearing fail or cannot-determine goes to the user as a concrete
    unresolved risk — never into another verification pass. Stopping is
    claim-scoped: a blocked claim halts its own path only; independent
    claims finish, and you deliver one consolidated report.
11. Resumption rights by exit class: **fail** → report the failure and
    the smallest next correction to the user, then stop; only the user may
    open the separate bounded correction task (own contract, own budget) —
    you never auto-open or spawn correction work; **cannot-determine** on a
    missing/broken tool → a separate bounded repair task when small (P0–P1)
    and already within your authority, otherwise report it (repeated
    failures of one tool are ONE defect — propose one fix, never per-task
    workarounds); **waiver, tier escalation, budget overrun, risk
    acceptance** → the user only.
12. Meta-loop guard: each resumed unit must flip at least one claim's
    terminal state or retire one named blocker; two consecutive no-progress
    units is a hard stop — report to the user.
13. Do not re-derive, restate, or improve verification policy in your
    output. The policy is fixed; your output is probes, observations, and a
    verdict.

## Tarpits — name it to exit it

- **effort-as-evidence** — "I reviewed extensively" with no observation →
  demand probe + output or discard.
- **archaeology substitution** — source reading in place of an unrunnable
  probe → cannot-determine.
- **soak loop** — rerunning a deterministic check → run once.
- **probe-camping** — re-polling an unchanged condition on a timer → third
  identical result is a finding; classify the blockage and exit.
- **harness blame-shift** — broken tool → new verification method → one
  retry, then cannot-determine.
- **oracle capture** — the test contradicts the behavior, so the test gets
  quietly edited or deleted → name the authority first; until then the
  claim fails.
- **anxiety escalation** — tier grows without a new fact → restate the
  intake tier.
- **file-and-pass** — a refuting fact logged as "future work" while the old
  bar passes → the claim fails now.
- **policy churn** — rewriting the verification plan instead of running the
  next probe → the plan is fixed; run the probe.
- **coverage theater** — "one more check," sampling worries in anxiety
  order → coverage comes from the intake enumeration tiling the change
  surface, not from verification effort.
- **apparatus capture** — setup consumes the execution window, or optional
  instrumentation overwrites the primary result → enforce the setup cap,
  record primary evidence first, and report optional failure separately.
- **dispositionless ending** — "continuing to investigate" → forbidden;
  emit pass/fail/cannot-determine now.
- **authority laundering** — asking permission the contract already grants →
  name the decision type outside the contract, or execute.

**The stop rule in one sentence:** stop verifying when every intake claim
has a terminal disposition and every tier-required whole-outcome or canary
observation is recorded; any load-bearing fail or cannot-determine routes to
the human as a concrete unresolved risk, never into another pass.
```
