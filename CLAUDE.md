# CLAUDE.md — north

north is the fact-native life/work app on the **fram** engine. This file is the
always-loaded surface: load-bearing rules + thin pointers. Detail lives in what it points to.

## The model in one breath
- **fram** (`~/code/fram/main`) = the engine. Store layer: every fact is a `(subject predicate object)` triple of interned value-ids (subject/predicate/object share ONE flat content-interned id-space — purer than RDF/Datomic); lifecycle is DERIVED from facts, never a stored status.
- **north** = the app: the durable thread/intent ledger served by the canonical coordinator on **:7977** (data `~/code/north-data` → `~/.local/state/north`).
- **One branch, always `main`** (all repos consolidated 2026-06-23 — no feature branches; a pin is a SHA, never a branch).

## Execution model — two-speed

- **`north` / `north-mcp` run LIVE from this checkout** (`~/code/north/main`): an
  edit takes effect on the next invocation — no rebuild, no restart.
  `north-packaged` / `north-mcp-packaged` are the generation-pinned escape
  hatches when the checkout is broken.
- **The fram coordinator daemon never runs working-tree code.** It runs an
  exact git COMMIT you deliberately select: `north-coord-runtime promote
  ~/code/fram/main <rev>` checks that SHA into a deployment dir named by the
  hash, composes its classpath (deployment code first, store jars after —
  tools.deps at boot is a hard error, never a fallback), and a service restart
  adopts it. Rollback = promote the previous SHA. `north doctor` prints the
  running SHA.
- **Rebuilds are for system config only**: no local input auto-promotes at
  `firn rebuild` any more. Fram's adoption verb is `north-coord-runtime
  promote`; north and beagle are dev-channel and never trigger a rebuild.
- Provider **guards** (agent-spawn/tripwire/clock-guard, harness-dial,
  registry, and Beagle's Codex hooks) adopt through
  `north-enforcement-promote <rev> --why …`: a sealed root-owned 0444
  deployment under `/var/lib/north-enforcement`, attested by a promote record
  and reached through the generation's stable /etc names. Enforcement still
  never runs from a mutable tree — the tree it runs from is sealed by promote
  instead of by the Nix store. The lifecycle runtimes
  (north-on-spawn/-tooluse/-stop, north-mark-delegated) stay generation-pinned:
  they execute out of the North package that supplies their PATH and
  `NORTH_HOME`. Rollback = `north-enforcement-promote rollback --why …`.
- Full detail: `nixos-config:docs/north-delivery-mode.md`.

## Agent dispatch — SDK + thread-driven posture

Agent coordination uses the **TypeScript SDK** (`~/code/north/main/sdk/`), not bash scripts.

- **Dispatch**: `bun run ~/code/north/main/sdk/src/dispatch.ts <thread-id>` — reads thread facts, derives posture (unplanned/atomic/composite), injects the right prompt + tool set, and records the run stream.
- **Spawn**: `bun run ~/code/north/main/sdk/src/spawn.ts <prompt>` — direct agent spawn with SDK `query()`.
- **Parallel**: `spawnParallel()` in `~/code/north/main/sdk/src/spawn.ts` — `Promise.all` over multiple agents.
- **Work queue**: north threads on **:7977** — `ready`/`next`/`leverage` to pick; acquire a thread with `driver @agent`.
- **Observe/steer**: `north agents`, `north show`, and `north steer` over the coordination CLI.
- **Concurrency lives in the engine** (the DB owns it): write-serialization + OCC + the **lease** primitive in fram's `coord.clj`.

## Write safely (fact-backed, concurrent agents)
- Session start: `north doctor` → `north up` if down.
- New work: `north capture` — coordinator-native (asserts through the daemon, renders the `.md` FROM the log; no file-first stranding, no driver-at-birth).
- Field changes: `north tell`/`untell` (serialized, rule-checked) — **never `north set`** (races the log).
- **Never `north export` under concurrent work** (`import` is idempotent/safe). The log is the source of truth; thread `.md` files are a regenerable projection — `doctor` distinguishes benign log-ahead lag from a real file-ahead conflict.

## Pointers
- north thread `2026-06-23-132319` — store-layer purity + north-as-client architecture.
- `~/code/fram/main` — the engine (fact model, coordinator, lease primitive).

## The agent-profile contract

`north:agent-profile` is a **stable symlink** to whatever directory currently
holds the personal agent profile (today `profiles/tom`). It exists so that
consumers outside this repo — firn's `north-profile` module, which materialises
`~/.agents/*` and the root-owned `/etc/codex` projection — never encode north's
internal layout. Reorganise `profiles/` freely; repoint this one symlink and
nothing downstream changes. Do not delete it, and do not have outside repos
reference `profiles/…` directly.
