# North

North is a durable coordination and execution harness for human and agent work.
It tracks general things in a Beagle Store fact graph, then derives meaning from
relations, contextual roles, and immutable history. Messages, assignments, and
run evidence share that graph, exposed through a CLI, MCP tools, and the Bridge
terminal app.

Use North to decide what is ready, run and steer agents across authenticated
provider accounts, and inspect what happened after a process or session ends.
Provider adapters and local journals carry execution evidence; the Store-backed
graph remains the authority for work and coordination.

North is pre-1.0 and currently supported on its author's configured hosts.
Portable installation is still in progress.

## Start

The checkout can always show its generated command card without a running
Store:

```console
$ ./bin/north help
```

On a configured host, verify the runtime, track one thing, and open Bridge:

```console
$ north doctor
$ north work track "Document the release boundary" --tracked-by @actor:tom --json
$ north work catalog --json
$ north bridge
```

Bridge opens on the fixed `Agents | Goals | All` navigation. `Agents` is the
semantic actor roster, `Goals` is every tracked thing with a desired outcome,
and `All` is the complete tracked-thing catalog. Select a row or use
`/agents`, `/goals`, and `/all`; type `/` for the commands available in the
current view. There is no top-level Referents category.

The shortest Bridge workflow is: open `north bridge`, choose an Agent, and type
a message. Use `Goals` to create or change tracked work and `All` to find,
inspect, or review its history. `/delegate` starts an admitted agent run from
the explicit arguments you supply; it does not silently turn the selected row
into a Task or transfer its ownership.

The main control surfaces are:

- work: `work`, `ready`, `show`, `history`;
- agents: `delegate`, `agents`, `watch`, `msg`;
- operations: `dashboard`, `doctor`, `health`; and
- reference: `help <topic>` or `help --all`.

## Work semantics

North does not give every tracked thing a fixed work type. The graph derives
these contextual roles from exact facts and occurrences:

- **Work** is a contextual role on a tracked thing.
- **Plan** is Work with a current, valid Plan revision.
- **Project** is a Plan with at least one valid historical `started`
  occurrence. Starting a later revision does not erase that history.
- **Task** is a Plan with a valid Assignment naming an Agent. Project and Task
  are independent: one tracked thing may be either or both.

A **Request** is an immutable addressed occurrence and may be about a tracked
thing. An **ACK** records that the recipient received that exact Request; it is
not acceptance, an Assignment, or an ownership transfer. Delegation is the
operational act of admitting and launching a run. Ownership changes only
through an acknowledged `work-ownership-v1` transition, while an Assignment is
the relation that makes its Plan a Task.

The command registry is [`cli/surface.edn`](cli/surface.edn). See
[building and testing](docs/building-and-testing.md) for runtime requirements,
the packaged path, and source builds.

## Documentation

- [Operating manual](docs/operating-manual.md) — command, coordination, and
  execution details.
- [Architecture](docs/architecture.md) — component ownership and Store write
  paths.
- [Provider architecture](docs/provider-architecture.md) — accounts, routing,
  fallback, and execution evidence.
- [System map](docs/system-map.md) — generated component and dependency map.
- [Influences](docs/INFLUENCES.md) — adopted mechanisms, provenance, and
  rejected boundaries.

## License

North is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. See the root
[license chooser](LICENSE).
