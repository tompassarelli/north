# North

North is a Beagle-native, Store-backed harness for running agents against one
durable model of work. Threads, dependencies, agent identities, observations,
run evidence, and outcomes are related facts rather than records scattered
across a task tracker, transcript directory, scheduler, and provider dashboard.

The result is a harness that can answer both “what should happen next?” and
“what actually happened?” from preserved state. North coordinates multiple
provider accounts, launches and steers agent lanes, records replayable execution
evidence, and projects the same state into a live text cockpit.

North is pre-1.0 and currently developed for its author's own hosts. The core
model is landed; some ownership is still moving from existing Clojure,
TypeScript, and process adapters into Beagle Native over Beagle Store. The
[current state and near destination](#current-state-and-near-destination) are
separated below.

## The model

The durable unit is a typed **thread**. A thread can describe a human intention,
an agent lane, or a larger body of work. `part_of` and `depends_on` relate
threads without introducing a parallel project/task hierarchy. Conditions such
as ready, blocked, active, and resolved are projections from facts, not a
mutable status field.

An admitted semantic change produces a new immutable **world revision**. A
world revision contains the typed threads, their edges, facts, and effect
records known at that point. A **branch** is only a mutable lineage pointer to a
world revision; it is not the semantic state itself. This distinction keeps
replay, comparison, and provenance about immutable values even when current
work moves forward.

North's typed [`ThreadWorld`](src/north/thread_core.bgl) gives facts explicit
epistemic status:

| Fact status | What it means | Required provenance |
| --- | --- | --- |
| **Declared** | An actor asserted an intention or claim. | Actor identity |
| **Derived** | A rule computed a claim from existing facts. | Rule identity and parent fact identities |
| **Observed** | An observer or probe witnessed a result. | Observer and observation identities |
| **Effect record** | An admitted executor reports an attempted or completed effect. | Effect and executor identities |

These categories are not interchangeable. A declaration is not proof, a
derivation remains inspectable through its inputs, an observation names its
witness, and an effect record does not become an external-state guarantee merely
because a process exited successfully.

## Architecture

```text
typed Beagle semantics
  threads · world revisions · memory · replay · provenance · scheduling
                              │
                    Beagle Store authority
                 durable facts · serialized admission
                              │
                projections and capability edges
           CLI · MCP · provider APIs · PTYs · terminal UI
```

- **Semantics are target-neutral Beagle.** Work and prompt lifecycle,
  dependency and admission rules, memory selection, session lineage, replay,
  provenance, and scheduling decisions must not name a provider model or host
  process API. The landed typed cores live under [`src/north/`](src/north/).
- **Beagle Store is the durable substrate.** The current coordination graph is
  published through one serialized, rule-checked Store RPC path. Canonical
  Store logs live in runtime state, outside this repository. See
  [the write path](docs/architecture.md#the-write-path).
- **Beagle JS and TypeScript are capability edges.** They integrate provider
  CLIs and SDKs, authenticated subscription accounts, MCP, filesystems,
  browsers, PTYs, and the terminal. Provider decoding stays under
  [`sdk/src/providers/`](sdk/src/providers/); host execution and the cockpit
  stay under [`sdk/src/bridge/`](sdk/src/bridge/). They report evidence inward
  rather than becoming a second semantic authority.

North uses one durable identity and provenance model across the system, but it
does not pretend every boundary is the same:

- The **execution boundary** decides which provider account and process may run
  a lane and records the resolved route.
- The **security boundary** admits a capability envelope. North enforces
  application authority; hostile same-user code still requires an OS or
  container boundary outside the harness.
- The **effect boundary** separates a semantic decision from a host or external
  side effect. Admission happens before the effect and a receipt comes back
  afterward.

Identity can therefore remain stable across retries, provider sessions, and
processes without conflating who or what a run is with where it executes, what
it may access, or which effects it performed.

## What is available now

### Typed work and durable coordination

- The coordination graph stores intentions, agent lanes, assignments,
  dependencies, concerns, leases, mail, evidence, and outcomes. Lifecycle is
  derived from those facts
  ([`src/north/projections.bclj`](src/north/projections.bclj)).
- The typed thread-world core admits threads, `part_of` and `depends_on` edges,
  provenance-bearing facts, and effect records as immutable values
  ([`src/north/thread_core.bgl`](src/north/thread_core.bgl)).
- Every managed lane receives a full UUID identity, a pre-provider run
  reservation, an ordered event ledger, run provenance, and an explicit
  terminal outcome
  ([`sdk/src/spawn.ts`](sdk/src/spawn.ts),
  [`sdk/src/run-ledger.ts`](sdk/src/run-ledger.ts)).

### Memory, lineage, replay, and scheduling

Target-neutral Beagle cores are landed for:

- scoped memory with source, trust, validity, supersession, and bounded recall
  ([`memory_core.bgl`](src/north/memory_core.bgl));
- immutable session lineage and context projection from a compaction plus its
  tail ([`session_core.bgl`](src/north/session_core.bgl));
- ordered replay, stable-prefix comparison, divergence, and terminal provenance
  ([`replay_core.bgl`](src/north/replay_core.bgl)); and
- scheduled-run origin, deduplication, revision checks, leases, and stale-owner
  reclamation ([`scheduled_run_core.bgl`](src/north/scheduled_run_core.bgl)).

The Bridge also keeps an append-and-replay host journal so local execution can
be recovered across a Store outage. That journal is execution evidence, not a
second coordination database.

### Allocation and orchestration

North separates semantic staffing from concrete execution. Orchestration
selects a role, capability tier, reasoning level, posture, and topology without
naming a provider model. North then resolves that request against authenticated
provider accounts and their current model availability.

Automatic routing allocates work across subscription accounts using configured
order, per-account entitlement pressure, weights, and reserved capacity. A
provider or account fallback is allowed only when the adapter proves the failed
attempt produced no observable side effect. The selected provider, account,
model, reasoning level, allocation reason, and fallback path remain in run
provenance. See [provider architecture](docs/provider-architecture.md).

### Live cockpit and control

`north dashboard` presents fleet, health, work, and account snapshots in one
text cockpit. The CLI and MCP surfaces can inspect live agents, tail transcripts,
send mid-run input, redirect a goal, and query durable lane receipts. The
cockpit is a projection of coordination and execution evidence; it is not the
authority those records depend on.

## A minimal current path

On a host with North, Babashka, Bun, and the selected Beagle Store runtime
configured:

```console
$ north dashboard
$ north capture "Document the release boundary"
$ north ready
$ north delegate "Document the release boundary"
$ north agents
$ north watch <agent-id>
$ north show <thread-id>
```

Use `north help <topic>` for `work`, `agents`, `comms`, `routing`, `store`, or
`ops`; `north help --all` prints the complete registered surface. The command
registry is [`cli/surface.edn`](cli/surface.edn), and a test keeps it aligned
with generated help and [`bin/north`](bin/north).

Running the coordination ledger needs Babashka and the host-selected Beagle
Store runtime. The agent SDK, MCP edge, and cockpit also need Bun. See
[building and testing](docs/building-and-testing.md).

The flake is not portable yet: its packaged entrypoint still selects a
machine-local `beagle-store.env`, so `nix run github:tompassarelli/north` does
not currently work on an unconfigured host.

## Why this helps multi-agent development

Multiple agents create leverage only when their work remains attributable and
recoverable. North gives each lane a stable identity, binds it to the intention
it serves, records what route and authority it received, and preserves ordered
evidence about what ran. Dependencies and ownership are queryable before work;
receipts and outcomes are queryable afterward.

That reduces three ordinary coordination costs: duplicate work is visible,
blocked work can be scheduled from explicit dependencies, and a dead or
interrupted process can be understood from durable evidence instead of guessed
from a partial transcript. Account allocation also lets concurrent work use
available subscription capacity without putting provider-specific choices into
the work model.

## Current state and near destination

| Area | Shipped now | Near destination |
| --- | --- | --- |
| Work | A live Store-backed coordination graph plus a typed `ThreadWorld` admission core. | Store-persisted world revisions become the sole authority for the typed thread model and its projections. |
| Memory and context | Typed Beagle memory, session-lineage, replay, provenance, and scheduled-run cores; existing execution journals and receipts. | Beagle Native owns their resident Store operations and scheduling, with no parallel host-language decision path. |
| Execution | Managed Anthropic and OpenAI/Codex lanes, semantic routing, authenticated account selection, subscription-pressure allocation, replay-safe fallback, MCP, and live steering. | Provider adapters contain only wire decoding and process capabilities; all provider-neutral policy is evaluated before crossing the edge. |
| Cockpit | A live terminal view over fleet, health, board, accounts, and Bridge sessions. | Every view is a bounded projection from the same world revisions and execution receipts. |
| Distribution | Works on the configured development hosts and remains pre-1.0. | A portable package with parameterized Store selection and documented setup. |

The ownership direction is explicit in
[Influences and ownership](docs/INFLUENCES.md): durable meaning moves inward to
typed Beagle and Beagle Native over Store; JavaScript remains only where the
operating system, terminal, browser, or provider requires it.

## Documentation

- [Operating manual](docs/operating-manual.md) — the current thread model,
  derived lifecycle, CLI, agent lifecycle, and concurrent-write behavior.
- [Architecture](docs/architecture.md) — layer ownership and the Store write
  path.
- [Harness architecture](docs/harness-architecture.md) — execution, replay,
  and Bridge boundaries.
- [Provider architecture](docs/provider-architecture.md) — routing, accounts,
  subscription allocation, fallback, and run evidence.
- [Influences and ownership](docs/INFLUENCES.md) — landed typed cores, intended
  owners, rejected architecture, and license-aware provenance.
- [Building and testing](docs/building-and-testing.md) — runtime requirements,
  rebuilding, and repository checks.

## License

North is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. See the root
[license chooser](LICENSE).
