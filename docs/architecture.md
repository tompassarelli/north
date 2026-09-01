# North architecture

North is one primary TUI for directing coding work, conversations, explicit
delegation, goals, and recurring work. This repository is a clean
reconstruction: it preserves North-v1's intended product and strongest
invariants without porting North-v1's implementation.

North is also Clause's primary non-game systems application. It must exercise
and accelerate Clause toward general-purpose use rather than merely consult a
small Clause state machine from an otherwise Rust-owned application.

`north-v2:agent-machinery/` is North's sole source for provider-independent
delegation contracts, run design, role templates, model/effort selection, and
reusable agent procedures. Keeping that module provider-independent does not
justify a second repository or a second live source. Direct Codex and other
consumers receive projections from this package; neither North-v1 nor the
retired standalone checkout is runtime authority.

## Owning boundary

```text
terminal input
    -> minimal Rust host
    -> Clause-owned North transition
    -> authorized foreign effect
    -> Codex app-server over stdio JSONL
    -> Codex native coding tools
    -> effect receipt
    -> Clause-owned settlement transition
    -> Rust projection in the North TUI
```

Clause owns North semantics: conversation and lifecycle states, authority,
routing, delegation decisions, effect intent and settlement, resumability,
goals, recurrence, and intervention policy. Rust owns irreducible mechanics:
terminal I/O, rendering, processes, signals, JSONL transport, and foreign
storage adapters. Codex app-server owns provider communication,
authentication, coding threads and turns, native tools, sandboxing, and
streamed events.

The host may retain opaque foreign payloads, such as prompt text that the
current Clause executable slice cannot represent. A successful Clause
transition must authorize dispatch before Rust sends such a payload. Rust
must not silently become the semantic authority for state that Clause cannot
yet express.

## Clause adoption ratchet

The target is a Clause-authored North above explicit operating-system and
foreign boundaries. Rust is bootstrap and passive host machinery, not North's
application language.

- Every North-specific decision, transition, data model, policy, and rendering
  choice belongs in Clause.
- When an accepted North journey exposes a missing Clause value, control-flow,
  lifetime, effect, concurrency, FFI, or tooling capability, preserve that
  journey as the executable counterexample, repair the smallest reusable Clause
  capability, repin North to it, and resume the same journey.
- Do not encode the missing capability as new North-specific Rust semantics.
- Rust may perform only generic mechanics whose choices were already made by
  Clause, plus explicitly named operating-system or foreign calls. Those
  adapters must remain reusable and ignorant of North policy.
- Each vertical journey must leave the Clause-owned application surface at
  least as large as it found it. A feature is not complete when changing its
  behavior still requires a North-specific Rust edit above the foreign
  boundary.

J1 is useful bootstrap evidence, not Clause-purity evidence. Its Rust host still
contains application-shaped control and protocol handling that must retreat as
Clause gains the required systems surface. North supplies the pressure and the
acceptance journeys for that retreat; Clause development is therefore on the
North delivery path when a journey proves the missing capability.

## Adopt, reject, defer

Adopt now:

- North-v1's product contract and delivery invariants, not its source graph.
- Clause's checked resident source workbench for admitted North state changes.
- Codex app-server's stable stdio protocol for direct coding turns.
- A single direct implementer path for ordinary text.
- Prompt, working directory, authority, mutation, reply, exit, and relaunch as
  one operator journey.

Adopt after the direct journey works:

- DIRGE's central deterministic intervention arbiter.
- At most one intervention at a safe boundary.
- Traces for selected and declined actions.
- Bounded repeated-failure response and explicit progress vocabulary.
- Deterministic continuation and recovery projections.

Reject:

- Any retired North-v1 subsystem name or compatibility concept.
- A tool-server protocol as a prerequisite for ordinary coding text.
- A read-only director and child spawn as the path to an atomic coding task.
- A provider SDK or a second agent loop inside North.
- SQLite, Store, or Rust-owned structs as a second semantic authority.
- Arbitrary lifecycle plugins, stacked critics, invisible model escalation,
  prose tool-call recovery, or model-authored summaries as truth.
- Porting North-v1 modules merely because they exist.

Defer until a journey consumes them:

- richer explicit-delegation surfaces beyond the accepted native route;
- persisted conversation selection and replay;
- images, goals, and recurring work;
- generalized execution governance and schema-aware tool repair;
- plugin runtimes, PTYs, tree-sitter, and provider libraries.

## Vertical journeys

1. Direct coding: launch, submit ordinary text, perform one exact repository
   mutation, show the exact final answer, quit, and relaunch.
2. Explicit delegation: deliberately invoke a coordinator with bounded child
   authority and settle every child.
3. Conversation control: create, switch, quit, relaunch, and replay without
   cross-contamination.
4. Image input.
5. Goal control.
6. Recurring work.

Every journey ends at the normal `north` command. A component or worktree pass
is evidence about a boundary, not a product milestone.

## Prior-art boundary

- Clause `072290e83484e826bd37a033a00849291568e4e9`, MIT OR Apache-2.0:
  executable semantic authority and resident source transitions.
- Codex `61a44880a85d2fd0d8770908dea5733495e571c8`, Apache-2.0, plus the
  [official app-server documentation](https://learn.chatgpt.com/docs/app-server):
  process/TUI separation and the stdio JSONL protocol.
- DIRGE `37785707f3dd3543f76dc5125fd8f331ca1c0813`, GPL-3.0-only:
  conceptual prior art only; no implementation is copied or adapted.
- North-v1 `f2b11f49f2ba655a2ce3ba73acc9bfe6170b6123`, operator-owned reference:
  product intent, failure evidence, and invariants only.
- Agent Machinery `672ea2f0cfe6c6323423fe7e2a89e6789435ced5`, MIT OR Apache-2.0:
  imported as first-party source into `north-v2:agent-machinery/`; its
  standalone checkout is migration evidence, not live authority.

The read-only checkouts live at `~/code/resources/clause` when materialized,
`~/code/resources/codex`, `~/code/resources/dirge`, and
`~/code/resources/north-v1`. The active Clause dependency is pinned by Git
revision in `Cargo.toml`; a mutable resource checkout is never build authority.
