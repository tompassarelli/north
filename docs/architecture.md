# North architecture

This document sets the owning boundaries of the `north` app, the TUI in
`src/` for directing coding work, conversations, explicit
delegation, goals, and recurring work. The rest of the harness, its commands,
hooks and policy, is described in [harness.md](harness.md).

The app is a clean reconstruction: it preserves North-v1's intended product
and strongest invariants without porting North-v1's implementation.

`north:agent-machinery/` is North's sole source for provider-independent
delivery doctrine, the project-exposure contract, and reusable agent
procedures. Keeping that module provider-independent does not
justify a second repository or a second live source. Direct Codex and other
consumers receive projections from this package; neither North-v1 nor the
retired standalone checkout is runtime authority.

## Owning boundary

```text
terminal input
    -> Rust host
    -> typed Rust North transition
    -> authorized foreign effect
    -> Codex app-server over stdio JSONL or an explicitly selected local socket
    -> Codex native coding tools
    -> effect receipt
    -> typed Rust settlement transition
    -> Rust projection in the North TUI
```

`src/state_core.rs` owns North semantics through domain structs, a typed
`Event` enum and exhaustive transition matching. This includes conversation
and lifecycle states, effect intent and settlement, resumability, goals,
recurrence, and intervention policy. `src/north_state.rs` supplies the typed
caller interface and projects state for the host. `src/menu.rs` owns the
standalone menu; the application state retains its separate menu transitions
and projects the shared menu view.

The host owns terminal I/O, rendering, processes, signals and JSONL transport.
`LocalStore` owns exclusive writer custody and persists the typed state under
`rust-state-v1`; its lifetime releases custody. Failed persistence blocks
later transitions and effect release. The old snapshot namespace remains
historical data and is not migrated. Codex app-server owns provider
communication, authentication, coding threads and turns, native tools,
sandboxing, and streamed events.

Before a build, `nix develop -c cargo check --locked --offline` checks the Rust
interfaces and exhaustive event matches. Existing state, restart and original
reference replay tests check behavior; the original replay bytes remain
historical evidence of the retired runtime.

Set `NORTH_CODEX_ENDPOINT=unix:///absolute/private-directory/codex.sock` to
connect North to an existing Codex app-server. North requires the socket and
its private directory to belong to the current user and checks the connected
peer's user identity. An explicit endpoint is authoritative: connection
failure never starts a replacement server. Closing or reconnecting North
closes its connection, leaving the shared server running.

For a deliberately shared session, run the selected installed Codex executable
with `app-server --listen unix:///absolute/private-directory/codex.sock`, and
connect the Codex terminal with `--remote` using the same address. The server
uses the account and environment selected when it starts. Both interfaces
then attach through the same server's `thread/resume`. North does not create
a persistent service or change the selected Codex runtime.

A conversation owned by a separate embedded Codex terminal has no attachable
endpoint. Its owner must release it before a shared server can resume that
same conversation. Reading saved history does not attach to a running turn.

`north --resume UUID` opens that exact conversation through the selected
connection, independent of the working directory's history order. The normal
launcher resolves its retained account home and server before starting North.
An explicit resume failure exits with the refusal; it never selects a different
conversation or starts a new one.

The host dispatches effects only after the typed state transition admits
them. Batch transitions restore state on rejection and advance the revision
once on success. Keep state choices in the typed core and operating-system or
provider calls at the host boundary.

Retain and exercise acceptance journeys under the
[verification skill](../agent-machinery/skills/verification/SKILL.md).

## Adopt, reject, defer

Adopt now:

- North-v1's product contract and delivery invariants, not its source graph.
- Typed Rust state and explicit events for admitted North state changes.
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
- A second semantic authority outside the typed Rust state core.
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

- Galileo at `d1195b99f5cc298eb52b2e9751fe493bebf8a502`: historical
  source-reference authority for the preserved transition replay; retired
  from the active product and build.
- Codex `61a44880a85d2fd0d8770908dea5733495e571c8`, Apache-2.0, plus the
  [official app-server documentation](https://learn.chatgpt.com/docs/app-server):
  process/TUI separation and the stdio JSONL protocol.
- DIRGE `37785707f3dd3543f76dc5125fd8f331ca1c0813`, GPL-3.0-only:
  conceptual prior art only; no implementation is copied or adapted.
- North-v1 `f2b11f49f2ba655a2ce3ba73acc9bfe6170b6123`, operator-owned reference:
  product intent, failure evidence, and invariants only.
- Agent Machinery `672ea2f0cfe6c6323423fe7e2a89e6789435ced5`, MIT OR Apache-2.0:
  imported as first-party source into `north:agent-machinery/`; its
  standalone checkout is migration evidence, not live authority.

Reference checkouts and original captures remain read-only historical
evidence. The product uses the typed Rust core without the retired runtime.
