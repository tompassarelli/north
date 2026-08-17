# Architecture and project layout

What lives where, and which layer owns which decision. The working manual is
[operating-manual.md](operating-manual.md); this file answers "where do I go to
change X."

## The layer stack

**Engine** → [Beagle Store](https://github.com/Autonymy/fram), selected by the installed
wrapper through the sealed FRAMRPC environment.
Beagle Store is a slot-addressable, typed-triple substrate: the triple store, the
Datalog evaluator, and the canonical FRAMRPC server. North does not vendor it,
fork it, or package it: [`flake.nix`](../flake.nix) deliberately does not select
a second engine, and both the checkout launcher and the installed package source
the host-published `beagle-store.env` for the engine's identity. That one sealed
environment is the single place the engine revision is decided.

**Coordination domain** → [`src/north/*.bclj`](../src/north). The vocabulary
and derivations that turn a domain-neutral triple store into a work ledger:

| module | owns |
|---|---|
| `projections.bclj` | the derived lifecycle — ready, blocked, terminal, driver liveness |
| `staleness.bclj` | needs-review and the staleness classifiers |
| `validate.bclj` | North's work rules on top of the engine's integrity rules |
| `worker_policy.bclj` | coordination-worker policy — idle/backoff bounds, admission, selection |
| `audit.bclj` | audit projections |
| `main.bclj` | the life-verb entry point `bin/north` slots into |

These are authored in [Beagle](https://github.com/Autonymy/beagle) and compiled
to Clojure under [`out/`](../out), which is committed — see
[building-and-testing.md](building-and-testing.md).

**CLI** → [`bin/north`](../bin/north). It aims the Beagle Store engine at your data,
sets capture provenance, and dispatches: life and coordination verbs
(`ready`/`threads`/`capture`/`agents`/`spawn`/`delegate`/`watch`/
`trace`/`config`) route to `north.main` or a [`cli/`](../cli) handler; engine
verbs (`import`/`show`/`tell`) pass through to Beagle Store. Any verb the registry does
not claim passes through to the engine. `validate` is the exception that proves
the split: it is **North-handled**, running the full check — the engine's
generic integrity rules plus North's work rules in `north.validate` — because
those work rules moved out of the kernel.

The human surface is a registry, not prose: [`cli/surface.edn`](../cli/surface.edn)
declares every verb, alias, topic, and help-card entry, and
[`cli/surface-gen.clj`](../cli/surface-gen.clj) renders
[`share/help/`](../share/help) from it.
[`cli/tests/surface-sync-test.clj`](../cli/tests/surface-sync-test.clj) welds
the registry, the rendered pages, and `bin/north`'s case arms together in both
directions, so a verb cannot exist in one and not the others.

**Agent surface** → [`cli/agents-cli.clj`](../cli/agents-cli.clj) and the
TypeScript SDK under [`sdk/src/`](../sdk/src): spawn
([`spawn.ts`](../sdk/src/spawn.ts)), dispatch
([`dispatch.ts`](../sdk/src/dispatch.ts)), the run ledger, routing, and the
provider adapters. Each managed lane receives a fresh full-UUID identity, a run
reservation written before the provider is invoked, a run ledger, and a
truthful terminal (`delivery=reported|unverified|blocked`).

**Bridge** → [`sdk/src/bridge/`](../sdk/src/bridge): the durable local execution
host. `northd.ts` is the daemon, `journal.ts` its append-and-replay log,
`protocol.ts` the wire between them, and `app.bjs` the terminal UI. It owns
`north bridge` and fronts `north dashboard` — the dashboard verb runs through
the bridge CLI, which re-execs [`cli/dashboard-cli.clj`](../cli/dashboard-cli.clj).
The bridge does not read or write the coordinator, so replay survives a Beagle Store
outage.

**MCP** → [`bin/north-mcp`](../bin/north-mcp), the AI-facing edge. Every tool
maps to a tested CLI operation through the Beagle Store server write path, so in-harness
agents dispatch through `mcp__north__dispatch` / `spawn` rather than the shell
verbs.

**Data** → your own private store. Canonical FRAMLOG databases live in runtime
state and are **not** part of this repository.

## The write path

Every **coordination-graph** write serializes through one current Beagle Store server,
which rule-checks it before it lands. The configured server listens locally on
`127.0.0.1:7977` (`NORTH_PORT`); canonical FRAMRPC requests carry the selected
SpaceId, and `north:cli/runtime-attestation.clj` binds the live listener to the
sealed Beagle Store release `BEAGLE_STORE_HOME` names — its receipt's revision and tree, its
Native artifact, database, and service owner.

Two writes are deliberately carved out of that path. Telemetry subjects
(`run:`/`session:`/`mine:`/`guard_denial:`) route to the telemetry partition on
its own port and space when `NORTH_TELEMETRY_PARTITION=1`
([`bin/north`](../bin/north)); and the Bridge journal is a local
append-and-replay log that keeps working while Beagle Store is down. Neither is a
coordination fact, and neither is exempt from rule-checking once it becomes one.

## Routing: who versus where

`north spawn` and `north delegate` read the staffing catalog at
[`orchestration/staffing/catalog.json`](../orchestration/staffing/catalog.json)
to answer *who* does the work — role, tier, reasoning, posture. North answers
*where* it runs and *how* you see it: provider account, subscription pressure,
dashboard. The staffing layer is account-blind; North resolves the semantic tier
through the chosen provider's catalog. See
[provider-architecture.md](provider-architecture.md).

## Emergency recovery

`north panic` is a Bash-only kill switch that works when babashka, Beagle Store, or the
FRAMRPC server are unavailable. It writes `dispatch=native` and `guards=off` to
`~/.local/state/north/harness.conf`, preserves the other keys, and prints the
exact restore commands. Use it to return to stock native operation while
repairing North, not as a routine posture change — `north config` is the
routine surface.

## Hosting

The same architecture runs on a laptop or on a server you own: the runtime is
the Beagle Store server plus North's coordination workers, and nothing above assumes a
particular host. There is no separate hosting guide yet.
