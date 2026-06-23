# CLAUDE.md — lodestar

lodestar is the claim-native life/work app on the **fram** engine. This file is the
always-loaded surface: load-bearing rules + thin pointers. Detail lives in what it points to.

## The model in one breath
- **fram** (`~/code/fram`) = the engine. CNF: every fact is a `(subject predicate object)` triple of interned value-ids (subject/predicate/object share ONE flat content-interned id-space — purer than RDF/Datomic); lifecycle is DERIVED from claims, never a stored status.
- **lodestar** = the app: the durable thread/intent ledger served by the canonical coordinator on **:7977** (data `~/code/lodestar-data` → `~/.local/state/lodestar`).
- **One branch, always `main`** (all repos consolidated 2026-06-23 — no feature branches; a pin is a SHA, never a branch).

## 🚩 Driving an agent fleet — use the PROTOCOL, never raw Agent/Workflow
When work means multiple agents, do NOT reach for the host's generic `Agent`/`Workflow`/ultracode spawning. There is a real, running, *better* substrate — persistent, role-based, lease-gated agents that are observable + steerable + durably coordinated:

- **Work queue**: lodestar threads on **:7977** — `ready`/`next`/`leverage` to pick; claim a thread with `driver @agent`.
- **Fleet jurisdiction**: the **:7978** coordinator (`~/code/fleet-data/claims.log`) — presence + **roles** + leases. Separate from :7977.
- **Spawn**: `~/code/fleet-data/spawn-agent.sh <role[,role]>` — mints `@agent:<uuid>` holding lease-gated roles (exclusive role → a 2nd holder self-aborts), event-driven (dormant-until-pinged, ~0 idle tokens). Env: `AGENT_MODEL/EFFORT/LIFECYCLE/SUPERVISOR/…`. Stop: `touch fleet-data/stop-<uuid>`.
- **Address by ROLE, not uuid**: `msg-cli.clj <port> send <from> <role> "<task>"` routes to the holder; a message IS the steer.
- **Observe/steer**: framescope on **:8088** — tails each agent's stream, `/steer`.
- **CLIs**: `~/code/beagle/.scratch/{presence,msg,fleet-listen,lease}-cli.clj`.
- **Recursive teams**: an agent may spawn its own workers via the protocol + coordinate peer-to-peer — **ALWAYS through the protocol, NEVER ultracode/Workflow.**
- **Concurrency lives in the engine** (the DB owns it): write-serialization + OCC + the **lease** primitive (`acquire`/`release`/`fence`) are in fram's `cnf_coord.clj`; apps express coordination as *claims*, never self-rolled locks. (`driver` = app intent; `lease` = DB mutual-exclusion — never conflate them.)

**Org brain**: FLEET PLAYBOOK = lodestar thread **`2026-06-22-232740`** — consult before reaching for tools; append learnings via `lodestar tell 2026-06-22-232740 learning "<finding>"`.
**Canonical how-to**: `~/code/fleet-data/RUNBOOK.md`.

## Write safely (claim-backed, concurrent agents)
- Session start: `lodestar doctor` → `lodestar up` if down.
- New work: `lodestar capture` — coordinator-native (asserts through the daemon, renders the `.md` FROM the log; no file-first stranding, no driver-at-birth).
- Field changes: `lodestar tell`/`untell` (serialized, rule-checked) — **never `lodestar set`** (races the log).
- **Never `lodestar export` under concurrent work** (`import` is idempotent/safe). The log is the source of truth; thread `.md` files are a regenerable projection — `doctor` distinguishes benign log-ahead lag from a real file-ahead conflict.

## Pointers
- `~/code/fleet-data/RUNBOOK.md` — fleet operating runbook (spawn/assign/steer/supervise).
- lodestar thread `2026-06-23-132319` — CNF purity + lodestar-as-client architecture.
- `~/code/fleet-consolidation-runbook.md` — engine/app/data seam-cut; remaining: the `:7978` daemon swap onto canonical-fram-with-lease (human/sudo step).
- `~/code/fram` — the engine (claim model, coordinator, lease primitive).
