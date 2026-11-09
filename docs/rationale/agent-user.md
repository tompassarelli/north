# Agent user: agents run under a Unix user of their own, with brokered credentials and depbox for every dependency run

Decided 2026-11-09 under north#977. North moves every agent session to a Unix user of its own on each host, gives agents only credentials they own or that a broker scopes per call, runs every dependency an agent builds, tests or installs inside depbox, and admits new dependencies only after a 7-day release age and an audit gate. Two floors sit underneath: recovery copies that only root can remove, and GitHub's own rules on main. This note is the public method and the measurements behind it; the readings of each host, the threat model and the host steps are held in south's agents/decisions/harness records.

## Method

The boundary is a Unix user, not a list of rules. A rule that names each path to protect has to be extended for every new path; a uid boundary is enforced by the kernel's file permissions for every path at once. The per-task Landlock layer (worker-sandbox.md) stays, inside the agent user, for what a uid cannot separate.

### Identity: one agent user per host

Each host declares one user, `agent`, home `/home/agent` (0750, group `agent`), and every agent session of that host runs as it. The operator's home stays 0700. The operator joins group `agent` to read the fleet's checkouts and to write its message spool; the agent user is given nothing of the operator's.

One user per host rather than one per session (a systemd DynamicUser, [systemd-exec](../references.md#systemd-exec)): north keeps per-host state that every session shares. The capacity admission ledger lives in the user's runtime directory, capacity and session scopes are units of the user's service manager, every lane of a project shares one git common directory, and the sessions on one Claude account share its profile. Per-session users would split the ledger and the scopes, and would need group write on the shared git objects and profiles, which returns to every session the reach over the others that separate users were meant to remove.

What moves to the agent user: the fleet's checkouts, at `/home/agent/code/<project>` (north names that root from north#984 on, with a symlink to the old root on each host until its cutover); north's state directories, which already derive from `$HOME`; the agent's own Claude profiles and setup tokens; north's user units, under a lingering user manager for `agent`. The lander moves to a system user of its own, so that its push credential is held apart from the agents whose lanes it judges (north#982).

One rule keeps the direction one-way: no operator or root process executes or builds anything the agent user can write. Code reaches the operator and root only as landed main fetched from GitHub: the operator's tools and dotfiles come from the Nix store, his git does not mark the agent's repositories safe, and root services fetch their sources from GitHub themselves.

### Credentials: owned by the agent or held by a broker

Each credential has one reach, recorded beside it in south's credential registry, which `north ops credentials check` reads:

- `operator`: read by the operator's user only.
- `agent-recipient`: decrypted by the agent user itself. The host's agent age key is a sops recipient of exactly these files, and a key service instance owned by the agent user holds only that key. The sops key service protocol carries the wrapped data key and not the file name, so a per-file rule inside one key service could not tell files apart; recipients are the boundary sops itself enforces.
- `broker:<allowlist>`: held by the credential broker and used through it.

The credential broker (north#981) runs as its own system user, with each GitHub App key and cloud token loaded as a systemd credential. It names each caller by SO_PEERCRED and logs every grant. It offers:

- GitHub installation tokens scoped per call to the repositories and permissions the call needs, which GitHub's token endpoint allows, with its fixed one-hour lifetime ([github-installation-token](../references.md#github-installation-token)); a ceiling per caller uid and App, and the lander's App only to the lander's user.
- A cloud API proxy after Docker Sandboxes' credential proxy ([docker-sandboxes](../references.md#docker-sandboxes)), without a VM: a tool sends its usual request with a placeholder token to the broker's per-provider path; the broker checks method and path against that provider's allowlist, swaps in the credential and forwards over TLS. CLIs that take a base URL work unchanged, and the credential itself never reaches the caller.

Claude setup tokens are the agent's own and are not brokered: the Claude process presents them itself, and a proxy in front of Claude Code would be a workaround around a vendor client.

### Privilege: no root-equivalent group

The agent user has no supplementary group by default (a GPU host may add `video` and `render`), and is never in `wheel`, `input`, `uinput`, `libvirtd`, `podman`, `docker` or `systemd-journal`; south's flake checks assert the list per host (firn#62). Nix `trusted-users` stays root only. Root work an agent needs goes through named system units: `firn rebuild --landed`, which builds only GitHub's main and admits the agent uid by name (firn#61), and a host's cutover unit, started through a polkit rule scoped to that unit.

### Supply chain: dependency code runs in depbox, and enters the lock only after admission

Builds and tests run code nobody vouched for: build scripts, proc macros, test binaries, install scripts and the packages' own code. Three parts:

- Execution (north#974): every agent `cargo`, `rustc`, `bun`, `bunx`, `node`, `npm`, `npx`, `pnpm` and `yarn` call enters depbox through the capacity-gate shims already first on every agent PATH, `depbox fetch` only for the verbs that download. The Landlock layer withholds EXECUTE beneath a lane's `target/` and `node_modules/`, so build output runs by its path only inside depbox, and `north lane exec` runs a lane's own binary outside it only while the lane's lock files equal main's. depbox becomes `north depbox` in Rust, since the change takes the script past the 80-line shell cap. The operator's terminal gets the same shims from the Nix store.
- Admission (north#983): dep-gate already holds a 7-day floor for Cargo.lock, bun.lock and flake.lock entries (crates/north-diff-gates/src/dep_gate.rs) and runs it in safe-push; the lander runs it too, with `cargo deny check advisories bans sources` ([cargo-deny](../references.md#cargo-deny)) and osv-scanner over the locks, flake inputs from an owner allowlist, and `depbox fetch -- cargo update` restoring the previous lock when it resolves a version under 7 days, which gives cargo the resolution floor bun has. cargo-vet ([cargo-vet](../references.md#cargo-vet)) is measured first: adopted only if the published auditors' imported audits cover at least half of north's lock, since a gate resting on exemptions an agent writes for itself vouches for nothing.
- The uid: whatever dependency code runs as the agent user reaches only what that user owns.

depbox costs nothing measurable: north-base's 66 tests took 6.79 s in it and 6.94 s outside it on 2026-11-09.

#### gVisor and microVMs, measured

Measured on 2026-11-09 on a 20-thread build host: north-base built cold with 8 jobs into a fresh target directory, built again unchanged, and its tests run, natively and under gVisor's rootless `runsc do` with no network ([gvisor](../references.md#gvisor), release 20260406).

- Cold build: native 9.1 and 9.7 s; gVisor 13.3 s, about 40% slower.
- Unchanged rebuild: native 0.1 to 0.2 s; gVisor 0.8 s with the host filesystem writable, and a full 12 to 14 s rebuild on every run with `runsc do`'s default overlay root, which loses cargo's fingerprints between runs.
- Tests: native 7.2 s (6.94 s in the test harness); gVisor 10.8 s, about 50% slower, with 2 of 66 tests failing for want of a loopback network for their ssh fixture; depbox passed all 66.
- Rootless runsc does not start inside a Landlock domain, which refuses its mount calls.

Verdict: against. A 40 to 50% longer build and test step would be paid on every landing, against a landing p90 target of 60 s. A microVM was not measured: it adds a guest boot and a shared-filesystem layer to the same compile and needs KVM on every host, and the cheaper of the two kernel boundaries already fails the bar. No part of the design needs Docker; its credential proxy is the broker's design, and nix-leverage.md's verdicts on Docker Sandboxes, microvm.nix and NixOS containers stand.

### Floors

- Recovery (firn#60): hourly read-only btrfs snapshots into a root-only directory where the filesystem is btrfs, with `user_subvol_rm_allowed` kept off; on every host a root unit, not a login user's unit, pushes the fleet's state to the append-only vault, its key a systemd credential.
- GitHub (north#973, north#982): a ruleset on main refuses non-fast-forward updates and deletion ([github-rulesets](../references.md#github-rulesets)), and a second rule restricts updates to the lander's App.

### The worker sandbox

Kept as a per-task layer inside the agent user: one session's removals stay in its own lane and trees, the agent's own tokens stay out of its Bash commands, and build output runs only in depbox. Its rule for the operator's GitHub login is removed once no agent runs as the operator (north#985). north#975 stays at its own priority, now separating agent sessions from one another.

## Phases

Each phase lands and is checked alone, and the fleet keeps running between them: phases 1 to 10 do not change which user runs agents, and each cutover drains one host while the others carry the fleet. Each issue holds its runnable Done-when boxes, what could break, its rollback and any step the operator takes himself.

1. Every agent session reaches GitHub as the agent App (north#977, first box).
2. GitHub refuses rewriting and deleting main (north#973).
3. Recovery copies only root can remove (firn#60).
4. Root services take sources only from GitHub; the agent's own key service instance (firn#61).
5. The credential broker (north#981).
6. Only the lander updates main (north#982).
7. Every agent dependency run goes through depbox (north#974).
8. The lander's dependency floor and audit gate (north#983).
9. North runs under any Unix user; the fleet root is `/home/agent/code` (north#984).
10. An inert agent user on every host (firn#62).
11. to 13. Host cutovers, build hosts first and the operator's own machine last, held in south.
14. The sandbox's operator-specific rules retired (north#985).

Phases 1, 2, 3, 7 and 8 start at once and in parallel; 4, 5, 9 and 10 next; 6 after 5; the cutovers in order; 14 last.

## Measured cost

- Build and test time: none from depbox; gVisor would add 40 to 50%.
- Disk: none extra; a cutover renames the fleet's tree on its own filesystem and re-owns it.
- Code: one new crate (the broker), depbox ported to Rust, one lander gate, and two literals of the operator's uid or home removed; the rest of north already derives its paths from `$HOME`.
- Downtime: one host's drain at a time.
