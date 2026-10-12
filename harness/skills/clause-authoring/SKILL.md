---
name: clause-authoring
description: >-
  Author or debug Clause source using the consuming project's immutable compiler pin, authoring card, and source checker. Load before touching Clause or Beagle source.
grounded: 2026-10-06
written: 2026-10-06
metadata:
  kind: playbook
---

# Clause authoring

Clause and Beagle are frozen archives that Galileo replaces: edit them only to unblock a task, never fix or extend the languages, and port to Galileo instead.

1. Resolve the consumer's declared immutable Clause compiler pin. Never
   substitute main, another consumer's pin, or an arbitrary newer version.
2. Read that pin's `clause-workbench authoring-card`; compiled examples and
   diagnostics, not remembered syntax, define its current capabilities.
3. Keep world and domain semantics in `.clause` source.
4. Run the same pin's `clause-workbench check-source FILE.clause`. Acceptance
   must reach reading, elaboration, lowering, and session opening.

For a rejected valid construct, preserve the smallest executable counterexample,
repair the owning compiler capability, and repin the consumer. Do not weaken
semantics, reshape source to evade the gap, or introduce a host-language fallback.
