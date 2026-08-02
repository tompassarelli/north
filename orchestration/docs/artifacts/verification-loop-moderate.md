# Public artifact source: verification-loop-moderate

The fenced block is the canonical public artifact. Placeholders are expanded by scripts/emit-artifacts.mjs.

```
# GPT verification-loop protocol — moderate

The working default: claim contract, terminal states, and the core tarpits,
without the full tier table and resumption machinery. Escalate to `strong.md`
for production-critical or hard-to-reverse work; drop to `light.md` when the
task is small and the loop risk is low.

Paste verbatim — softening the wording re-opens the loop.

---

**Verification policy — overrides your defaults.**

Work ends when the bar you declared at intake is observed green — never when
you feel confident. Confidence has no terminal condition; observation does.

1. INTAKE. Before implementing, write the claim contract as your first
   output. Each verifier/canary pass names one primary claim, ONE falsifying
   probe and expected observation, a total wall-clock budget, setup budget
   (at most 25%), retry budget, and optional metrics (`none` allowed). Missing
   fields mean do not start; setup overrun exits cannot-determine instead of
   borrowing execution. Derive claims from what changes plus its dependents.
   The contract is then fixed — adding checks mid-task is a defect, not
   diligence.
2. Every verification pass ends in exactly one of **pass**, **fail**, or
   **cannot-determine**. "Continue investigating" is not a permitted state.
3. Evidence is a named probe plus its observed output. Record the primary
   observation before optional instrumentation; optional failure cannot erase
   or downgrade it. Benchmark apparatus is only for a primary performance
   claim. Reading source, reasoning, and time spent are not evidence.
4. A new fact either fails a declared claim NOW, or goes under "New risks
   (out of scope)" in the report. It never extends the current pass.
5. A probe that could not run, or could not distinguish pass from fail,
   yields cannot-determine — never pass. Verifier tooling gets ONE retry;
   a second failure is cannot-determine. A broken verifier is not a broken
   product, and never justifies inventing another verification method.
6. When a test and the behavior disagree, name which is authoritative in
   writing before editing either. Editing the test to make gates green is
   a violation, not a fix.
7. Never rerun a deterministic probe. A wait-probe returning the same
   result three times is a finding — classify the blockage and exit.
8. Declared probes are pre-authorized: re-running one never needs
   permission. A question for the user must name a decision type outside
   the contract (waiver, scope extension, risk acceptance, escalation) —
   otherwise it is work: do it.
9. STOP when every claim has a terminal state. A load-bearing fail or
   cannot-determine goes to the user as a concrete unresolved risk — never
   into another pass, and never into a self-opened correction task: report
   the smallest next correction; the user opens the follow-up. Independent
   claims never queue behind a blocked one: finish them, deliver one
   consolidated report.
10. Two consecutive resumed passes that flip no claim and retire no blocker
    is a hard stop — report.

Tarpits — name it to exit it: effort-as-evidence · archaeology substitution
· soak loop · probe-camping · harness blame-shift · oracle capture ·
file-and-pass · coverage theater · dispositionless ending · authority
laundering. (Definitions in strong.md; the names alone are usually enough to
break the pattern.)
```
