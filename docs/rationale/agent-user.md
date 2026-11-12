# Agent user: agents run under a Unix user of their own

Decided 2026-11-09 under north#977. North runs agent sessions under a Unix user of their own, separate from the operator's, gives them only credentials they own or receive scoped per call, and runs the dependency code they build, test or install in depbox. The design, its phases and the per-host steps are held in south (agents/decisions/harness/agent-user-plan-2026-11-09.md, phase issues south#87 to south#98); this note keeps the two decisions that rest on north's own structure and measurements.

## One agent Unix user per host, not one per session

A systemd DynamicUser per session ([systemd-exec](../references.md#systemd-exec)) was weighed against one agent Unix user per host. North keeps per-host state that every session shares: the capacity admission ledger lives in the user's runtime directory, capacity and session scopes are units of the user's service manager, every lane of a project shares one git common directory, and the sessions on one Claude account share its profile. Per-session users would split the ledger and the scopes, and would need group write on the shared git objects and profiles, which gives every session back the reach over the others that separate users were meant to remove. North therefore derives every path from `$HOME`, XDG directories and the uid, so the same code runs under either user; the fleet's checkouts live at `/home/agent/code/<project>`.

## gVisor under depbox, measured

Measured on 2026-11-09 on a 20-thread build host: north-base built cold with 8 jobs into a fresh target directory, built again unchanged, and its tests run, natively and under gVisor's rootless `runsc do` with no network ([gvisor](../references.md#gvisor), release 20260406).

- Cold build: native 9.1 and 9.7 s; gVisor 13.3 s, about 40% slower.
- Unchanged rebuild: native 0.1 to 0.2 s; gVisor 0.8 s with the host filesystem writable, and a full 12 to 14 s rebuild on every run with `runsc do`'s default overlay root, which loses cargo's fingerprints between runs.
- Tests: native 7.2 s (6.94 s in the test harness); gVisor 10.8 s, about 50% slower, with 2 of 66 tests failing for want of a loopback network for their ssh fixture; depbox passed all 66 in 6.79 s.
- Rootless runsc does not start inside a Landlock domain, which refuses its mount calls.

Verdict: against. A 40 to 50% longer build and test step would be paid on every landing, against a landing p90 target of 60 s, while depbox itself costs nothing measurable. A microVM was not measured: it adds a guest boot and a shared-filesystem layer to the same compile and needs KVM on every host, and the cheaper of the two options already fails the bar. nix-leverage.md's verdicts on Docker Sandboxes, microvm.nix and NixOS containers stand; the broker's design borrows Docker Sandboxes' credential proxy ([docker-sandboxes](../references.md#docker-sandboxes)) without the VM.

## Dependency gates at the lander, and cargo-vet coverage measured

The lander, not only the pushing host, judges what enters a lock (south#96). Two lane gates run wherever lane gates run, at enqueue and in the lander's own pick, and follow the `landing_refusals` switch: `dependency age` reads each lock entry the lane adds from crates.io, npm or the flake's `lastModified` and refuses one published under 7 days ago unless a `Dependency-Urgent:` trailer names it; `dependency advisory` runs `cargo deny check advisories bans sources` ([cargo-deny](../references.md#cargo-deny)) over each changed Cargo.lock, with the crate graph built from the lock itself so no crate source is needed, against an advisory database under `$XDG_STATE_HOME/north/advisory-db` that it fetches at most once a day and otherwise reads offline, and `osv-scanner` over each changed bun.lock against an offline npm database beside it, refreshed the same way. It refuses only versions the lane adds, so an advisory against a crate main already locks does not stop unrelated lanes, unless a `Dependency-Advisory: <advisory-id> — <reason>` trailer accepts it. A flake input the lane adds must come from an owner listed as `owner` under `[flake]` in `.safe-push` (NixOS, tompassarelli and oxalica today); a local path input is exempt. Cargo gets the resolution floor bun has: after each cargo command it runs, `north depbox fetch` puts Cargo.lock back as it was whenever the command resolved an entry under 7 days old, and names each one.

cargo-vet coverage, measured on 2026-11-09 (cargo-vet 0.10.2): `cargo vet init` on north's lock exempted all 265 crates.io crates; importing the audit sets of all nine auditors in cargo-vet's registry (actix, ariel-os, bytecode-alliance, embark-studios, fermyon, google, isrg, mozilla, zcash) and running `cargo vet prune` left 197 exemptions, so the published audits cover 68 of 265 locked crates, 26%. Verdict: not adopted. Under half the lock is vouched for by anyone outside the fleet, so a gate on unaudited crates would rest on exemptions agents write for themselves; cargo-deny and the 7-day floor stay the gates.

## First slice

What changes about git: each project gets a second clone, owned by the agent user and fetched from GitHub, and lanes are worktrees of that clone as they are today. The lander takes a lane's commits from the git bundle safe-push already writes beside each outboxed ticket (crates/north-lander/src/land/enqueue.rs:172), or fetches them over ssh. Two other layouts were weighed: one repository written by both users, which fails because a repository's configuration and hooks name programs git runs; and a clone per lane, which costs a store per lane and breaks the lane verbs, which assume worktrees.

Planned 2026-11-12 under north#1041, and the operator decided the same evening to build it. The slice moves one build host's worker sessions under the agent user and takes nothing else from the fourteen phases in south's plan unless that host needs it. The full per-host plan, with each step's detail and check, is in south (agents/decisions/harness/agent-user-first-slice-2026-11-12.md).

### Host

The first host is a cloud build host whose sessions are all code-only. Hosts that also serve GPU, native or private boxes come later, in the order below.

### Steps, in order, with estimates

1. The agent user on the host: 1.5 h.
2. Session placement on the agent's login: 2 h.
3. The checkout root in briefs: 1.5 h (landed as cacc862579).
4. Lanes and git, as above: 2 h.
5. Client profiles and credentials: 1 h.
6. Scratch and caches, seeded once: 1 h.
7. North's units and the runtime link: 2 h.
8. Brokers: 2 h.
9. Hooks and settings: 1 h.
10. Fleet messages, build offload and Buck2: 1 h.
11. The replay: 1.5 h.

Total: 16.5 hours of work. Rollback returns the host to the operator's path.

### What gets slower

The first build of each project in the new clone, unless step 6 seeds the caches. Every other host keeps running as it does today until its own turn.

### The proof

The slice is proven on its host by a replay of each class of finding from the 2026-11-12 review, each ending in a refusal from the kernel rather than from north, and then one day of normal placement that keeps the fleet's first-try rate and the landing p90. The replays themselves are in the south record.

### Delete list, once every host has cut over

Whole files under crates/north-runtime/src/worker_sandbox/, 665 lines:

- request_checkout.rs (53);
- request_env.rs (89);
- brokered_reads.rs (100);
- offload_request.rs and its tests (90 + 296);
- readable.rs (37).

Parts of files, an estimated 430 lines:

- the ssh part of sockets.rs (about 80);
- the operator-only verbs and gates of broker.rs (about 120);
- the operator-path entries of writable.rs, state.rs and lane_only.rs (about 30);
- their test cases (about 200).

Three per-path rules outside the sandbox crate go as well.

Total: about 1,100 lines. The per-session Landlock layer stays, as the plan's own phase 14 keeps it.

### Remaining hosts

1. The other cloud build host of the same shape, once the replay holds on the first.
2. The 20-thread host's code-only sessions. Its GPU and private boxes wait for an agent path of their own.
3. The remote-serve host, once remote-serve has its verb set.
4. The laptop, last.

### A rootless container per session

The 2026-11-09 verdict measured gVisor. A plain namespace container, as depbox is, measured no cost (6.79 s against 6.94 s), so that verdict does not cover it; the verdicts on Docker Sandboxes and a microVM stand. A container per session is not part of this slice and would be measured before it is adopted.

## Sources

GitHub's scoped installation tokens ([github-installation-token](../references.md#github-installation-token)), rulesets ([github-rulesets](../references.md#github-rulesets)), cargo-deny ([cargo-deny](../references.md#cargo-deny)) and cargo-vet ([cargo-vet](../references.md#cargo-vet)) are the outside pieces the south plan builds on.
