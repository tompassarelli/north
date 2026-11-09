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

## Sources

GitHub's scoped installation tokens ([github-installation-token](../references.md#github-installation-token)), rulesets ([github-rulesets](../references.md#github-rulesets)), cargo-deny ([cargo-deny](../references.md#cargo-deny)) and cargo-vet ([cargo-vet](../references.md#cargo-vet)) are the outside pieces the south plan builds on.
