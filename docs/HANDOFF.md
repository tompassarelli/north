# Lodestar — Handoff (single source of truth)

**Status: PAUSED at a clean checkpoint.** Repo is public (`tompassarelli/lodestar`),
CI is green on `main` (HEAD `0289555`, "multi-host: complete the cross-host last-mile"),
the working tree is clean, and `out/` is committed (runs on bare babashka). There are
**zero git tags / GitHub releases** by deliberate policy. This is not work-in-flight that
stalled; it is a deliberate stop. The Lodestar agent is standing down and hands its
remaining work to the Fram orchestrator. **The next real motion is gated on a human
product decision (dogfood-vs-SaaS), not on code** — see Open decisions and Resume triggers.

---

## Done & verified

Built **and** test-/CI-verified (CI runs `clock_test`, `staleness_test`,
`cnf_lifecycle_test` on the committed `out/`, plus `smoke_test.sh` and `crosshost_test.sh`):

**Auth gateway & hosted plumbing** (`deploy/gateway/gateway.clj`)
- **Bearer-token auth → tenant → coordinator.** `POST /v1/rpc` requires `Authorization: Bearer <token>`; the token (sha256) maps to a tenant via the registry and forwards one EDN line to that tenant's coordinator over a socket (`coord-rpc`). *Verified:* smoke test "authed /v1/rpc reaches coordinator" gets a `:version` reply; bad/missing token → 401.
- **Tokens stored HASHED, never plaintext** (sha256-hex). Registry holds only hashes; `provision.sh` mints + hashes and echoes the plaintext exactly once. tenants.edn is gitignored.
- **Rotation + revocation with a no-downtime grace window.** `:tokens` is a set (unioned with the legacy single `:token-sha256`); registry re-read on mtime change → rotate/revoke take effect with **no gateway restart**. *Verified:* smoke step 7 empties the token set, waits for reload, confirms the old token now 401s.
- **Audit logging that never logs the object value (`:r`).** One EDN line per request (`:event :tenant :op :te :p :status :remote :ts`); unauthorized attempts also audited. *Verified:* smoke step 6 greps the audit log.
- **Body-size cap** (default 64 KB, bounded read regardless of a lying Content-Length → 413; malformed/non-map/non-keyword-`:op` → 400; coordinator unreachable → 502). *Verified:* smoke step 5 with `GATEWAY_MAX_BODY=64`.
- **Rate limiting** — per-tenant token bucket → 429 (audited). *Built, code path present, but the threshold is NOT exercised by a test* (nothing floods to trigger 429).

**Topology / isolation**
- **Instance-per-tenant** — one coordinator + one `claims.log` per tenant (`provision.sh`, registry shape, `hosting.md`).
- **Loopback (single-host) path** — *Verified:* `smoke_test.sh` stands up a real Fram coordinator + gateway on loopback, all 7 checks pass in CI.
- **Cross-host path** — coordinator with `FRAM_BIND=0.0.0.0` + registry `:coordinator-host` pointing off-loopback. *Verified:* `crosshost_test.sh` (3/3) binds non-loopback (asserts via `ss`), asserts the coordinator logs an UNAUTHENTICATED warning, confirms the gateway forwards over the non-loopback host. The cross-host last-mile is LANDED (`c713dea`/`0289555`).

**Examples / units (present, reviewed — NOT test-verified):** TLS termination via `deploy/Caddyfile.example` (HTTPS, body cap, security headers); systemd `lodestar-coordinator@.service` + `lodestar-gateway.service` (hardened, unprivileged); backups (`backup.sh` + `lodestar-backup.{service,timer}`, append-only log → torn-free copy without quiescing); Docker (single image, two roles; `docker-compose.example.yml` publishes only the gateway port). Treat these as reviewed examples, not verified.

**Local life-verb surfaces (built, predate the hosted-product design):**
- **CLI life verbs** (`src/lodestar/main.bclj`) — `ready/blocked/leverage/next/agenda/plate/needs-review/audit/doctor/capture` + the `clock` family. Reads fold the tenant log off disk and project via `lodestar.projections`; writes (`capture`, `clock_*`) go through the coordinator with tell-retry on conflict. The life verbs live in `lodestar.main`, NOT the engine.
- **CLI wrapper routing** (`bin/lodestar`) — life verbs → `bb -m lodestar.main`; `up` → fram-up; everything else → the fram CLI. Supplies life-domain env incl. `FRAM_SINGLE_VALUED` and capture provenance.
- **Local stdio MCP** (`bin/lodestar-mcp`) — working JSON-RPC-2.0-over-stdio MCP, serverInfo `{name "lodestar", version "0.1.0"}`, 17 life-verb tools shelling the `lodestar` wrapper. **stdio only — no HTTP/SSE.**
- **Fram engine MCP** (`fram/bin/fram-mcp`) — sibling at engine altitude, serverInfo `{name "fram"}`, vocabulary-generated tools + structured `query` + structural tools. **No life verbs.**

> Honesty note: the local CLI + stdio MCP shipped for other reasons. The **hosted product surface** (remote transport, HTTP verb routes, warm head) is **100% unbuilt**. Do not conflate "the life verbs exist" with "the hosted product exists." The only hosted routes that exist are `/healthz` and `/v1/rpc` (a raw EDN-op relay — it does NOT proxy life verbs and was never meant to).

---

## Pending work, sequenced

Ordered by dependency + leverage. **Everything below the line is gated on the BLOCKING
product decision** (SaaS vs dogfood) — see Open decisions.

### A. Product surface — exposing life verbs to remote/hosted clients
Source: `docs/product-surface-design.md` (STATUS: DESIGN — not built; deliberately sequenced behind Fram's churn).

1. **Phase 0 — coordinate the seam** (design artifact only, no code). The §4 boundary
   (Fram MCP = neutral engine verbs; Lodestar MCP = life verbs; siblings by altitude,
   neither proxies the other) must be **ratified with the Fram agent** before any Phase 1
   code. *Not Fram-coupled as code, but REQUIRES Fram coordination.* Unblocks 2.
2. **Phase 1 — MCP-over-HTTP per tenant, shell-out base.** Serve the existing
   `bin/lodestar-mcp` tool surface over Streamable HTTP / SSE, fronted by the gateway for
   auth + tenant routing (reuse the existing token→tenant registry — one auth system).
   Verbs implemented by shelling the tenant's `lodestar` CLI per request with that tenant's
   env. *NOT Fram-coupled* (only indirect: it spawns the CLI). Cost: babashka cold-start +
   full log re-fold per call — fine at personal/MVP traffic, visibly bad for chatty fan-out.
   Unblocked by Phase 0.
   - **(Optional, additive) Web-app JSON routes** — parallel `GET /v1/verb/*` returning the
     `JThread/JClockReport/...` records the CLI already emits, same shell-out path. Add if/when needed.
3. **Phase 2 — warm per-tenant Lodestar head.** A long-lived per-tenant process holding the
   folded graph in memory, serving life verbs over a socket, subscribing to the coordinator's
   `{:op :subscribe}` stream to stay fresh. Slots UNDER the same MCP/HTTP edge as a
   non-breaking perf swap. Reuse `cmd-doctor`'s freshness/staleness logic as the staleness
   guard. **Still NOT a second writer — writes funnel to the coordinator.** **Fram-COUPLED
   (heaviest):** it embeds the `fram.kernel/fold/rt` fold loop as a resident server, the most
   exposed to Fram library-API churn. **Build LAST, against a pinned Fram, after Fram's churn
   (incl. its new engine MCP/query surface) settles.**
4. **Phase 3 — product polish.** Per-tenant head systemd unit alongside the coordinator unit;
   quotas/observability on the life-verb edge; control-plane hooks. After Phase 2.

### B. Cross-host security — mTLS gateway ↔ coordinator
5. **mTLS between gateway and coordinator for untrusted links** — ✅ **Fram ASSESSED
   (2026-06-17); now downstream-only, NOT Fram-coupled.** The coordinator protocol is
   UNAUTHENTICATED, so a non-loopback link still needs a TLS boundary — but Fram found
   that babashka's native image cannot terminate TLS server-side (no `SSLServerSocket`),
   so engine-terminated mTLS is **specced-and-deferred** (would require running only the
   daemon under JVM Clojure). The **supported transport is an stunnel mTLS sidecar** —
   `fram/deploy/stunnel.example.conf` — coordinator stays loopback, stunnel provides
   mutual TLS, the wire protocol is byte-identical, **zero Fram code**. So Lodestar's
   remaining work is just to wire stunnel into its compose/deploy (downstream, no Fram
   coupling). See `fram/docs/coordinator-bind-and-wire.md` §"Securing a non-loopback bind".

### C. Control plane & scale (later, hosting.md roadmap items 3–6)
6. **Control plane** — provisioning/lifecycle API, per-tenant daemon supervision/orchestration,
   quotas, key management beyond the flat EDN registry. Today: manual `provision.sh` + systemd
   template + hand-edited `tenants.edn`; no API, no quotas beyond the per-tenant rate limit.
   *Not Fram-coupled.*
7. **Product layer** — self-service signup, billing, web client, teams. *Not Fram-coupled.*
   (PROPOSAL.md Phase 5; design-gated.)
8. **Transactional store swap** for very large single tenants (XTDB/Datomic/Datahike) —
   model-stable; current substrate is in-memory fold + flat append-only log per tenant.
   **Engine-side (Fram) concern.**

---

## Open decisions (need the human)

1. **SaaS-vs-dogfood — `BLOCKING`. This gates the entire product surface (Section A) and
   most of B/C.** PROPOSAL.md §15 #5 "Hosting posture — self-host-first (assumed) vs SaaS-first";
   Phase 5 is gated behind a human call that single-user dogfood is "undeniably better than
   Linear" (§11/§13). If the target is personal dogfood, the local stdio MCP **already**
   delivers the full life-verb surface and **none of Phases 1–3 are needed**; the gateway's
   raw-op proxy is the only hosted plumbing and the "gap" is moot. Nothing in the repo commits
   to which. **A human must pick before any Phase 1 work is justified.**
2. **No-releases policy — LIVE and intentional.** No git tags / GitHub releases until there is
   a usable product. `v0.1.0` was already created-then-deleted once (CI ran against it
   2026-06-17, ref now exists in neither local nor remote). Public repo + green CI does NOT
   mean release-ready. **Do not tag a release.** (Aside: `bin/lodestar-mcp:121` hardcodes
   serverInfo `:version "0.1.0"` — an MCP handshake field, not a git release; there is no
   source-of-truth version file.)
3. **Engine-MCP vs life-MCP seam (§4 default product posture).** Recommendation: expose the
   Lodestar/life MCP as the everyday "run my life" client; reserve the Fram engine MCP for
   graph-level tooling; advertise as distinct servers (`fram`=engine, `lodestar`=life) so tool
   names don't collide. **Design-agreed in the doc but flagged "coordinate with Fram before
   building" — an open cross-repo agreement, not a settled fact.** The two agents must ratify it.
4. **Lower-priority (PROPOSAL.md §15, all deferred):** multi-tenant store choice (#2);
   per-predicate conflict policy beyond the starter set (#3); the value model
   (`value_financial`/`value_joy`, Pareto-ranked not scalarized — this is what would turn the
   agenda from a "ready set" into a ranked happy path) (#4); Beagle vs dropping to Clojure for
   the coordinator if Beagle blocks velocity (#6); product **name** ("Lodestar" is a working
   codename, rename-at-review).

> Note: `docs/claim-native-redesign.md` is **SHIPPED (historical)** — its Q1–Q6 "open
> decisions" are CLOSED. Do not re-open them. The live open decisions are the PROPOSAL.md §15
> set above. Read each doc's header (SHIPPED vs DESIGN-not-built) before treating any "open
> decision" as actionable.

---

## The engine ↔ app seam — do NOT re-complect

Fram (engine, `~/code/fram`) and Lodestar (app, this repo) are **separate repos with a
pinned-SHA seam by design.** Keep them decoupled.

- **Fram owns:** the domain-neutral claim substrate; the coordinator; `FRAM_BIND` and its
  default-loopback security invariant; the line-delimited EDN wire protocol
  (`:version/:status/:validate/:assert/:retract` + the `:subscribe` event stream); and its own
  engine MCP (vocabulary-generated tools + structured `query`, NO lifecycle/clock/presentation).
- **Lodestar owns:** lifecycle projections (`lodestar.projections`) and the life verbs
  (`lodestar.main`); the gateway + tenancy/auth (`deploy/gateway/gateway.clj`); and the life
  MCP. Lodestar's `tell/untell/capture` are opinionated, provenance-stamped writes — NOT bare
  engine asserts re-branded.
- **Boundary rules (hold jointly, §4):** (1) no life verbs in the Fram MCP; (2) no bare neutral
  assert/retract re-exposed as a "Lodestar" tool; (3) neither MCP proxies the other (each
  reaches the coordinator directly); (4) advertise as distinct servers.
- **Contract-of-record:** Fram's `docs/coordinator-bind-and-wire.md` (the wire protocol, the
  `FRAM_BIND` invariant, topology). If Fram changes `:assert/:retract/:subscribe/:status/:version`
  it is a **breaking change** for the gateway AND any future warm head — must be pinned/versioned jointly.

**Pin:** the SHA lives in the `FRAM_VERSION` file — **the single source of truth** (don't
duplicate it in prose, it drifts). CI (`.github/workflows/ci.yml`) reads that file; the
`Dockerfile` (`ARG FRAM_REF`) carries the same SHA and is **not auto-synced — bump both.** Lodestar links
Fram's library API in-process (`fram.clock/export/fold/import/json/kernel/rt`; `projections.bclj`
does `(require fram.kernel :as k)` calling `k/one-i`, `k/terminal-i?`), so a Fram library change
can break the committed `out/` even if the wire protocol is untouched. **That is why we pin
instead of tracking main.** Bump procedure is **verify-then-pin, not pin-then-test:** check out
the candidate Fram, `./build.sh`, re-run the tests against the committed `out/`, *then* edit the
SHA. (Build vs run: running needs only babashka + the Fram checkout; **rebuilding `out/` from
`src/lodestar/*.bclj` additionally needs Beagle** and links gitignored `src/fram` via `build.sh`.)

---

## Key joint task: kernel genericization

The decomplecting is **partly done.** Lodestar already injects the single-valued cardinality
vocab — `bin/lodestar:11` exports `FRAM_SINGLE_VALUED="title owner lead driver source part_of
do_on valid_until estimate_hours created_at updated_at name body created_by committed outcome
abandoned superseded_by merged_into session_of start_time end_time clockify_id"` — and the
kernel consumes it (`fram/src/fram/kernel.bclj:18-25`). The kernel's hard-coded list is an
explicitly **transitional back-compat fallback** that "goes away once every caller sets the
env." Lodestar **is** that caller, so the moment Fram deletes the fallback, nothing breaks.

**What still lives baked into Fram's kernel LOGIC** (as of the pinned SHA):
- `terminal?`/`terminal-i?` hard-code the predicate strings `"outcome"` and `"abandoned"`
  (`kernel.bclj:65, 199`).
- the validate path hard-codes `"depends_on"` edge rules + the "points at abandoned" check
  (`kernel.bclj:121-133, 227-238`).
- `export.bclj:17-27` bakes an ordered field-vocab list incl. `committed/outcome/abandoned/depends_on`.

**The joint change (Fram parameterizes, Lodestar declares):** Fram keeps the derivation
**machinery** (`one`/`many`/`terminal?`/`cycle?`/`validate`) but takes the predicate **names**
as injected config. Lodestar then DECLARES: "done" = `outcome`, "canceled" = `abandoned`,
"commitment" = `committed`, "blocked-edge" = `depends_on`, "active" = `driver`. The vocab
knowledge already lives app-side (`projections.bclj` references these names), but today it
RELIES on Fram's kernel applying terminal/blocked semantics.

**Depends on a Fram release that** (i) deletes the transitional single-valued fallback, and
(ii) exposes an injection point for the lifecycle-derivation predicate names. **Until Fram
provides that injection point, Lodestar cannot complete its half.** Fram-coupled.

**Open sub-decision (drives exactly what Lodestar declares):** *where* the lifecycle vocab gets
injected — a new env var parallel to `FRAM_SINGLE_VALUED` (e.g. `FRAM_TERMINAL_PREDICATES`),
vs a config map asserted into the graph (the `@ui`/`emoji_*` precedent), vs passing predicate
names as args into `fram.kernel` functions. Also undecided: whether `export.bclj`'s
field-ordering vocab is "engine" or "app". A human/the two agents must pick the seam shape.

---

## Resume triggers

Spin a Lodestar session back up when any of these become true:

1. **The human makes the SaaS-vs-dogfood call (BLOCKING).** If **SaaS** → ratify the §4 MCP
   seam with Fram (Phase 0), then build Phase 1 (MCP-over-HTTP + gateway routing, not
   Fram-coupled). If **dogfood** → most of Section A is moot; resume only for targeted polish.
2. **Fram ships kernel genericization** — i.e. deletes the single-valued fallback AND exposes
   an injection point for the lifecycle predicate names. Then Lodestar declares its
   lifecycle vocab (after the human picks the injection seam shape).
3. **Fram changes the wire protocol** (`:assert/:retract/:subscribe/:status/:version`) or a
   linked library API (`fram.kernel/fold/rt/...`) — breaking for the gateway and the committed
   `out/`; requires a verify-then-pin bump of `FRAM_VERSION` (+ `Dockerfile ARG FRAM_REF`).
4. **Need to safely expose cross-host over an untrusted link** → build mTLS (Section B),
   which needs a coordinating Fram change first.
5. **A usable product is reached and the human approves shipping** → only then revisit the
   no-releases policy.

### Write-safety for any orchestrator touching a live store
- **Never run `lodestar export` during concurrent work** (clobbers un-imported edits).
- Serialized writes go through the coordinator via `lodestar tell/untell` — **never
  `lodestar set`** (races the log). New threads: `lodestar capture` or file-edit + `import`.
- **Never publicly expose a raw coordinator port** — it is UNAUTHENTICATED. Only the gateway is
  reachable, and only behind a TLS-terminating reverse proxy on loopback. The gateway itself
  binds `0.0.0.0` and speaks plain HTTP by design.
- `FRAM_BIND` defaults to loopback; opt into `0.0.0.0` only on a trusted private network until
  mTLS lands.
