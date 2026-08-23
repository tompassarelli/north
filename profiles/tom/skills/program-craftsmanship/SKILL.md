---
name: program-craftsmanship
description: >-
  Run a bounded behavior-preserving maintenance pass over mature or high-value
  code. Use when the operator requests craftsmanship, cleanup, or refactoring
  of semantics, naming, authority, abstractions, modularity, APIs, errors,
  tests, or local structure without changing public, persisted, security, or
  operational behavior.
---

# Program craftsmanship

Reduce semantic ambiguity, cognitive load, accidental coupling, and future
change cost while preserving observable behavior. Do not turn a maintenance
request into a redesign.

## Bound the pass

Before editing:

1. Name one semantic scope and its protected surfaces.
2. Read its repository instructions, architecture constraints, source, tests,
   and only the recent history needed to understand a concrete concern.
3. Use `verification` to select the nearest existing behavioral baseline that
   can detect unintended change. Do not manufacture a broad gate merely to
   justify cleanup.
4. Prefer scopes with demonstrated friction, expected change pressure, or
   meaningful blast radius. Do not polish cold code for aesthetic satisfaction.

If the repository already records maintenance freshness, use that record. Do
not create a cross-project ledger, one sidecar per file, or an attestation
system merely to run this pass.

## Inspect in semantic order

1. **Conceptual model and vocabulary** — one concept per name and one name per
   concept; names express semantic roles rather than implementation accidents.
2. **Authority and lifecycle** — clear ownership, source of truth, state
   transitions, effects, and invalidation.
3. **Abstractions** — collapse proven concepts and repeated change patterns;
   remove speculative genericity, aliases, and needless indirection.
4. **Cohesion and coupling** — colocate responsibilities that change together,
   separate those that evolve independently, and reveal hidden coordination.
5. **Module and file structure** — make structure reflect the discovered model
   without reorganizing files for its own sake.
6. **APIs, errors, state, and control flow** — simplify private contracts,
   propagate errors deliberately, reduce mutation and nesting, and remove
   impossible states.
7. **Tests, comments, and documentation** — express guarantees and constraints;
   remove narration that merely restates code.
8. **Mechanical polish** — formatting, imports, proven dead code, deterministic
   private renames, and compiler-supported simplification.

## Choose the permitted change class

Act automatically on mechanical work whose value and equivalence are clear.
Make local structural changes only when the scope is bounded and behavior is
strongly covered. This includes extracting or inlining functions, reducing
nesting, consolidating proven duplication, clarifying private types or errors,
and splitting or merging private modules.

Do not smuggle public API redesign, persisted representation changes,
compatibility policy, authority relocation, concurrency, security, durability,
deployment behavior, or broad vocabulary migration into the pass. Route those
to `planning` or `production-hardening` as appropriate.

Never rewrite for taste, generalize from hypothetical future use, replace
understandable duplication with a weaker abstraction, mix unrelated changes,
or weaken a test or diagnostic. Never hand-edit generated or vendored code;
change the owner source and use its generator or upstream mechanism. For
Beagle source, use `beagle-authoring`.

## Execute and prove

Work in small coherent batches and keep behavior-preserving transformations
separate from incidental bug fixes. Use repository-native formatters,
typecheckers, tests, and output comparisons selected by `verification`; do not
substitute a generic command list or repeat a check after it has decided the
claim. Inspect the final diff for accidental semantic change and revert changes
whose value is marginal or whose equivalence cannot be supported.

Record durable residual debt only in an existing repository or continuity
mechanism with a concrete reconsideration trigger. The commit message and final
handoff are sufficient when no such record exists.

Stop when no high-value bounded change remains, the next step is speculative,
validation is insufficient, a protected surface would change, or further
polish has diminishing return.
