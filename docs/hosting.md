# Hosting Lodestar

Lodestar runs three ways off **one architecture** — your laptop, a server you own,
or a multi-tenant service you host for others. There is no fork in the design; the
only thing that changes between modes is the **transport in front of the
coordinator socket**. This doc covers all three and is honest about what's built,
what's an MVP, and what's still ahead.

## The architecture (recap)

```
threads/*.md ──import──▶ claims.log (append-only) ──fold──▶ in-memory graph
                                                              │
                              coordinator daemon  ◀── clients query + assert
                              (sole writer, 127.0.0.1)        │
                                                   consumer (Lodestar) derives
                                                   ready / blocked / leverage / clock
```

- **Truth** is an append-only `claims.log` of `(subject, predicate, object)` triples.
- **The coordinator** is a single-writer babashka daemon: it folds the log into an
  in-memory graph and serves `query`/`assert`/`retract` over a **loopback** socket
  with optimistic concurrency and commit-time rule checks. It binds `127.0.0.1`
  and is **unauthenticated by design**.
- **Lodestar** is the life domain on top (lifecycle projections, clock, billing).
- **Runtime dependency is just [babashka](https://babashka.org)** — the compiled
  Clojure is committed in both repos (`out/`); Beagle is only needed to rebuild.

Everything below is about what sits *in front of* that loopback socket.

---

## Mode 1 — Self-host, single machine (works today)

The default. One operator, one box, the coordinator on `127.0.0.1`.

```sh
git clone https://github.com/tompassarelli/fram     ~/code/fram
git clone https://github.com/tompassarelli/lodestar ~/code/lodestar
~/code/lodestar/bin/lodestar up        # start the coordinator (idempotent)
~/code/lodestar/bin/lodestar ready     # use it
```

No build step (runs on the committed `out/`), no network exposure, no auth needed.

## Mode 2 — Self-host, remote box you own

Same as Mode 1 on a server, with one of:

- **SSH tunnel (zero new components):** run the coordinator on the box; forward the
  loopback port to your workstation: `ssh -L 7977:127.0.0.1:7977 you@box`. Your
  local CLI/MCP talks to `127.0.0.1:7977` as if local. Good for one person.
- **The auth gateway (for non-SSH clients / an AI over HTTP):** put `deploy/gateway`
  in front (bearer token → your single tenant → the coordinator), TLS via a reverse
  proxy. This is the same component the SaaS mode uses, with one tenant.

## Mode 3 — Multi-tenant SaaS (you host for others)

**Model: instance-per-tenant.** Each account gets its own coordinator + its own
`claims.log`. An authenticated gateway routes each request to the right one.

```
                         ┌─ coordinator(acme)   + acme/claims.log
client ─HTTPS─▶ proxy ─▶ gateway ─┼─ coordinator(globex) + globex/claims.log
              (TLS)   (authn +    └─ coordinator(…)      + …/claims.log
                       tenant route)
```

Build the image, provision tenants, run it — see `deploy/README.md` and
`deploy/docker-compose.example.yml`. Provisioning mints a bearer token, starts the
tenant's coordinator, and registers it; the gateway authenticates and forwards.

### Why instance-per-tenant (not one shared graph)

- **Isolation is the only safe boundary.** The per-assertion `frame` records *who
  asserted* a claim — it's provenance, not authorization. A shared graph
  partitioned by frame would let any authenticated caller assert any frame. So
  tenancy = **separate logs + separate coordinators**, full stop.
- **The architecture makes it cheap.** A tenant is *already* just "a log + a
  coordinator on a port." Instance-per-tenant falls out for free — no shared-state
  redesign, no distributed consensus.
- **Right-sized safety.** Single-writer + optimistic concurrency is exactly the
  model you want *per tenant*; you run N small independent authorities, not one big
  contended one.
- **Scale.** Each personal graph is small and its writes serialize through one warm
  process (µs commits). You scale by adding tenant instances across hosts, not by
  scaling one graph. Very large *single* tenants would eventually swap the
  in-memory-fold + flat-log for a transactional store (XTDB/Datomic/Datahike) — the
  claim model is unchanged; only the substrate underneath swaps.

---

## Security model

- The raw coordinator protocol is **unauthenticated** — it must never be exposed.
  Only the gateway is reachable, and only behind TLS.
- The gateway authenticates a **bearer token**, stored **hashed** (sha-256) in the
  registry; it maps the token to exactly one tenant's coordinator.
- Coordinators bind loopback; the gateway forwards over loopback (same host / shared
  netns). Cross-host forwarding needs a configurable bind + mTLS (roadmap below).
- Plain-text data, no telemetry, `export` is claim-identical — the leave-anytime
  guarantee holds in every mode, including hosted.

## Operations

- **Backups:** each tenant's `claims.log` is append-only plain text — back it up
  with `git`/snapshots/object storage. `import`/`export` round-trips are lossless.
- **Supervision:** systemd template unit per tenant (`lodestar-coordinator@<id>`)
  + the gateway unit; both `Restart=on-failure`. Or one container per coordinator.
- **Upgrades:** pull the repos (or a new image tag) and restart; the log format is
  stable and forward-only.
- **Health:** gateway `GET /healthz`; coordinator liveness via `lodestar doctor` /
  an `{:op :status}` RPC.

---

## Status — built vs. MVP vs. planned

| Capability | State |
|---|---|
| Claim log, fold, Datalog derivation, single-writer coordinator | **built** (Fram, tested) |
| Lifecycle/clock/billing projections | **built** (Lodestar, tested) |
| Runs on bare babashka, no build step | **built** (`out/` committed) |
| Single-machine + SSH-tunnel remote | **built** |
| Authenticated gateway (bearer → tenant → coordinator), per-tenant isolation | **built** (`deploy/gateway`, smoke-tested in CI) |
| Token rotation/revocation, audit logging, rate limit + body cap | **built** (gateway; covered by the smoke test) |
| Tenant provisioning + systemd/Docker/compose | **built** (`deploy/`) |
| TLS termination | **built** example (`deploy/Caddyfile.example`) — delegated to a reverse proxy |
| Per-tenant backups (snapshot + prune, timer) | **built** (`deploy/backup.sh` + `lodestar-backup.{service,timer}`) |
| Cross-host coordinators (configurable bind + mTLS) | **planned** — gateway side ready (`:coordinator-host`); needs the Fram change below |
| Control plane (provisioning API, quotas, key mgmt beyond a file) | **planned** |
| Self-service signup, billing, web client | **planned** (product layer) |
| Transactional store swap for very large single tenants | **planned** (model-stable) |

## Roadmap (in dependency order)

1. ~~**Harden the gateway:** token rotation/revocation, request caps, audit logging.~~
   **Done** — `deploy/gateway`.
2. **Configurable coordinator bind + mTLS** so coordinators can live on separate
   hosts behind the gateway (lifts the same-host constraint; keeps loopback default).
   The gateway already forwards to `:coordinator-host`; the remaining piece is a
   one-line Fram change (spec below).
3. **Control plane:** provisioning/lifecycle API, per-tenant daemon supervision,
   quotas, key management beyond a flat registry.
4. **Product layer:** self-service signup, billing, web client, teams.
5. **Scale path:** transactional store option for outsized single tenants.

None of this is a foundation rewrite — it's the product layer the
[proposal](PROPOSAL.md) (Phases 4–5) already mapped.

### Drop-in spec: configurable coordinator bind (Fram)

The only thing blocking cross-host coordinators is that the daemon binds loopback
unconditionally. When Fram is ready for it, this change (kept loopback-default, so
nothing regresses) enables it — in `cnf_coord_daemon.clj`'s `serve`:

```clojure
;; default loopback; honor FRAM_BIND for behind-the-gateway deployment
(defn- bind-addr []
  (let [b (System/getenv "FRAM_BIND")]
    (if (or (nil? b) (#{"" "loopback" "127.0.0.1"} b))
      (java.net.InetAddress/getLoopbackAddress)
      (java.net.InetAddress/getByName b))))
;; in serve: bind `(bind-addr)` instead of getLoopbackAddress, and when it is NOT
;; a loopback address, log a WARNING that the protocol is unauthenticated and MUST
;; sit behind the gateway / a firewall.
```

Then a per-tenant coordinator can bind a private interface, the gateway reaches it
via `:coordinator-host`, and the raw port is still never publicly exposed. (Left
unapplied here on purpose — Fram is under active development; this is ready to drop
in once it settles.)
