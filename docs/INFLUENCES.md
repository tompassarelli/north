# Influences

North is an independent agent harness built around one durable coordination
graph. It studies other harnesses for bounded mechanisms, not for a host
runtime, product surface, or source tree to copy. The controlling synthesis is:

> Provider-independent reasoning and orchestration belong in target-neutral,
> typed Beagle. Durable memory, identity, scheduling, replay, provenance, and
> resident hot paths belong in Beagle Native over Beagle Store. JavaScript is a
> thin capability boundary for PTYs, provider CLIs, browsers, and Node-only
> integrations.

An upstream idea earns a place only when North can name the behavior, its
owner, its durable evidence, and the boundary that keeps provider and host
details out of core semantics. North adopts ideas rather than source
expression. Python, Racket, shell, and compatibility layers are not candidate
implementation targets.

## Evidence set and licensing

The audit is commit-specific. A refreshed upstream commit can differ from the
read-only local checkout; references below identify the revision whose ideas
were considered. Links are attribution and research provenance, not notice
that source was copied.

| Project | License | Local evidence | Refreshed default branch |
| --- | --- | --- | --- |
| [oh-my-pi](https://github.com/can1357/oh-my-pi) | [MIT](https://github.com/can1357/oh-my-pi/blob/72000acfeb902e21816252699482887f34d1a5a4/LICENSE) | [`45e12e5b`](https://github.com/can1357/oh-my-pi/tree/45e12e5bb758198a920c6070e7e64cb33b21beac) | [`72000acf`](https://github.com/can1357/oh-my-pi/tree/72000acfeb902e21816252699482887f34d1a5a4) |
| [Qwen Code](https://github.com/QwenLM/qwen-code) | [Apache-2.0](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/LICENSE) | [`c1b8f1a1`](https://github.com/QwenLM/qwen-code/tree/c1b8f1a11f245e8a2a83df5dbfaafad69e02b244) | [`5f3165f1`](https://github.com/QwenLM/qwen-code/tree/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39) |
| [Pi](https://github.com/earendil-works/pi) | [MIT](https://github.com/earendil-works/pi/blob/b7bb00b936dbe21b8e160b3e89efdec361846699/LICENSE) | [`b7bb00b9`](https://github.com/earendil-works/pi/tree/b7bb00b936dbe21b8e160b3e89efdec361846699) | `b7bb00b9` |
| [Hermes Agent](https://github.com/NousResearch/hermes-agent) | [MIT](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/LICENSE) | [`3ef6bbd2`](https://github.com/NousResearch/hermes-agent/tree/3ef6bbd201263d354fd83ec55b3c306ded2eb72a) | [`b259668c`](https://github.com/NousResearch/hermes-agent/tree/b259668cac1cba7faf913c227b9262fe7a513da2) |
| [Cline](https://github.com/cline/cline) | [Apache-2.0](https://github.com/cline/cline/blob/1bb0833287a7c1dfbb78d53060611bd1cdb35901/LICENSE) | [`1bb08332`](https://github.com/cline/cline/tree/1bb0833287a7c1dfbb78d53060611bd1cdb35901) | `1bb08332` |

MIT and Apache-2.0 both permit studying and adapting ideas. If North later
copies or adapts protected expression, the relevant copyright, license text,
and attribution must travel with it in
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md). This audit currently
records no copied source.

## What North already has

- One graph for user intentions, agent lanes, assignments, dependencies, and
  outcomes, with lifecycle derived from facts rather than a mutable status
  field ([`src/north/projections.bclj`](../src/north/projections.bclj)).
- A serialized Beagle Store publication path for coordination facts and a
  separate append-and-replay Bridge journal for host execution while the Store
  is unavailable ([`docs/architecture.md`](architecture.md#the-write-path),
  [`sdk/src/bridge/journal.ts`](../sdk/src/bridge/journal.ts)).
- Full-UUID agent identities, a pre-provider run reservation, exact provider
  event ledgers, and evidence-bearing delivery terminals
  ([`sdk/src/identity.ts`](../sdk/src/identity.ts),
  [`sdk/src/dispatch.ts`](../sdk/src/dispatch.ts),
  [`sdk/src/run-ledger.ts`](../sdk/src/run-ledger.ts)).
- Provider-neutral semantic routing with provider model resolution confined to
  adapters ([`sdk/src/provider-neutral-route.ts`](../sdk/src/provider-neutral-route.ts),
  [`sdk/src/providers/`](../sdk/src/providers/)).
- Durable coordination mail plus observable live-lane steering
  ([`cli/msg-cli.clj`](../cli/msg-cli.clj),
  [`sdk/src/live-input-route.ts`](../sdk/src/live-input-route.ts)).
- A target-neutral prompt lifecycle rule that appends an interrupted terminal
  only for one uniquely attributable unfinished attempt and preserves ambiguous
  ledgers as typed unknown results
  ([`src/north/prompt_lifecycle.bgl`](../src/north/prompt_lifecycle.bgl)).
- Target-neutral typed cores for scoped memory facts and bounded recall,
  immutable session lineage and compaction-tail projection, ordered replay and
  terminal provenance, and scheduled-run lease reclamation
  ([`memory_core.bgl`](../src/north/memory_core.bgl),
  [`session_core.bgl`](../src/north/session_core.bgl),
  [`replay_core.bgl`](../src/north/replay_core.bgl),
  [`scheduled_run_core.bgl`](../src/north/scheduled_run_core.bgl)).

These are not reimplemented because another harness has a similar surface.
Upstream evidence may sharpen their invariants or reveal a missing seam.

## Ownership matrix

| Concern | North owner | Required implementation shape | Boundary rule |
| --- | --- | --- | --- |
| Work and prompt lifecycle, dependencies, admission, and orchestration semantics | [`src/north/`](../src/north/), including [`prompt_lifecycle.bgl`](../src/north/prompt_lifecycle.bgl) | Target-neutral typed Beagle core | No provider model IDs, host process APIs, or durable side stores |
| Durable memory and learned facts | [`src/north/memory_core.bgl`](../src/north/memory_core.bgl) over the canonical Store RPC selected by [`sdk/src/beagle-store.ts`](../sdk/src/beagle-store.ts) | Store-backed typed facts, supersession, validity, trust, and bounded recall | Files and prompts may be projections, never the system of record |
| Agent and session identity | [`src/north/session_core.bgl`](../src/north/session_core.bgl), [`sdk/src/identity.ts`](../sdk/src/identity.ts), and Store-backed North vocabulary | Beagle Native identity allocation, lineage, and evolution | Provider conversation IDs remain adapter evidence, not North identity |
| Scheduling and admission | [`src/north/scheduled_run_core.bgl`](../src/north/scheduled_run_core.bgl), [`src/north/worker_policy.bclj`](../src/north/worker_policy.bclj), and [`sdk/src/execution-admission.ts`](../sdk/src/execution-admission.ts) | Typed policy and scheduled-run state in Beagle; resident scheduling in Beagle Native | JavaScript may launch admitted work but may not decide provider-neutral policy |
| Replay, run evidence, and provenance | [`src/north/replay_core.bgl`](../src/north/replay_core.bgl), [`sdk/src/execution-fold.ts`](../sdk/src/execution-fold.ts), [`sdk/src/run-ledger.ts`](../sdk/src/run-ledger.ts), and [`sdk/src/run-provenance.ts`](../sdk/src/run-provenance.ts) | Canonical typed event fold, divergence, terminal summary, and Store-backed receipts in Beagle Native | Provider payload decoding stays in the provider adapter |
| Provider CLI and streaming protocol | [`sdk/src/providers/`](../sdk/src/providers/) | Thin Beagle/JavaScript adapter | Fallback is allowed only before observable side effects |
| PTY and terminal UI | [`sdk/src/bridge/`](../sdk/src/bridge/) | Thin Beagle/JavaScript host adapter | Terminal state is a projection of canonical execution state |
| Browser sharing surface | [`sdk/src/run-share-viewer.ts`](../sdk/src/run-share-viewer.ts) | Thin Beagle/JavaScript browser adapter | The viewer receives bounded projections, never Store authority |
| Node-only filesystem, process, and MCP integration | [`sdk/src/`](../sdk/src/) and [`bin/north-mcp`](../bin/north-mcp) | Small JavaScript capability adapters | Capability effects return evidence to the typed core |

The matrix names current paths even where ownership must move inward. A move is
complete only when the Beagle owner is authoritative and the JavaScript path is
an explicit adapter rather than a second implementation.

## Ideas North should adopt

The five audits converge on five mechanisms. Each is owned as an independent
North seam rather than as an imported framework.

- **Prompt lifecycle reconciliation.** Qwen Code separates an append-only
  prompt terminal ledger from workflow snapshots and only repairs unfinished
  work when attribution is unique
  ([design](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/docs/design/2026-08-19-prompt-terminal-ledger-design.md#L30-L88),
  [ledger](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/packages/cli/src/serve/prompt-terminal-ledger.ts#L130-L300),
  [journal](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/packages/core/src/agents/runtime/workflow-journal.ts#L7-L153)).
  North owns the target-neutral rule in
  [`prompt_lifecycle.bgl`](../src/north/prompt_lifecycle.bgl); Store persistence
  and process observation remain Native boundaries.
- **Typed memory with bounded recall.** oh-my-pi models source, scope, trust,
  validity, and supersession
  ([types](https://github.com/can1357/oh-my-pi/blob/72000acfeb902e21816252699482887f34d1a5a4/packages/mnemopi/src/core/beam/types.ts#L106-L218),
  [store behavior](https://github.com/can1357/oh-my-pi/blob/72000acfeb902e21816252699482887f34d1a5a4/packages/mnemopi/src/core/beam/store.ts#L305-L340)),
  while Hermes bounds asynchronous recall and lets recall failure yield no
  injected context rather than block the turn
  ([provider](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/agent/memory_provider.py#L94-L132),
  [manager](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/agent/memory_manager.py#L525-L545)).
  North's owner is a Beagle Native Store fact model and recall decision, never
  a second database.
- **Session lineage and immutable context projection.** Pi validates globally
  sequenced session events, unique IDs, parent links, and leaf chaining, then
  projects context from the latest compaction summary plus its immutable tail
  ([types](https://github.com/earendil-works/pi/blob/b7bb00b936dbe21b8e160b3e89efdec361846699/packages/agent/src/harness/session/types.ts#L87-L159),
  [state](https://github.com/earendil-works/pi/blob/b7bb00b936dbe21b8e160b3e89efdec361846699/packages/agent/src/harness/session/state.ts#L97-L157),
  [context](https://github.com/earendil-works/pi/blob/b7bb00b936dbe21b8e160b3e89efdec361846699/packages/agent/src/harness/session/context.ts#L45-L99)).
  Cline reinforces explicit restore lineage and checkpoint references
  ([session types](https://github.com/cline/cline/blob/1bb0833287a7c1dfbb78d53060611bd1cdb35901/sdk/packages/core/src/types/session.ts#L89-L127),
  [versioning](https://github.com/cline/cline/blob/1bb0833287a7c1dfbb78d53060611bd1cdb35901/sdk/packages/core/src/session/session-versioning-service.ts#L162-L243)).
  North owns these as typed session facts and pure projections, not copied
  provider state.
- **Ordered replay and provenance.** oh-my-pi preserves an append-only stable
  prefix and explicit divergence
  ([context](https://github.com/can1357/oh-my-pi/blob/72000acfeb902e21816252699482887f34d1a5a4/packages/agent/src/append-only-context.ts#L25-L89));
  Qwen Code separates resumable journal evidence from terminal snapshots
  ([snapshot](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/packages/core/src/agents/workflow-snapshot.ts#L35-L141));
  Hermes preserves exact wire content and insertion order independently of
  display metadata
  ([state](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/hermes_state.py#L6512-L6557)).
  North's canonical fold and receipts belong in Beagle Native; provider wire
  decoding stays in adapters.
- **Durable scheduled-run provenance.** Hermes gives scheduled work an origin,
  predecessor-output edge, bounded imported context, and stale-owner recovery
  ([scheduler](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/cron/scheduler.py#L590-L715),
  [identity lock](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/gateway/status.py#L1352-L1515)).
  Cline persists run revisions, leases, receipts, and dedupe state before
  execution
  ([schema](https://github.com/cline/cline/blob/1bb0833287a7c1dfbb78d53060611bd1cdb35901/sdk/packages/core/src/cron/store/cron-schema.ts#L10-L117)).
  North owns these as Store-native scheduler facts; host processes only launch
  or observe admitted work.

## Ideas North should reject

- A JavaScript or Python orchestration core, because host-language objects are
  not North's durable semantic authority.
- Transcript files, Markdown memory, vector indexes, or provider session state
  as a second system of record. They may be observations or projections of
  Store facts.
- Provider-specific model names, prompt dialects, or tool event names in
  provider-neutral scheduling and lifecycle code.
- Shell supervisors and polling loops for identity, scheduling, or liveness.
  Host launch is a capability boundary; durable supervision belongs in Beagle
  Native.
- Compatibility shims for an upstream API or an older North design. Current
  North and its in-tree consumers move together.
- Autonomous effect execution without pre-effect admission and a post-effect
  receipt. A successful process exit is not proof that the intended external
  state exists.

The concrete upstream implementations rejected by those rules include
oh-my-pi's runtime-selected SQLite and compatibility-shaped memory API
([database](https://github.com/can1357/oh-my-pi/blob/72000acfeb902e21816252699482887f34d1a5a4/packages/mnemopi/src/db.ts#L1-L99)),
Qwen Code's filesystem prompt ledger and process-local attachment lease
([sidecar](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/packages/acp-bridge/src/prompt-ledger.ts#L19-L123),
[lease](https://github.com/QwenLM/qwen-code/blob/5f3165f17ea3224a7b982f0c75ae560e8d4aaa39/packages/cli/src/agent-view/attach-lease.ts#L50-L145)),
Pi's authoritative JSONL repair path
([storage](https://github.com/earendil-works/pi/blob/b7bb00b936dbe21b8e160b3e89efdec361846699/packages/agent/src/harness/session/jsonl/storage.ts#L69-L107)),
Hermes's dynamic Python memory and scheduler providers
([memory provider](https://github.com/NousResearch/hermes-agent/blob/b259668cac1cba7faf913c227b9262fe7a513da2/agent/memory_provider.py#L43-L145)),
and Cline's filesystem session manifests and process-local resource limiter
([manifest](https://github.com/cline/cline/blob/1bb0833287a7c1dfbb78d53060611bd1cdb35901/sdk/packages/core/src/session/stores/session-manifest-store.ts#L1-L78),
[limiter](https://github.com/cline/cline/blob/1bb0833287a7c1dfbb78d53060611bd1cdb35901/sdk/packages/core/src/cron/runner/resource-limiter.ts#L1-L45)).

## Adoption rule

An influence moves from research to implementation only when one row can name:

1. the upstream behavior and exact commit-specific source references;
2. the North-owned invariant it improves;
3. one authoritative Beagle or Beagle Native owner;
4. any thin host adapter and its capability envelope; and
5. one focused deterministic check that completes within 60 seconds.

This is the same discipline North applies to provider fallback: the architecture
must decide before side effects become observable.
