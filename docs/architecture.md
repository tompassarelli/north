# North architecture

North is one primary TUI for directing coding work, conversations, explicit
delegation, goals, and recurring work. This repository is a clean
reconstruction: it preserves North-v1's intended product and strongest
invariants without porting North-v1's implementation.

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

- `Bridge` as a product, subsystem, role prefix, or compatibility concept.
- MCP as a prerequisite for ordinary coding text.
- A read-only director and child spawn as the path to an atomic coding task.
- A provider SDK or a second agent loop inside North.
- SQLite, Store, or Rust-owned structs as a second semantic authority.
- Arbitrary lifecycle plugins, stacked critics, invisible model escalation,
  prose tool-call recovery, or model-authored summaries as truth.
- Porting North-v1 modules merely because they exist.

Defer until a journey consumes them:

- explicit delegation and any North MCP surface;
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

- Clause `bbb738985fd894152cb181816a6244ea3972c3ed`, MIT OR Apache-2.0:
  executable semantic authority and resident source transitions.
- Codex `61a44880a85d2fd0d8770908dea5733495e571c8`, Apache-2.0, plus the
  [official app-server documentation](https://learn.chatgpt.com/docs/app-server):
  process/TUI separation and the stdio JSONL protocol.
- DIRGE `37785707f3dd3543f76dc5125fd8f331ca1c0813`, GPL-3.0-only:
  conceptual prior art only; no implementation is copied or adapted.
- North-v1 `f2b11f49f2ba655a2ce3ba73acc9bfe6170b6123`, operator-owned reference:
  product intent, failure evidence, and invariants only.

The read-only checkouts live at `~/code/resources/clause` when materialized,
`~/code/resources/codex`, `~/code/resources/dirge`, and
`~/code/resources/north-v1`. The active Clause dependency is pinned by Git
revision in `Cargo.toml`; a mutable resource checkout is never build authority.
