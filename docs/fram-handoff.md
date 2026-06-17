# Hand-off → Fram: configurable coordinator bind for gateway-fronted deployment

> ## ✅ LANDED in Fram (2026-06-17)
>
> The Fram agent shipped this. **`FRAM_BIND`** is configurable in
> `cnf_coord_daemon.clj` — **default loopback** (existing users unchanged);
> set **`FRAM_BIND=0.0.0.0`** behind the gateway (binds all interfaces *incl.
> loopback*, so local CLI/doctor keep working; logs a one-time UNAUTHENTICATED
> warning to stderr). Wire protocol unchanged. Contract-of-record now lives in
> Fram at `docs/coordinator-bind-and-wire.md`. Verified: Fram's `bind_test.clj`
> (both modes) + **this repo's gateway smoke test passes 7/7** against the new build.
>
> **Lodestar is unblocked.** Remaining cross-host steps are downstream-only (see
> "What Lodestar does after Fram lands this" below): bridge-network compose with
> `FRAM_BIND=0.0.0.0` + `:coordinator-host` service names, the cross-host gateway
> integration test, and flipping `docs/hosting.md` to "multi-host" — which is also
> where this doc + its `hosting.md` link should finally be removed.

**This is a hand-off document for the Fram engine repo.** It captures the one
engine-side change that the Lodestar multi-tenant work depends on, plus the
contract that keeps the two repos decoupled. Move/copy it into Fram; delete it
from Lodestar once Fram has absorbed the work.

> **Why this exists:** the cross-host coordinator bind is a *Fram* concern (it's
> the engine's socket), but it surfaced while building Lodestar's gateway. Rather
> than have a Lodestar agent reach into the engine (complecting the two), this
> hands the engine-side work to the Fram agent with everything needed to finish it
> independently.

---

## The seam (who owns what)

The two repos meet at **exactly one interface: the coordinator's wire protocol.**
Keep all other concerns on their own side.

| Concern | Owner |
|---|---|
| Claim kernel, fold, Datalog, structural integrity (`validate`) | **Fram** |
| Coordinator daemon: socket, **bind address**, wire protocol, sole-writer lock | **Fram** |
| Authentication, tenant routing, rate limit, audit, body caps | **Lodestar** (the gateway) |
| Tenant provisioning, lifecycle projections, time/billing | **Lodestar** |

**The contract between them** (must stay stable, or be versioned): a client opens a
TCP connection, writes **one line of EDN** (the request), reads **one line of EDN**
(the response). The coordinator carries **no auth and no domain code** — that's
deliberate; auth is the gateway's job, lifecycle is the consumer's. Fram must not
take on auth/tenancy; Lodestar must not reach past this protocol into engine
internals.

Current request/response surface (from `cnf_coord_daemon.clj` `handle`):

```
{:op :version}                          -> {:version <n>}
{:op :status}                           -> {:version <n> :claims <n> :log "<path>"}
{:op :validate}                         -> {:violations [...]}
{:op :assert  :te "@id" :p "pred" :r v :base <n>} -> {:ok <n>} | {:conflict ...}
{:op :retract :te "@id" :p "pred" :r v :base <n>} -> {:ok <n>} | {:conflict ...}
{:op :subscribe}                        -> {:subscribed <n>} then a stream of events
(unknown)                               -> {:error "unknown op"}
```

If Fram's in-flight "big updates" change this protocol, that is a **breaking change
for the gateway** — coordinate it here (version the protocol or keep it additive).

---

## The work item

Today the daemon binds loopback unconditionally (in `cnf_coord_daemon.clj`, the
`serve` accept loop — locate the current bind site; it may have moved):

```clojure
(.bind (InetSocketAddress. (java.net.InetAddress/getLoopbackAddress) (int port)))
```

**Make the bind address configurable via `FRAM_BIND`, defaulting to loopback** so
no existing single-machine user is silently exposed:

```clojure
;; default loopback; honor FRAM_BIND for behind-the-gateway deployment
(defn- bind-addr []
  (let [b (System/getenv "FRAM_BIND")]
    (if (or (nil? b) (#{"" "loopback" "127.0.0.1"} b))
      (java.net.InetAddress/getLoopbackAddress)
      (java.net.InetAddress/getByName b))))

;; in serve: bind (bind-addr) instead of getLoopbackAddress; and when it is NOT a
;; loopback address, log a WARNING to stderr that the wire protocol is
;; UNAUTHENTICATED and MUST sit behind the Lodestar gateway / a firewall.
```

### The subtlety to decide (engine-side)

The CLI and `fram-up`'s doctor check **connect to `127.0.0.1`** (see the `client`
fn and `rt.clj`). If the daemon binds a *single non-loopback IP*, those local
clients break on that host. Two safe resolutions — Fram's call:

1. **Recommended: `FRAM_BIND=0.0.0.0`** binds *all* interfaces including loopback,
   so local CLI + doctor keep working, and isolation is enforced by the network
   (firewall / private network / the gateway as sole ingress) rather than by
   binding one IP. Simplest; no client change.
2. Also make the client *connect* address configurable (a `FRAM_CONNECT`/host arg)
   if you want to bind a single private IP. More moving parts.

Either way: **loopback stays the default**; non-loopback always logs the warning.

### Security invariant (must hold)

The coordinator protocol is unauthenticated by design. Binding non-loopback is only
safe paired with a network boundary where **the only thing that can reach the port
is the Lodestar gateway** (which authenticates). Never publish the raw port. The
default-loopback + loud-warning behaviour is what protects existing users.

---

## Acceptance criteria

- **No `FRAM_BIND`** → binds `127.0.0.1` exactly as before; all existing Fram tests,
  `fram-up`, and `fram doctor` are unchanged and green.
- **`FRAM_BIND=0.0.0.0`** → listens on all interfaces (verify with `ss -tlnp`),
  local loopback clients still work, and a warning is logged once at startup.
- **Lodestar's gateway smoke test still passes** unchanged (it uses loopback) — i.e.
  no regression to the wire contract above. (Run it from a Lodestar checkout:
  `FRAM_HOME=/path/to/fram bash deploy/gateway/smoke_test.sh`.)

A small test in Fram's suite asserting the two bind modes (default loopback vs
`FRAM_BIND=0.0.0.0`) would lock it in.

---

## Target topology this unblocks (Lodestar side, for context)

With the configurable bind, the per-tenant coordinators can run as separate
containers/hosts on a private network, with the gateway as the only public ingress:

```
internet ─TLS▶ gateway ─┬─ coordinator-acme   (FRAM_BIND=0.0.0.0, port 7801, not published)
                        └─ coordinator-globex (FRAM_BIND=0.0.0.0, port 7802, not published)
```

The gateway already supports this on its side — each tenant's registry entry can set
`:coordinator-host` (default `127.0.0.1`) to the coordinator's private hostname/IP.
Nothing else in Lodestar needs Fram to change.

---

## What Lodestar does *after* Fram lands this (so the seam is clear)

Strictly downstream of the contract — no further engine changes needed:

1. Switch `deploy/docker-compose.example.yml` from `network_mode: host` to a bridge
   network with each coordinator setting `FRAM_BIND=0.0.0.0` and the gateway's
   registry pointing `:coordinator-host` at the service names.
2. Add a Lodestar integration test that forwards through the gateway to a
   coordinator on a non-loopback address.
3. Flip `docs/hosting.md` from "same-host / shared-netns only" to "multi-host".

That's the whole remaining cross-host story. Everything else on the Lodestar
roadmap (control plane, product layer) is independent of Fram.
