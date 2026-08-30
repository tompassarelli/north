# North

North is a text-first, multi-account agent harness built on
[Beagle Store](https://github.com/tompassarelli/beagle). It keeps work,
execution authority, and run evidence in one durable fact model, then uses
provider CLIs, PTYs, MCP, and a terminal cockpit to execute and inspect that
model.

North is for running several agents across several authenticated subscription
accounts without turning a transcript folder, process table, or provider UI
into the scheduler. It can answer two separate questions after a process dies:
what work was authoritative, and what execution evidence was actually observed?

North is pre-1.0 and currently operated on its author's configured hosts. The
core below is on public `main`; portable installation is still in progress.

## One authority, explicit edges

```text
Beagle semantics                 Beagle Store
work · roles · policy    ──►     durable authority
                                 threads · account eligibility
                                 leases · attempts · observations
                                 replay position · receipts · safe next
                                              │
                                              ▼
provider capability edges        Codex / Claude CLIs · PTYs · MCP
                                 transcripts / JSONL · terminal UI
```

Store-authoritative state includes:

- work and its relationships, roles, and derived readiness;
- exact account role and execution eligibility;
- fenced thread and account leases;
- immutable execution attempts and their launch, provider-start, terminal, or
  proved-unsent evidence;
- admitted observations and effect or command receipts;
- the greatest contiguous, digest-linked wire-event position; and
- the conservative next action: reserve, launch, send, cancel, reconcile,
  advance, or no-op.

The other side of the boundary is deliberately thinner. Provider adapters
resolve protocols and stream events. PTYs launch processes. MCP exposes bounded
tools. Transcripts and JSONL preserve provider evidence. The dashboard renders
the current projection. None of them independently decides that work is ready,
that an account may execute it, or that an uncertain effect is safe to repeat.

See the [architecture](docs/architecture.md) and
[provider boundary](docs/provider-architecture.md) for the detailed ownership
rules.

## Routing work to models and accounts

A human or agent requests a provider-neutral role, capability tier,
deliberation level, posture, and topology. North then deterministically filters
authenticated accounts by Store authority, provider capability, subscription
pressure, resource policy, and any explicit pin before an adapter resolves a
concrete model.

An OpenAI account is admitted by exact singleton Store facts. Its role is
either `execution` with `execution_eligible=true`, or `oversight` with
`execution_eligible=false`. Oversight accounts remain visible in Store-backed
projections, but North will not launch work through them. A missing, duplicated,
or contradictory authority fact also fails closed.

Automatic allocation can be preferential, balanced, or reserved. A provider
pin may fall back only to another eligible account for that provider; an exact
account pin has no fallback. Any fallback must be proven pre-side-effect. The
resolved provider, account, model, reasoning, authority receipt, and fallback
path stay attached to the run.

Luna, Terra, and Sol are concrete OpenAI model families, not hard-coded agent
personas. The checked-in provider catalog currently maps the semantic ramp this
way:

| Requested tier | Current OpenAI resolution |
| --- | --- |
| `economy` | Sol at low reasoning |
| `standard` | Sol at medium reasoning |
| `senior` | Sol at high reasoning |
| `frontier` | Sol at xhigh reasoning (`max` only by exception) |

The catalog is policy data, so callers request the semantic tier and reasoning
they need instead of embedding that table in work. Current mappings and their
calibrated model notes live under
`north:agent-runtime/orchestration/providers/` and
`north:agent-runtime/orchestration/docs/deltas/`.

## Quick start

From a checkout, the shortest entry point is the generated CLI card. It works
without a running Store:

```console
$ ./bin/north help
```

Operational commands currently require a host-selected, matching Beagle Store
runtime. On a configured host, the shortest useful path is:

```console
$ north doctor
$ north capture "Document the release boundary"
$ north ready
$ north delegate "Document the release boundary"
$ north agents
$ north dashboard --once
```

Add an isolated Codex execution account explicitly, then inspect the route:

```console
$ north account add codex-personal openai --role execution
$ north account login codex-personal
$ north providers
$ north spawn implementer "Add the parser regression" --tier standard
```

Useful inspection and control commands are:

```console
$ north show <thread-id>
$ north watch <agent-id>
$ north msg <agent-id> "Use the smaller seam"
$ north lanes
$ north help routing
$ north help --all
```

The public command registry is [`cli/surface.edn`](cli/surface.edn); generated
help is checked against both that registry and [`bin/north`](bin/north).
[Building and testing](docs/building-and-testing.md) describes the current
Babashka, Bun, and Store requirements.

## Restartability and replay

North records intent before crossing an execution edge, then records provider
start, command delivery, terminal, or proved-unsent receipts afterward. Thread
and account leases fence concurrent ownership. On restart, the Store snapshot
is decoded fail-closed, wire events are reconstructed only through the greatest
contiguous digest-linked position, and the kernel computes a safe next action
from those immutable facts.

That makes interruption explicit instead of magical. A reserved attempt with
no launch intent may be launched. A launch intent without a provider-start
receipt requires reconciliation. A delivered command is not sent again. A
terminal or proved-unsent attempt can advance once. Conflicting facts, replay
gaps, or digest conflicts produce no executable guess.

Bridge journals, provider transcripts, and exported JSONL remain valuable:
they preserve exact wire evidence, support local recovery during a Store
outage, and feed projections. They are not scheduler truth. A partial
transcript cannot grant a lease, make an oversight account executable, or prove
an external effect completed.

`north dashboard` is the live text cockpit over fleet, health, work, accounts,
and execution evidence. Its Store-backed view includes account role and
eligibility, attempt state, lease fences, replay position, and safe-next
decisions. It is a control and inspection surface, not another authority.

## What exists now

| On public `main` | Near-term direction |
| --- | --- |
| Store-backed coordination graph, typed Beagle policy, and the current execution ledger | Move remaining provider-neutral resident decisions into typed Beagle over Store without a parallel host-language authority |
| Store-authoritative Codex account roles and execution eligibility; oversight excluded from execution | Extend the same explicit authority boundary wherever another provider or account class needs it |
| Durable attempt reservations, thread/account lease fences, launch and provider-start evidence, terminal/proved-unsent receipts, command delivery receipts, replay reconstruction, and safe-next decisions | Complete the resident scheduler loop without a parallel host-language decision path |
| Managed Claude and Codex execution, multi-account routing, pre-side-effect fallback, MCP, steering, journals, receipts, and a live text dashboard | Keep JavaScript only at provider, process, terminal, browser, and Node-only capability edges |
| A Nix flake and working configured-host deployment | Parameterize Store selection and document a portable first-run setup |

North's [influences](docs/INFLUENCES.md) are useful mainly for provenance: the
project studies bounded mechanisms from other agent harnesses, records the
exact licensed sources, and adopts ideas only when North can name their typed
owner, durable evidence, adapter boundary, and focused check. It does not use
an upstream harness as a hidden runtime or source of scheduler truth.

## License

North is dual-licensed under the [MIT License](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option. See the root
[license chooser](LICENSE).
