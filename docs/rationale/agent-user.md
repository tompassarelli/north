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

## Sources

GitHub's scoped installation tokens ([github-installation-token](../references.md#github-installation-token)), rulesets ([github-rulesets](../references.md#github-rulesets)), cargo-deny ([cargo-deny](../references.md#cargo-deny)) and cargo-vet ([cargo-vet](../references.md#cargo-vet)) are the outside pieces the south plan builds on.
