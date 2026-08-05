# Architecture and project layout

What lives where, and which layer owns which decision. The working manual is
[operating-manual.md](operating-manual.md); this file answers "where do I go to
change X."

## The layer stack

**Engine** → [Fram](https://github.com/Autonymy/fram), selected by the installed
wrapper through the sealed FRAMRPC environment.
Fram is a slot-addressable, typed-triple substrate: the triple store, the
Datalog evaluator, and the canonical FRAMRPC server. North does not vendor it and
does not fork it — it links Fram's library API, so Fram's exact source revision
is pinned by the `fram` node in [`flake.lock`](../flake.lock). The Nix package,
CI, and the Docker image all consume that one lock record; there is no second
revision file to update or let drift.

**Coordination domain** → [`src/north/*.bclj`](../src/north). The vocabulary
and derivations that turn a domain-neutral triple store into a work ledger:

| module | owns |
|---|---|
| `projections.bclj` | the derived lifecycle — ready, blocked, terminal, driver liveness |
| `staleness.bclj` | needs-review and the staleness classifiers |
| `validate.bclj` | North's work rules on top of the engine's integrity rules |
| `gatepolicy.bclj` | gate policy evaluation |
| `audit.bclj` | audit projections |

These are authored in [Beagle](https://github.com/Autonymy/beagle) and compiled
to Clojure under [`out/`](../out), which is committed — see
[building-and-testing.md](building-and-testing.md).

**CLI** → [`bin/north`](../bin/north). It aims the Fram engine at your data,
sets capture provenance, and dispatches: life and coordination verbs
(`ready`/`threads`/`capture`/`agents`/`spawn`/`delegate`/`watch`/
`trace`/`config`) route to `north.main` or a [`cli/`](../cli) handler; engine
verbs (`import`/`show`/`validate`/`tell`) pass through to Fram. Any verb the
registry does not claim passes through to the engine.

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

**MCP** → [`bin/north-mcp`](../bin/north-mcp), the AI-facing edge. Every tool
maps to a tested CLI operation through the Fram server write path, so in-harness
agents dispatch through `mcp__north__dispatch` / `spawn` rather than the shell
verbs.

**Data** → your own private store. Canonical FRAMLOG databases live in runtime
state and are **not** part of this repository.

## The write path

Every write serializes through one current Fram server, which rule-checks it
before it lands. The configured server listens locally on `127.0.0.1:7977`
(`NORTH_PORT`); canonical FRAMRPC requests carry the selected SpaceId, and
`north:cli/runtime-attestation.clj` binds the live
listener to its exact Fram source, artifact, database, and service owner.

## Routing: who versus where

`north spawn` and `north delegate` read the staffing catalog at
[`orchestration/staffing/catalog.json`](../orchestration/staffing/catalog.json)
to answer *who* does the work — role, tier, reasoning, posture. North answers
*where* it runs and *how* you see it: provider account, subscription pressure,
dashboard. The staffing layer is account-blind; North resolves the semantic tier
through the chosen provider's catalog. See
[provider-architecture.md](provider-architecture.md).

## Emergency recovery

`north panic` is a Bash-only kill switch that works when babashka, Fram, or the
FRAMRPC server are unavailable. It writes `dispatch=native` and `guards=off` to
`~/.local/state/north/harness.conf`, preserves the other keys, and prints the
exact restore commands. Use it to return to stock native operation while
repairing North, not as a routine posture change — `north config` is the
routine surface.

## Hosting

The same architecture runs on a laptop or on a server you own. The supported
operating layouts are documented in [hosting.md](hosting.md).
