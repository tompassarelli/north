# streams/ — lossless capture + tiered distillation

The stream layer the operating manual describes. Two directories:

- `streams/raw/` — **lossless transmission events**: full session transcripts
  (Claude Code and Codex JSONL), dictated thoughts, captured conversations.
  **Local-only, gitignored** — raw transcripts carry everything (private
  context, tool output); the repo publishes projections, not the source signal. Files:
  `YYYY-MM-DD-<bounded-slug>.<lineage-digest>.jsonl`. The lineage digest binds
  provider + source authority + root-relative lineage; a provider session UUID
  is not assumed globally unique. A copy is a live mirror — sessions keep
  appending; the durable byte cursor advances on the next sweep.
- `streams/distillations/` — **committed tiered compressions** of raw streams.
  Tier 1 = one session → decisions, principles, spawned threads, artifacts,
  with `@thread-id` links so the coordination graph and the narrative cross-reference.
  Files: `YYYY-MM-DD-<slug>.<raw-lineage-digest>.tier1.md`; the full lineage
  digest is the write-once identity across crash recovery and retries.

Provenance contract: every distillation names its raw source(s) and the north
thread minted for the session (`stream thread`), which carries `relates_to`
edges to every thread the conversation spawned. Chain: utterance → distillation
→ stream thread → spawned thread → outcome fact → commit. Queryable end to end.

Mining (retry loops, verb votes, doc re-reads) is `north-mine`'s job, not this
layer's — raw here is its input corpus.

## Cost contract — this layer is nearly free; keep it that way

- **Raw capture = byte copy, zero tokens.** Claude Code writes
  `projects/<proj>/<session>.jsonl`; Codex writes
  `sessions/YYYY/MM/DD/rollout-<session>.jsonl`. `north-stream-sync-all`
  discovers ambient provider homes, provider accounts, profiles, and managed
  Codex launches. All land in this same raw directory. Each explicit provider
  authority owns scoped `.cursors.v4.*` state and a stable raw destination.
  Never have a model regenerate conversation text; mirror the file the harness
  already wrote.
- **Managed Codex retention is mirror-gated.** A settled launch home is prunable
  only after its rollout and `north-launch.json` receipt have been copied here
  and `north-stream-mirrored` has been written back to the launch home.
  Unacknowledged homes may exceed the normal retention bound rather than lose
  the only transcript during a burst between timer sweeps.
- **Distillation = cheap-tier agent** (sonnet-worker / haiku), never the
  coordinator model. Exception: if the coordinator already holds the whole
  session in context at session end, its ~1k-token summary is cheaper than a
  fresh agent re-parsing megabytes of JSONL — allowed, but that's the only case.
- **Coordinator's only job**: mint the stream thread + `relates_to` edges
  (a handful of facts).
- **Tier 1 claims are explicit and project-scoped.** `north stream-distill`
  accepts a coordinator-minted stream thread, matches the raw session's exact
  repository identity, and claims one settled managed mirror. The model sees
  the static distiller contract plus that claimed snapshot as untrusted task
  data. Global machine law and the declared routing contract remain explicit,
  receipt-attested policy; ambient memory, skill catalogs, and activated
  resources do not enter the task context. North publishes provenance and marks
  completion only after the write-once artifact is durable. A replaced claim
  token makes late model output stale.
