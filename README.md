# North

A **fact-native coordination substrate**. One graph of `(subject predicate
object)` triples, two faces derived from it:

- **A life/work ledger** — capture an intention; query what's **ready**,
  **blocked**, and the highest-leverage keystone. The board is *derived*, never
  hand-maintained.
- **A managed multi-agent orchestrator** — a canonical coordinator daemon,
  agent lanes spawned with full identity/telemetry/attribution, Gaffer-routed
  dispatch, and concurrent-agent coordination (concerns, leases, mail).

These are not two products. **Agents are threads; coordination is facts.** A
spawned lane, its run ledger, its done-bar evidence, and the intent it serves
share one graph with your personal work — so the same reads see both, and
lifecycle is *derived* (committed = accepted, `outcome` = done, `driver` =
active, `depends_on` = blocked), never a stored status field.

North is a **consumer of the [Fram](https://github.com/tompassarelli/fram)
engine** (a domain-neutral fact substrate). It supplies the *coordination
domain*: the lifecycle projections, the cardinality vocab (`FRAM_SINGLE_VALUED`),
capture conventions, time tracking, and the agent lifecycle + routing surface.

## The two faces

### The personal ledger — capture → ready/blocked/leverage → clock

The primitive is `thread`: any node with a `title`. There is no `task`/
`project`/`epic` type and no `state` enum — condition is a **query over facts**.
Capture a thought, assert facts about it, and the projections do the rest:

```sh
north capture "<thought>"   # mint a committed thread (fact-first)
north ready                 # committed ∧ unblocked, ranked by leverage
north blocked               # waiting on a depends_on target
north board                 # active drivers + top-ready + counts (alias: plate)
north show <id>             # one thread's facts + body
north clock in <owner>      # one human client billing session (Clockify sync target)
```

Time tracking (`north clock`), staleness/`needs-review`, and billing are all
fact-native projections — `src/north/{projections,clock,clockify,staleness,audit}.bclj`.

### The agent substrate — coordinator → lanes → Gaffer routing → attribution

The same graph is the coordination plane for managed multi-agent work:

- **Coordinator** — a canonical Fram daemon on `127.0.0.1:7977` (default
  `NORTH_PORT`). Every write serializes and is rule-checked through it.
  `north up` ([`bin/north-coord-up`](bin/north-coord-up)) starts it locally; the
  hosted mode runs one per tenant via
  [`deploy/north-coordinator@.service`](deploy/north-coordinator@.service).
- **Managed lanes** — spawned through the TypeScript SDK
  ([`sdk/src/spawn.ts`](sdk/src/spawn.ts),
  [`sdk/src/dispatch.ts`](sdk/src/dispatch.ts)). Each lane gets a fresh
  full-UUID identity, a run reservation with a capability minted *before*
  provider execution, a run ledger, and a truthful terminal
  (`delivery=reported|unverified|blocked`).
- **Gaffer routing** — `north spawn`/`delegate` read Gaffer's staffing catalog
  (`~/code/gaffer/staffing/catalog.json`) to answer *who* does the work
  (role/tier/reasoning/posture); North answers *where* it runs and *how* you see
  it (provider account, subscription pressure, dashboard). Gaffer is
  account-blind; North resolves the tier through the chosen provider's catalog.
  See [docs/provider-architecture.md](docs/provider-architecture.md).
- **Done-bar evidence** — dispatch warns when a committed thread lacks a
  `done_when` bar; managed workers record observed probe results with
  `north evidence record`, run-scoped and capability-checked.
- **Concurrent-agent coordination** — *concerns* declare a footprint (they
  coexist, never block), *leases* claim exclusive jurisdiction, and *mail*
  (`msg-cli`) plus `north watch`/`steer`/`retask` drive live lanes.

```sh
north agents [--json]           # who's live now + the stable machine roster
north spawn <role> "<prompt>"   # compose a worker from Gaffer's catalog
north delegate "<task>" ...     # atomic (--role) or --composite handoff
north watch <id>                # tail a running lane's transcript
north steer <id> "<msg>"        # inject a message into a running lane
north trace <id>                # diagnose one lane's lifecycle (F1–F7)
```

In-harness agents dispatch through MCP (`mcp__north__dispatch` / `spawn`) rather
than the shell verbs.

## Shape

- **Engine** → [Fram](https://github.com/tompassarelli/fram) (`~/code/fram`):
  facts, Datalog, the coordinator daemon. The hard substrate.
- **Coordination domain** → `src/north/*.bclj`: the lifecycle derivations,
  billing projection, and staleness layer that make the engine a work ledger.
- **CLI** → [`bin/north`](bin/north): aims the Fram engine at your data and sets
  capture provenance. Life/coordination verbs (`ready`/`board`/`capture`/`clock`/
  `agents`/`spawn`/`delegate`/`watch`/`trace`/`config`/…) route to `north.main`
  or the `cli/` handlers; engine verbs (`import`/`show`/`validate`/`tell`/…) to Fram.
- **Agent surface** → [`cli/agents-cli.clj`](cli/agents-cli.clj) and the
  TypeScript SDK under [`sdk/src/`](sdk/src): spawn, dispatch, run ledger,
  routing, provider adapters.
- **MCP** → [`bin/north-mcp`](bin/north-mcp): the AI-facing edge — every tool
  maps to a tested CLI op through the coordinator write path.
- **Data** → your own private store (the canonical `facts.log`, projected to
  `~/.local/state/north/` at runtime). Data is **not** part of this repo.

## Hosting

Run it three ways off one architecture — **on your laptop, on a server you own,
or as a multi-tenant service you host for others** — with no fork in the design;
only the transport in front of the coordinator changes.

- **[docs/hosting.md](docs/hosting.md)** — the three modes (self-host single box,
  self-host remote, multi-tenant SaaS), the instance-per-tenant model, security,
  ops, and the roadmap.
- **[deploy/](deploy/)** — `docker-compose.example.yml`, systemd units, and the
  authenticated **[gateway](deploy/gateway/)** (bearer token → tenant → that
  tenant's coordinator) with `provision.sh` + an integration test. The one
  runtime image (`Dockerfile`, bb + Fram + North) lives at the repo root.

## Docs

- [docs/operating-manual.md](docs/operating-manual.md) — the working manual:
  thread model, fact format, derived lifecycle, the CLI surface, agent
  lifecycle, concurrent-write safety, and session behavior. **Start here.**
- [docs/fact-native-redesign.md](docs/fact-native-redesign.md) — the design
  record for the fact-native model.
- [docs/provider-architecture.md](docs/provider-architecture.md) — routing,
  provider accounts, and subscription-entitlement billing.
- [docs/PROPOSAL.md](docs/PROPOSAL.md) — the original vision and architecture.

## Running and building

**Running the ledger needs only [babashka](https://babashka.org)** — the
compiled Clojure is committed in `out/` (no Beagle required at runtime), same as
Fram. You need the Fram engine checked out too (`FRAM_HOME`, default
`~/code/fram`); `bin/north` puts both on the classpath. The agent SDK and MCP
edge additionally need [Bun](https://bun.sh).

North links Fram's library API, so its exact source is pinned by the `fram`
node in [`flake.lock`](flake.lock). The Nix package, CI, and Docker image all
consume that one lock record mechanically; there is no second revision file to
update or let drift.

To **rebuild** from the `.bclj` sources you also need
[Beagle](https://github.com/tompassarelli/beagle) (the Lisp North is written
in). `build.sh` links the engine sources in (`src/fram`, gitignored) and compiles
the coordination-domain modules into `out/`; commit the result when sources
change. Set `FRAM_HOME`/`BEAGLE_HOME` if they aren't at the defaults.

## Tests

Life-domain (babashka) + gateway, then the agent SDK (the `sdk` package script
owns hermetic preloads and test isolation — don't bypass it):

```sh
CP="out:$FRAM_HOME/out"
bb -cp "$CP" clock_test.clj
bb -cp "$CP" staleness_test.clj
FRAM_LOG="$FRAM_HOME/facts.log" bb -cp "$CP" lifecycle_test.clj
bash deploy/gateway/smoke_test.sh        # gateway auth + routing
cd sdk && bun run check && bun run test  # TypeScript agent SDK
```

## License

North is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. See the root
[license chooser](LICENSE).
