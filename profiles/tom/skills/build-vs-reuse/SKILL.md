---
name: build-vs-reuse
description: >-
  Choose what a product should own versus reuse at its differentiation
  boundary. Use when a consequential design asks whether to build, adopt,
  extend, fork, or replace a library or platform capability, especially when
  custom infrastructure is proposed for a commodity concern.
---

# Build versus reuse

Minimize custom ownership outside the product's differentiation boundary
without outsourcing the reason the product exists.

This heuristic owns product and architecture boundary choices. It does not own
the ordinary YAGNI, repository-pattern, standard-library, platform, existing-
dependency, then smallest-code ladder; the always-loaded profile applies that
rule to routine implementation. Use `prior-art` when precedent must be
researched rather than assumed.

## Establish the boundary

Identify:

- the capability the product must deliver;
- the semantics or control that differentiate the product;
- commodity capabilities that merely support it;
- protected surfaces a reused component must preserve; and
- integration, operating, licensing, and exit constraints.

Apply a reuse preference when mature capabilities exist outside the
differentiation boundary and their constraints are acceptable. Reduce or
disable it when the abstraction itself is being researched, semantic control
is central to the objective, or performance, security, portability,
determinism, dependency independence, or replacement cost makes adoption the
larger owned surface.

Never let this heuristic override policy, explicit operator direction,
protected data or contracts, the declared project thesis, or evidence from
`prior-art`.

## Compare ownership choices

1. Search the current system and authorized precedent for capabilities outside
   the differentiation boundary.
2. Compare use, configuration, composition, extension, fork, focused
   replacement, and deliberate invention without treating them as a moral
   ladder.
3. Account for integration complexity, long-term maintenance, operational
   risk, licensing, portability, lock-in, and exit cost.
4. Choose the smallest owned surface that preserves the thesis and protected
   surfaces.
5. Keep custom work independently replaceable where practical.

Before adapting or copying an external implementation, use `external-code` to
establish its license and attribution boundary.

For material custom work outside the differentiation boundary, require:

```text
Needed capability:
Existing approaches considered:
Requirement they fail:
Smallest custom surface:
Replacement boundary:
Validation:
```

## Make the decision observable

Return a compact ownership decision:

```text
Differentiating semantics:
Commodity capabilities to reuse:
Owned custom surface:
Why existing approaches stop short:
Dependency and exit risks:
Falsifying evidence:
```

Do not interpret reuse as always choosing a library, minimizing line count at
any cost, accepting a poorly matched framework, extending indefinitely when a
focused replacement is smaller, or outsourcing strategically important
semantics.

Minimize custom ownership outside the reason the product exists, not inside it.
