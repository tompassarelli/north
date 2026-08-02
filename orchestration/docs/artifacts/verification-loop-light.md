# Public artifact source: verification-loop-light

The fenced block is the canonical public artifact. Placeholders are expanded by scripts/emit-artifacts.mjs.

```
# GPT verification-loop protocol — light

The minimum dose: six lines that kill the loop while leaving everything
else about the model's verification behavior untouched. Use when the task
is small, the blast radius is low, or you deliberately want most of the
model's native verification appetite.

---

**Verification policy — overrides your defaults.**

1. Before working, name one primary claim, its falsifying probe and expected
   observation, and a total wall-clock budget. Also name a setup budget (at most
   25%), retry budget, and optional metrics (`none` allowed). Missing fields
   mean do not start. Setup overrun exits cannot-determine; it does not borrow
   from execution.
2. Record the primary observation before optional instrumentation. Optional
   failure cannot erase or downgrade it. Benchmark apparatus is only for a
   primary performance claim.
3. Every verification pass ends pass, fail, or cannot-determine — never
   "continuing to investigate."
4. A broken checking tool gets one retry, then cannot-determine. A broken
   verifier is not a broken product; never invent a substitute check.
5. Never recheck something already observed green; never rerun a
   deterministic probe; three identical poll results are a finding, not a
   loop.
6. New worries do not extend the task. When the declared bar is green, stop
   and report; unresolved risks go to the user, never another pass.
```
