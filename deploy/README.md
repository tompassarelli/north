# deploy/

Artifacts for running Lodestar as a service. Full architecture and the three
hosting modes are in [`../docs/hosting.md`](../docs/hosting.md); this is the
quick index.

| file | what |
|------|------|
| `gateway/` | the authenticated multi-tenant edge (token → tenant → coordinator); auth, rotation/revocation, audit log, rate limit + body cap; `provision.sh` + smoke test |
| `../Dockerfile` | one runtime image (bb + Fram + Lodestar); runs as a coordinator or the gateway |
| `docker-compose.example.yml` | example one-host topology: a gateway + one coordinator per tenant |
| `lodestar-coordinator@.service` | systemd template — one coordinator per tenant |
| `lodestar-gateway.service` | systemd unit — the gateway |
| `Caddyfile.example` | reverse proxy terminating TLS in front of the gateway |
| `backup.sh` + `lodestar-backup.{service,timer}` | per-tenant `claims.log` snapshot + prune, on a daily timer |

## The shape

Each tenant is an isolated **coordinator + `claims.log`**. The gateway authenticates
a request and routes it to that tenant's coordinator. Coordinators bind loopback,
so the gateway and the coordinators it fronts share one network namespace (one
host, `--network host`, or one pod) — multi-host is a roadmap item (see hosting.md).

## Fastest real deployment (systemd, one host)

```sh
# 0. prerequisites on the box: babashka, plus Fram + Lodestar at /opt
sudo install -d /opt /var/lib/lodestar /etc/lodestar -o lodestar -g lodestar
sudo git clone https://github.com/tompassarelli/fram     /opt/fram
sudo git clone https://github.com/tompassarelli/lodestar /opt/lodestar

# 1. provision a tenant (mints a token, starts its coordinator, registers it)
sudo -u lodestar env GATEWAY_TENANTS=/var/lib/lodestar/tenants.edn \
  LODESTAR_TENANT_ROOT=/var/lib/lodestar/tenants \
  /opt/lodestar/deploy/gateway/provision.sh acme 7801

# 2. install the units
sudo cp /opt/lodestar/deploy/lodestar-coordinator@.service /etc/systemd/system/
sudo cp /opt/lodestar/deploy/lodestar-gateway.service       /etc/systemd/system/
printf 'FRAM_PORT=7801\nFRAM_LOG=/var/lib/lodestar/tenants/acme/claims.log\n' \
  | sudo tee /etc/lodestar/acme.env
printf 'GATEWAY_PORT=8088\nGATEWAY_TENANTS=/var/lib/lodestar/tenants.edn\n' \
  | sudo tee /etc/lodestar/gateway.env
sudo systemctl daemon-reload
sudo systemctl enable --now lodestar-coordinator@acme lodestar-gateway

# 3. put TLS in front of :8088 (Caddy one-liner): reverse_proxy 127.0.0.1:8088
```

## Security

- The raw coordinator port is **unauthenticated** — never expose it; only the
  gateway is reachable, and only through TLS.
- Tenant bearer tokens are stored **hashed** (sha-256). `provision.sh` prints the
  plaintext once.
- Run as a dedicated unprivileged `lodestar` user; the units ship basic hardening.
