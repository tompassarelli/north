# Structure and code-health

Keep the code workable as it grows: modules with one responsibility, callers
that go through interfaces, names that say what things are, and size and
complexity limits on the code that changes most. The repo's `structure.toml`
holds the limits and the module map; the `code-health` tool measures against
it. Where files live comes first, below.

## File layout

Trace the real sources, producers, consumers, retained state and rerun command
before choosing directories, because a layout built from guesses misplaces
authority. Add a directory boundary only where authority, lifecycle, retrieval
or retention differs, since extra folders cost lookups without telling a reader
anything. Keep the language's and build tool's native layout, because tools and
newcomers expect it.

For data pipelines, keep immutable inputs or their external identities and the
transforms that produced each result, so a result can be traced and rerun. Keep
a checkpoint only when it saves an expensive rerun or serves inspection,
recovery or handoff; other intermediates are disposable. Ordinary software
needs no data-stage or experiment folders.

Name files by the repository's conventions and put a date, sequence or subject
first only when it is how people retrieve them, because names are the index.
Move every consumer, script, build rule, test and document in the same change
as the files, so nothing points at the old place.

A handoff layout is done when the native workflow runs and a newcomer can find
the source, inputs, outputs, disposable work and the rerun command. Data
provenance, checkpoint and naming detail is in the
[project layout notes](../../repo-safety/references/project-layout.md).

## Modules and responsibilities

Each `[[module]]` in `structure.toml` names its paths, a `responsibility` of one
line, and `may_use`, the modules it may import. The `may_use` edges form a DAG.
A file belongs to the first module whose glob matches; a new hand-written
source file must fall in some module.

If a module's responsibility needs "and" between unrelated things, or new code
does not fit any stated responsibility, that is a structural decision: split
the module or add one, and say so in `structure.toml`.

## Interface layers

Callers use a module's public entry (its index, `mod.rs` or header), not its
internals. Imports follow `may_use`; an import against the DAG, or one that
closes a cycle, is a layer problem, not a style one. When a lower module needs
something from a higher one, pass it in (a parameter, callback or data) or move
the shared piece down.

## Naming

Name a thing for what it is or does in the domain: `hitResolution`, not
`helpers2`; `MoveTable`, not `Data`. A file or module named `utils`, `misc`,
`common` or `manager` usually hides several responsibilities. One concept keeps
one name across modules.

## Limits

`[thresholds]` sets `file_lines`, `function_lines`, `function_complexity`
(cyclomatic: 1 plus each branch, loop, case, catch, ternary and run of the same `&&`, `||` or `??` operator) and `hotspot_top`, the files ranked by churn times complexity whose
total complexity may not rise. Read the numbers from the repo; do not carry
another repo's. A `[[module]]` may set an optional inline `thresholds` table
with the same keys, overriding repo defaults for files in the first matching module
(including `hotspot_top`, the rank cutoff for that module's files). Generated files are exempt only when `[[generated]]` names a
producer that exists in the tree; `[ignore]` covers vendored and fixture data.

## Reading a code-health message

`code-health gate BASE HEAD` runs in safe-push and CI and blocks only new debt;
existing debt never blocks. `code-health check FILE` runs after each edit and
returns the same rules for that file as feedback, never blocking. Each line
names the rule, the path and line, the measured value against the limit, and
the fix. The rules:

- cycle: a new import edge that puts two files in one cycle.
- layer: a new edge to a module outside `may_use`, or a hand file in no module.
- file-size: a new hand file over `file_lines`, or one that crossed it. Growing a file already over is the hotspot rule's concern.
- function: a new function over a line or complexity limit, or one that crossed it. Growing one already over is the hotspot rule's concern.
- hotspot: a top hotspot file whose total complexity rose.
- generated: a claimed generated file whose producer is missing.

Exit 2 is a tool error; report it rather than working around it.

## The three fixes

1. Split the file or function along a real responsibility boundary, with the
   behavior check from the main skill.
2. Move the import behind the allowed module's interface: call the public entry
   of a module in `may_use`, or move the shared piece down to a module both may
   use.
3. Change `structure.toml` in the same change when the structure itself should
   change: a new module, a new `may_use` edge, a moved boundary or a different
   limit. It is a structural decision and is reviewed like code; say why in the
   commit.

## Code-health-allow

For file-size, function and hotspot only, a commit trailer
`Code-health-allow: <rule> <path> — <reason>` passes the gate when splitting
now would be worse, for example a table-driven function whose branches are
data. The reason must say why. The report lists every allow as debt for the
recurring review. Cycle, layer and generated cannot be allowed; fix them or
change `structure.toml`.

## The recurring review

An hourly timer runs `code-health signal` per enrolled repo. It fires after
the `[review]` thresholds (commits, hand lines added, days with a landing, or a
hotspot score) since the last review. A fired review starts one worker or lead
that reads `code-health report`, lands behavior-preserving slices against the
top hotspots, allowed debt, cycles and layer violations, then runs
`code-health report --save` and `code-health review-done`. Ordinary work does
not wait for it; fix what your own change touches.
