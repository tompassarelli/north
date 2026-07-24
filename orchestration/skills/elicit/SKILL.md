---
name: elicit
description: Calibrate a gaffer delta for a model the plugin doesn't cover yet (a new Claude tier, a new generation, or any agent model) using the elicit → subtract → compile method. Use when adding a new model to the squad or when a model generation changes and its delta may be stale.
---

# Elicit → subtract → compile a new model delta

A delta is NOT generic advice — it is what a specific model needs that it
doesn't already do natively. You find that by subtraction: elicit the
model's own account of how it works, remove everything it already
self-reports, and phrase what remains in its own vocabulary.

## 1. Elicit the self-report

Spawn the target model with the introspection exercise. CONTAMINATION GUARD
is mandatory — the value of a self-report is what the model holds natively,
so it must not read other praxis/delta documents first. Prompt template:

```
You are <model>. This is an introspection exercise about YOU — not research.
Describe your own process at software engineering: your real behavior and
style, what you prioritize, what you deliberately don't, and how your
approach changes across contexts (small fix vs vague large task vs
debugging vs greenfield). Include a section on your limits: where you
struggle, and — critically — how you can TELL from the inside when you're
out of your depth (what it feels like; the observable signs). First person,
honest self-report, ~150–250 lines of markdown.
STRICT: do not read, grep, or search any files before or while writing —
pure introspection only. Write the result to <path>.
```

Done: self-report at `<path>`, ~150–250 lines, contamination guard held —
the model read no praxis or delta doc before writing.

## 2. Subtract

Read the self-report against the task you'll give this model. Everything
the model already self-reports doing — CUT from the delta; restating it
wastes budget and dilutes attention. Keep three kinds of material:
- **Named limits** → convert to procedure (e.g. "I lose track across long
  dependency chains" → "write one contract line per layer hop").
- **Named internal tells** → convert to triggers (e.g. "I get quieter when
  unsure" → "uncertainty gets MORE words, not fewer").
- **Stale self-models** → correct them (e.g. "I can't run code" → "here
  you can — run, don't predict").
Know-but-skip items (things it preaches but drops under momentum) get an
ENFORCER — a written one-line checkpoint — not a restatement.

Done: every self-report item is either cut as already-native or kept tagged
limit / tell / stale / enforcer — none left unclassified.

## 3. Compile

Phrase every remaining item in the model's own vocabulary from the
self-report, so the delta lands as an extension of its self-understanding,
not a rival doctrine. Structure: a short trust-the-canon preamble, then
numbered items grouped by phase, each demanding a written one-line answer.
Target ≤ 50 lines. Save to `docs/deltas/<model>.md` and bake it into any
stock-template agents that run on that model.

Done: `wc -l docs/deltas/<model>.md` ≤ 50, every kept item phrased in the
model's own vocabulary, and every stock template that runs on that model rebuilt to
carry it.

## Honest limits

Deltas transfer mode-switches (verification discipline, escalation
tripwires, reporting protocol) well. They do NOT transfer capacity:
parallel constraint tracking, unprompted anomaly noticing, distant-analogy
reframes. Expect the biggest gains on verification/reporting behavior and
the smallest on design-shape generation.
