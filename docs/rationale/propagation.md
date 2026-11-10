# Propagation: build each artifact once and move it to every host

A landed change that must reach every machine (the north runtime, and firn's system closures for
whiterabbit, gex45, cpx62 and nexus) should cost one build on one host plus a network copy per
host. This file records what each stage cost on 2026-11-10, the prior art, and the design chosen
(north#1081). north#1022 owns the runtime's promotion trigger; this file owns build-once,
distribute-everywhere.

## Method

Both followers are user timers on the laptop, and their journals are the only stage record today:

- `journalctl --user -u north-runtime-follow -o short-unix`: build is the time from `this host builds
  its own runtime` to the first `nix-store --realise of N store paths` line; ship to settle is from
  that line to the unit's `Consumed ... wall clock` line, which comes after every host reads live
  and healthy.
- `journalctl --user -u firn-host-follow -o short-unix`: a remote deploy is from the first
  `HOST runs REV; deploying REV` line of a run to its `Consumed` line (eval, build on the builder,
  copy, activate, all hosts in parallel); the laptop's own deploy is from `self runs ...; deploying`
  to its `Consumed` line.
- Paths between hosts: `ping` round trips over each link the hosts use, and `ssh -G HOST` for the
  route the laptop's ssh takes.

## Stage timings, 7 days to 2026-11-10

- North runtime build on the laptop: n=171, p50 40 s, p90 242 s, max 1092 s. The land check had
  already compiled the same tree on a build host, and the laptop rebuilt it at 4 idle jobs.
- North runtime ship to settled on every host: n=530, p50 50 s, p90 493 s, max 6.5 h (a host
  unreachable or unhealthy holds the run).
- Firn closure, remote hosts (gex45, cpx62, nexus): n=166, p50 82 s, p90 627 s, max 4.3 h. Every
  run starts with a 30 s quiet wait for main to settle (`FIRN_PUSH_QUIET`), then evaluates and builds
  each host's toplevel on gex45, then `nix copy --from ssh-ng://gex45 --to ssh-ng://HOST`, which
  routes every byte through the laptop.
- Firn closure, the laptop: n=139, p50 91 s, p90 627 s, 11 failed runs (a transient unit name the
  previous run left loaded). The laptop builds its own closure from source through the
  landed-rebuild service and substitutes nothing a build host built.

The bytes are not the cost: a closure delta is tens of megabytes. The cost is building on hosts that
did not need to build (the laptop for itself and for the runtime), the busiest build host doing the
builds, and copies relayed through the laptop's long-haul link. Per-host round trips and the private
network's reach are in South (agents/decisions/infra/propagation-2026-11-10.md).

## Prior art and what each gives us

- Content-addressed artifacts keyed by input hash: Nix store paths are already keyed by the hash of
  their derivation, so "built once" means one host realises the derivation and every other host
  fetches the same path. Nothing new to adopt; the work is making every host fetch instead of build.
- Bazel and Buck2 remote cache and remote execution
  (https://bazel.build/remote/caching, https://buck2.build/docs/users/remote_execution/): an action
  cache keyed by input digest plus a content store, with execution sent to a pool. Nix's derivation
  hash is the same key, and `builders`/`--store ssh-ng://` is the remote execution half. Adopting
  either tool would mean a second build graph beside Nix and Cargo; prior only.
- A shared substituter: nix-serve (Perl, https://github.com/edolstra/nix-serve), nix-serve-ng
  (Haskell, https://github.com/aristanetworks/nix-serve-ng), Harmonia (Rust,
  https://github.com/nix-community/harmonia) and Attic (Rust, https://github.com/zhaofengli/attic).
  Harmonia serves the build host's own `/nix/store` over HTTP and signs each narinfo on the fly, so
  a path is in the cache the moment the build finishes, with no upload step. Attic is a separate
  multi-tenant store with chunk deduplication, a database and a push step; it pays off when the
  store must outlive the build host, which costs a server and an upload per build here.
- Compiler caches: sccache with a shared backend is already in place (nexus serves a WebDAV level,
  build-cost.md). It saves compile time inside a build; it does not stop a host from building.
- Binary diff transfer: `zstd --patch-from` (https://github.com/facebook/zstd) and casync
  (https://github.com/systemd/casync). A store path changes whole when an input changes, so a diff
  against the previous generation would shrink the north binary's transfer; at tens of megabytes
  per change and sub-millisecond links between the Hetzner hosts it is not the bottleneck. Prior only.
- Push versus pull: a push (`nix copy --to`) needs the pusher to reach every host and relays through
  it; a pull (each host substitutes) needs every host to reach the cache. With
  `nix copy --substitute-on-destination` the follower keeps the push as its orchestration, and the
  destination's daemon fetches each missing path from its own substituters first, so the bytes go
  cache to host directly wherever the host reaches the cache.

## Design chosen

- The build host with the most free cores builds every host's closure and the runtime once, rather
  than the host that also runs the game clients and the runner farm.
- The build host serves its own store with Harmonia on its private-network address (firn module
  `binary-cache`, option `serve`), and its nix daemon signs every path it builds with the same key
  the cache signs with (`secret-key-files`), a key the hosts already trust for deploys.
- Every host lists the build host's cache ahead of the public caches (`binary-cache.substituters`,
  priority 30 against cache.nixos.org's 40), and the follower's existing
  `nix copy --substitute-on-destination` push makes each destination pull from it.
- Why Harmonia: it is the only option with no upload step, in Rust, packaged with a NixOS module in
  the nixpkgs firn already pins (3.1.0), MIT licensed and maintained by nix-community. It reads the
  store through the local nix daemon and listens on one TCP port on the private link.
- Reach: a host substitutes only once it has a direct private link to the build host; until then
  it keeps the relayed copy. Which hosts have one is in South.
- Stage timings go to the ledger per artifact, stage and host, so the landing-to-live p90 targets
  (2 minutes for the runtime, 5 for a firn closure) are read from records instead of journals.
- Once a host holds a new runtime, every long-lived north process on it moves onto it at its next
  safe point through one mechanism ([runtime follow](runtime-follow.md)).
