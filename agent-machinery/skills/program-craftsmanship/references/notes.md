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

## Port into the target's idioms

A language port is a rewrite whose acceptance is behavioral equivalence, so the
craftsmanship scope is the whole ported unit rather than one friction point.
Mechanical translation is useful scaffolding and a poor deliverable: it carries
the source language's workarounds (manual allocation patterns, positional
setters, value-type emulation, sentinel integers, getters around plain fields)
into a language that does not need them.

Map each source concept to the target construct that expresses it directly:
records and readonly data for value types, discriminated unions for state
machines and tagged variants, literal unions or `as const` tables for
enumerations, optional or `undefined` for absence instead of sentinels, module
functions over data instead of classes used as namespaces, and inference where
the declaration adds nothing. Let the compiler reject what the source checked
at run time.

Some source shapes exist because the runtime requires them, for example
preallocated storage on a hot path of a garbage-collected or embedded runtime,
or exact numeric operations a host compiler would otherwise reorder. Keep those
deliberately, measure where the cost claim matters, and say in one comment
which constraint forces the shape.

Equivalence comes from an oracle the port cannot adjust: kept contract tests,
recorded inputs with per-step state comparison against the source build, and
native checks where the runtime differs from the test host. A port never edits
an expected value to pass; a disagreement is either a port defect or a source
defect, named as such.

Port a test for a contract worth keeping: a reference value, an invariant, a
reproduced defect. Restating the implementation, pinning incidental constants
or duplicating what the differential check already covers makes the result
harder to change without making it safer.

Evidence: requested by the owner on 5 October 2026 during the Smashcraft
Wurst-to-TypeScript port, after a deterministic converter produced compiling
but Java-shaped TypeScript; the owner asked for idiomatic target code with no
source-language cosplay.
