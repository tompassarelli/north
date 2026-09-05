---
name: program-craftsmanship-reference
description: Full notes on behavior-preserving cleanup, friction diagnosis, and refactor scope.
---

# Craftsmanship: full notes

## Aim at the next correct change

Improve established code without changing observable behavior. Invest where
current work, repeated edits, defects, or meaningful coupling demonstrate
friction. Inactive code that could look nicer is not automatically on the path.

## Diagnose the friction

Useful signals include one concept with several names, several concepts hidden
behind one name, unclear state/effect/lifecycle ownership, implicit ordering,
duplicated decisions, and abstractions that hide rather than explain behavior.
Also inspect APIs permitting invalid states or discarding errors, tests hiding
their contract, and comments contradicting executable structure.

Trace a concrete change through the code before prescribing extraction or
indirection. “Too many lines” is not by itself a semantic diagnosis.

## Choose a bounded transformation

Mechanical renames, proven dead-code removal, formatting, and compiler-supported
simplification usually offer a clear equivalence argument. Extraction,
inlining, type refinement, error cleanup, and module reshaping need an evident
reduction in complexity plus coverage of the affected behavior.

A private simplification can be worthwhile without a general framework.
Conversely, a repeated ownership decision may justify one shared authority even
when the resulting code is not shorter.

## Checks and limits

Use the existing relevant behavior check and inspect the diff for accidental
API, ordering, error, resource-lifetime, or dependency changes. Those are not
cosmetic. If new behavior is required, name it separately rather than smuggling
it into a cleanup.

Stop when the next correct change is easier and behavior remains covered.
Do not use craftsmanship as a route to unrequested product redesign.
