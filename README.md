# North

North is a work tracker and agent orchestrator whose board, lanes, and
timesheets are all queries over one graph of triples.

The primitive is a **thread**: any node carrying a `title`. There is no
`task`/`project`/`epic` type and no `state` column — a thread's condition is
read off its facts. `committed` means accepted, a live `driver` means active,
an unresolved `depends_on` means blocked, an `outcome` means done
([`src/north/projections.bclj`](src/north/projections.bclj)). Agent lanes are
threads too, so a running lane, its run ledger, its done-bar evidence, and the
intention it was spawned to serve all sit in the same graph as your own work,
and one query reads both. North supplies the coordination vocabulary and the
lifecycle derivations; the storage engine underneath is
[Fram](https://github.com/Autonymy/fram), a slot-addressable typed-triple
substrate.

## Documentation

- [docs/operating-manual.md](docs/operating-manual.md) — thread model, fact
  format, derived lifecycle, the CLI surface, agent lifecycle, concurrent-write
  safety. **Start here.**
- [docs/architecture.md](docs/architecture.md) — what lives where: engine,
  coordination domain, CLI, agent SDK, MCP edge, your data.
- [docs/hosting.md](docs/hosting.md) — laptop and server-owned operating
  layouts.
- [docs/provider-architecture.md](docs/provider-architecture.md) — routing,
  provider accounts, subscription-entitlement billing.
- [docs/fact-native-redesign.md](docs/fact-native-redesign.md) and
  [docs/PROPOSAL.md](docs/PROPOSAL.md) — the design record.
- [docs/building-and-testing.md](docs/building-and-testing.md) — rebuilding
  from source and running the suites.

## Quickstart

```console
$ nix run github:tompassarelli/north
north — coordinate work, agents, and time

  NOW
    north dashboard               agents, concerns, board, health — one screen
    north ready                   what you could start now, ranked by leverage
    north inbox                   notifications waiting on you
    north agenda                  dated and overdue work

  WORK
    north capture "<thought>"     one thought → one committed thread
    north show <id>               a thread's facts + body
    north tell <id> <pred> <val>  assert a fact (retract removes)
    north threads                 active / ready / blocked overview

  AGENTS
    north delegate "<task>"       hand work to a new lane
    north agents                  who's live now
    north watch <id>              tail one agent's transcript
    north steer <id> "<msg>"      nudge it mid-flight

  [SYSTEM and MORE groups elided]
```

That card is one screen because work and agents are one graph — `north ready`
and `north agents` are two projections of the same triples. It is generated
from [`cli/surface.edn`](cli/surface.edn), and
[`cli/tests/surface-sync-test.clj`](cli/tests/surface-sync-test.clj) fails when
the registry, the rendered pages, and `bin/north`'s dispatch disagree.
`north help <topic>` opens one of six topic pages; `north help --all` prints
the whole surface.

Without Nix, the ledger needs [babashka](https://babashka.org) and a Fram
checkout on `FRAM_HOME`; the agent SDK and MCP edge also need
[Bun](https://bun.sh). See
[docs/building-and-testing.md](docs/building-and-testing.md).

## Why?

- **Lifecycle is derived, never stored.** There is no status field to forget to
  update: ready is committed ∧ unblocked, blocked is an unresolved
  `depends_on`, done is an `outcome`. A `driver` fact is an assignment rather
  than proof of activity, so liveness enters as a separate classifier input
  ([`src/north/projections.bclj`](src/north/projections.bclj)).
- **Agents and intentions share one graph.** A spawned lane gets a full-UUID
  identity, a run reservation written before the provider is invoked, a run
  ledger, and a truthful terminal (`delivery=reported|unverified|blocked`)
  ([`sdk/src/spawn.ts`](sdk/src/spawn.ts),
  [`cli/run-ledger.clj`](cli/run-ledger.clj)).
- **Done-bars carry evidence.** Dispatch warns when a committed thread has no
  `done_when` ([`sdk/src/dispatch.ts`](sdk/src/dispatch.ts)), and workers record
  observed probe results with `north evidence record`, reserved against the bars
  the thread carried at dispatch ([`cli/bars-cli.clj`](cli/bars-cli.clj)).
- **Concurrent agents coordinate without locking.** *Concerns* declare a
  footprint and coexist, *leases* claim exclusive jurisdiction, and *mail* plus
  `north watch`/`steer`/`retask` drive live lanes
  ([`cli/concern-cli.clj`](cli/concern-cli.clj),
  [`cli/lease-cli.clj`](cli/lease-cli.clj),
  [`cli/msg-cli.clj`](cli/msg-cli.clj)).
- **One serialized write path.** Every write goes through the current Fram
  server on `127.0.0.1:7977` (`NORTH_PORT`), which serializes and rule-checks
  each publication through canonical FRAMRPC.
- **Agent duration is run telemetry.** Every managed lane records `kind=run`
  with its agent, thread, observed duration, outcome, and estimate comparison
  ([`sdk/src/telemetry.ts`](sdk/src/telemetry.ts)).

## Status

North is pre-1.0: surfaces change between releases and there are no
back-compatibility shims. Your data is not in this repository — canonical
FRAMLOG databases live in your own state directory and are projected at
runtime. When the Fram server or its runtime is unavailable, `north panic` is a
Bash-only recovery path that writes `dispatch=native` and `guards=off` to
`~/.local/state/north/harness.conf`, preserves other keys, and prints the exact
restore commands.

For the working manual, start with
[docs/operating-manual.md](docs/operating-manual.md).

## License

North is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. See the root
[license chooser](LICENSE).
