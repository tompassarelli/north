# North

North is a durable coordination and execution harness for human and agent work.
It tracks general things in a Beagle Store fact graph, then derives meaning from
relations, contextual roles, and immutable history. Messages, assignments, and
run evidence share that graph, exposed through a CLI, MCP tools, and the North
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

On a configured host, verify the runtime, track one thing, and open North:

```console
$ north doctor
$ north work track "Document the release boundary" --tracked-by @actor:tom --json
$ north work catalog --json
$ north
```

In North, choose an Agent and type a message; use `Goals` for desired outcomes
and `All` to inspect the complete catalog. Type `/` for commands in the current
view.

The main control surfaces are:

- work: `work`, `ready`, `show`, `history`;
- agents: `delegate`, `agents`, `watch`, `msg`;
- operations: `dashboard`, `doctor`, `health`; and
- reference: `help <topic>` or `help --all`.

The command registry is [`cli/surface.edn`](cli/surface.edn). See
[building and testing](docs/building-and-testing.md) for runtime requirements,
the packaged path, and source builds.

## Documentation

- [Operating manual](docs/operating-manual.md) — tracked-work semantics,
  commands, coordination, and execution details.
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
