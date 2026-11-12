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

## Promotion cadence (north#1022, planned 2026-11-11)

Propagation decides how fast one promoted runtime reaches every host. This section decides how often the settling
host promotes one. The two had been fused: the event-driven rule (e068c4c057, 2026-11-10) promotes on each landing
that changes a runtime input, so every landing was a fleet-wide deploy of the control plane.

### What the event-driven rule cost

Read from the settling host's pin journal (`runtime-pins.jsonl` as mirrored to gex45 at 2:58 AM +07 on 2026-11-11)
for the 8 h 53 min it ran before the mirror, 3:59 PM to 12:52 AM +07, and from the follow journal and capacity
ledgers the proxy and this plan read:

- 197 landings changed a runtime input and 74 distinct runtimes were promoted, about 8 an hour. The follow journal
  counted 32 `north runtime promoted:` lines in the 24 h to 4:45 AM, and 8 release builds ran on the laptop between
  3:14 and 4:05 AM before builds moved off it (664250fd40).
- Landing to live on the settling host: p50 241 s, p90 627 s, max 1,979 s over those 197 landings. The 2-minute
  bar the rule was built for (concerns.toml `ready-runtime-live-p90`) failed every hour of it: one flight at a time
  cannot keep up with landings arriving every 2.7 minutes.
- 10 promotions failed the walk on the settling host and were rolled back after 35 to 325 s. None reached a second
  host. Two more on 2026-11-11, after the mirror, failed on the wording of the launch probe's answer, not on a
  fault (455c0e5c and 12f6d9b6). Each one cost a release build, a walk and a delay for every landing queued behind
  it.
- The 5-minute landing-to-live page fired twice in ten minutes after the off-laptop builds went live: e3cf5bb8
  waited 1,011 s and 04e79016 374 s, both queued behind one flight while 03da559f was pinned. With one promotion per
  landing, the page fires on most landings and stops meaning anything.
- Build-host time per promotion: 74 s on cpx62 (warm target) and 6 min 10 s on gex45 (6 free cores, cold). The
  gex45 capacity ledger's one runtime build since then took 306.6 s with 10 cores reserved, cold, predicted 344.8 s.
  At 8 promotions an hour that is 10 to 51 minutes of build-host time per hour. The build-ahead change
  (18afcca179) adds one release build for every queued candidate on top of that.
- Each promotion re-execs every long-lived north process on every host (11 landers, session hosts, watchdogs) and
  makes one launch per provider for the canary.

### Decision

Promote on a cadence, with a fast path. This is item 1 of the strategy record, and its bars are Tom's 2026-11-10
answer 2 (south agents/decisions/harness/throughput-strategy-2026-11-10.md): no spine-breaking promotion reaches a
second host, rollback within 2 minutes, the running runtime at most a day old, and a fast path for red-main fixes.
The event-driven rule and its 2-minute landing-to-live bar went past that answer, and nothing records Tom changing
it. The north#1081 ask (propagation at the speed of the network) is about how fast one artifact moves, and the
cadence keeps that: each promotion still goes live everywhere within the 2-minute propagation target.

- **Slots.** The settling host promotes at fixed slots every 4 hours: 00:00, 04:00, 08:00, 12:00, 16:00 and 20:00
  UTC (7 AM, 11 AM, 3 PM, 7 PM, 11 PM and 3 AM +07). At most 6 a day. The spacing lives in harness/defaults.toml, not
  in a constant. A slot promotes origin/main's newest head that has a release artifact ready, and only if some commit
  since the pin changes a runtime input. Otherwise the slot passes and the pin stays.
- **Fast path.** A landing whose commit carries a `Live-fix: REASON` trailer is promoted at once. REASON names what
  is broken: red main, or the running runtime itself (a lander that cannot land, a broken worker loop, a canary
  that rolls back good runtimes). A fast-path promotion takes the next slot's place, so a day with at most one
  fast path per slot interval stays at 6. This replaces the `Main-red:` trailer of 64f4e8ec6b, because a broken
  lander (north#902) is the same emergency as red main and needs the same path. `north runtime status` prints the
  fast-path count for the last 24 h, so overuse shows.
- **Ordered promotion.** A commit already on origin/main reaches the settling host and every deploy host at once with
  `north runtime order --reason REASON [--commit REV]`, run as the coordinator's step on the settling host, with no
  landing trailer and no hand steps. Each host builds or fetches that runtime, runs its canary probes against it
  before it is live, goes live only on a pass and rewrites its stable pin where it had one; a failed canary leaves
  that host on its old runtime and names the probe. The command stops `north-stable-hold.service` for the run and
  starts it again, which is why a worker sandbox that cannot see the systemd user manager cannot run it. One
  `ordered` row per host in `runtime-pins.jsonl` records the commit, who ordered it, the reason and the verdict.
- **One build per slot.** Ten minutes before each slot, the settling host asks the build host predicted to finish
  soonest to build origin/main's head. At the slot it promotes the newest head whose artifact is ready. Build-ahead
  for queued candidates (18afcca179) narrows to candidates that carry `Live-fix:`, the only landings whose
  promotion waits on a build. That is about 6 builds a day plus fast paths, against about 200.
- **A failed slot keeps the old pin.** The walk rolls the runtime back and quarantines it as today. The proxy is
  alarmed with the failing probe, the next slot takes a newer head, and the fix can take the fast path. A rollback
  now delays fixes by a whole slot instead of a quarter hour, so a probe that fails on wording (north#1022's
  planted-token box) has to land before the cadence goes live.
- **What waits.** The lander's call after each landing, and the follow timer, read the schedule. Between slots they
  print `north runtime pinned: P stays live; H waits for the HH:MM slot or a Live-fix landing` and exit 0. One flight
  at a time and the lock queue stay as they are.

### What the page and the bars measure after the change

- **The page measures due to live.** A promotion is due at its slot, or at the landing time of a fast-path landing.
  The pin journal records a `due` event with sha, reason (slot or fast path) and time. The coordinator is paged when
  a due promotion is not live on the settling host 5 minutes later. An ordinary landing waiting for its slot is on
  schedule and pages nothing. A fast-path landing pages at 5 minutes, as every landing does today.
- **Page volume.** At 5:52 AM +07 on 2026-11-11 the per-landing page tripped the proxy's page-flood ceiling with 4
  pages in 10 minutes. One of them named the landing it paged about, 18afcca1, as the pinned runtime, which is a
  separate false-page box. The coordinator can act on one page per stuck promotion. Under the cadence that is at
  most one per due promotion: 6 slots plus the fast paths, about 7 a day, and never two in one slot interval unless
  a fast path is itself stuck. Later due promotions that queue behind a stuck one join its episode, and the episode
  is paged once, as `north runtime lag` pages once per episode (north#1081). More than one page in 10 minutes then
  means the promotion machinery is broken, which is the one thing the page is for.
- **The bars.** `ready-runtime-live-p90` becomes due to live p90 ≤ 2 min over 24 h, since one hour holds at most
  one slot. The runtime-age bar returns (main's oldest unpromoted runtime-input landing ≤ 24 h). Breaks reaching a
  second host (0) and rollback (≤ 2 min) stay. `north runtime status` prints `promotions in 24 h: N of at most 6`
  again.

### As built (n1022-slots, 2026-11-11)

- The schedule is `crates/north-runtime/src/runtime/cadence.rs`. The spacing and the build lead are
  `runtime_promotion_slot_sec` and `runtime_slot_build_lead_sec` in harness/defaults.toml.
- A slot is due only for a landing the settling host saw at or before it. A slot with nothing waiting passes, and a
  landing seen after it waits for the next slot.
- A slot the settling host was not running at is promoted on its first pass afterwards, with its `due` event at the
  slot's time, so the lateness shows in the due-to-live bar and pages once.
- A fast-path `due` is keyed by the fix's commit and opens once. A fix that was promoted and rolled back is not
  promoted again; the next fix carries its own trailer. A head quarantined after a rollback opens no slot.
- Build-ahead in the lander asks only for candidates whose commits carry `Live-fix:`.

### Cost to the boxes that wait on a live runtime

| Box | Under the cadence | Change it needs |
| --- | --- | --- |
| north#1022 page (ticked) | stops firing on scheduled waits | the page reads `due`, not `landed` |
| north#1022 no rebuild after landing | read at a slot, up to 4 h away | check gap is due to promoted < 2 min |
| north#1022 build ahead | artifact-ready 0 s holds for fast-path landings only | check reads a fast-path landing |
| north#1022 canary < 60 s, ship < 30 s p90 | about 6 samples a day; a 1 h window often has none | window becomes 24 h |
| north#1022 stage durations on the scoreboard | unchanged, fewer samples | none |
| north#1022 live launch probe (from north#1027) | waits for the next canary, up to 4 h | none |
| north#1022 planted-token probe | now a prerequisite | lands before the cadence goes live |
| north#1022 this box | `rg -c 'north runtime promoted:'` ≤ 6 after one day of slots | none |
| north#1081 processes move within 60 s | read 2 min after a promotion, up to 4 h to the next one; 6 re-execs a day instead of about 200 | none |
| north#1081 `north runtime lag` < 300 s | fails by design against origin/main (up to 4 h behind) | lag is measured against the settling host's pin, which `north runtime lag` now takes as its reference (north#1022) |
| north#1081 runtime landing to live p90 < 2 min | not met for ordinary landings (p50 about 2 h, p90 about 3.6 h) | becomes pin to live on every host p90 < 2 min; firn half unchanged |
| north#902 lander fix within one landing round | an ordinary lander change waits up to 4 h | a lander fix carries `Live-fix:` and takes the fast path |

The cost in latency: an ordinary landing is live a median of about 2 h after it lands, at most about 4 h. A fix
that is not marked `Live-fix:` waits for its slot. In exchange: about 6 builds, walks, probe launch pairs and
fleet-wide re-execs a day instead of about 200, rollbacks of good runtimes become rare and visible, and the
5-minute page means something again.

### Alternative: restate the bar

Keep promoting per landing and write the measured number into the strategy record: about 200 promotions a day, a
landing to live p90 of about 10 min, and a page at 5 min after each landing. Fixes arrive fastest (a median of
about 4 min). The cost is everything listed above at full rate: the flight queue already misses its own 2-minute
bar, 10 walk rollbacks in 9 h, 10 to 51 minutes of build-host time per hour plus a build per candidate, every
long-lived process re-exec'd about 8 times an hour, and a page that fires on most landings. It also keeps the
root shape the strategy record named as the largest source of fragility: the fleet runs on the code it is
changing.

### Builder choice

`build_host::freest` picks the build host with the most free cores. On 2026-11-11 that chose gex45 (6 free, cold
target, 6 min 10 s) over cpx62 (74 s warm). Free cores only predict a build's wall time when every host's target is
equally warm. The choice should be the host predicted to finish soonest: its admission queue wait plus the capacity
model's predicted wall seconds for `north cargo build` on that host and cache state. The capacity ledger already
records `predictedWallSeconds` for each build, 344.8 s for the cold gex45 build above. This is filed as its own box
on north#1022, checked by a test where a host with fewer free cores and a warm target is chosen over one with more
free cores and a cold target.
