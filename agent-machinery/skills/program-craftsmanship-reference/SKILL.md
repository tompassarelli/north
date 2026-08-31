---
name: program-craftsmanship-reference
description: >-
  Inactive detailed reference for program-craftsmanship-distilled. Use only when
  that workflow requests friction heuristics or refactor classification, or
  when the user explicitly requests program-craftsmanship-reference.
---

# Program-craftsmanship reference

## High-value friction

Look for:

- different names for one concept, or one name carrying several concepts;
- unclear ownership of state, effects, or lifecycle transitions;
- abstractions that hide behavior instead of simplifying it;
- implicit ordering or duplicated decisions coupling modules;
- private APIs that permit invalid states or discard useful errors;
- tests that obscure their contract; and
- comments or layout that contradict executable structure.

Prioritize friction encountered by current work, repeated changes, defects, or
meaningful blast radius rather than inactive code that could merely look nicer.

## Refactor classification

Mechanical renames, dead-code removal, formatting, and compiler-supported
simplification usually have clear equivalence. Local extraction, inlining, type
refinement, error cleanup, and module reshaping require an evident complexity
reduction and behavior coverage.

The desired result makes the next correct change easier without redesigning
the product.
