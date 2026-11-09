# Nix leverage and agent sandboxing: an adversarial review of "fix everything with a Nix overlay"

Review of 2026-11-09 for north#968: Geoffrey Huntley's post on Nix ([ghuntley-nix](../references.md#ghuntley-nix),
2026-10-07) and its companion repository ([ghuntley-nix-demo](../references.md#ghuntley-nix-demo)), tested claim by
claim against how north, firn and south use Nix, plus the question of whether agents need Docker or a microVM. Each
claim is argued against first, then judged adopt, already done or reject. The review was on paper: no experiment ran
against a home directory, a backup, a snapshot or a cloud API, and nothing that widens what agents can do was
adopted. Host-specific readings behind the verdicts are in south (agents/decisions/harness/nix-leverage-2026-11-09.md).

## Verdicts

- Claim 1, one Nix expression for laptop, CI and agent sandboxes: already done for the Rust toolchain; one drift
  removed (bun), one environment left unmeasured (cloud workers).
- Claim 2, patch git so agents cannot force-push: reject the patched binary; adopt the server-side rule it stands in
  for (north#973).
- Claim 3, NixOS VM tests before deploy: reject for now; evaluation-time assertions catch the configuration faults
  we actually had.
- Claim 4, agents may use sudo because NixOS rolls back: reject; one grant of the login user removed (firn
  d97235a26c), the rest left to Tom in south's record.
- Claim 5, one overlay patches a vulnerable dependency fleet-wide: already in use, and it stops at each flake's own
  nixpkgs.
- Claim 6, Nix's store caches incrementally worse than Bazel or Buck2: true and already avoided; Nix pins the
  toolchain and nothing more.
- Claim 7, Nix-built container images: not applicable; north ships no image, and firn's one image exists to mirror
  GitHub's runner.
- Claim 8, Docker Sandboxes or a microVM per agent: reject; the Linux primitives north already runs cover both threats,
  extended to every path by north#974 and north#975.

## Claim 1: one expression is the single source of truth

Against: devenv is a second layer over flakes with its own lock and its own binary cache to trust, and its `devenv up`
services and git hooks duplicate what north already does with capacity leases and safe-push. A shared expression also
does not stop host drift: a dev shell is impure, so the host's other programs still leak in, which is the class
north#942 fixed when GitHub's Cargo workflow ran on Ubuntu outside the shell.

Evidence: north's flake dev shell is already the one expression. Workers enter it through the capacity shims,
landings through `land-shell --dev` (bin/land-shell), build-host slots through `north capacity dev-shell run`, and
GitHub's Cargo workflow through the nix-dev-shell action (.github/workflows/cargo.yml:41, north#942). Two drifts were
left:

- bun: the dev shell took nixpkgs' bun 1.3.13 while firn installs a pinned 1.4.4 on every host and the harness
  workflow pins 1.4.4 (.github/actions/harness/action.yml:25), so tests/it/automation_chrome_service.rs tested a
  different bun from the one the north services run. Fixed here: flake.nix pins the same release by the same hash, so
  the store path of the download is shared with the hosts and the shell builds offline.
- Cloud workers run in a provider-managed environment that is not Nix. Their verdict does not count, since a handback
  lands through the same checks as any lane, so the cost is first tries lost to a difference, which no record counts
  yet. Not adopted: measure first (count handbacks the lander refuses for an environment difference) before paying a
  Nix install per cloud session.

Minor and not adopted: the harness workflow installs ripgrep, shellcheck and bubblewrap from Ubuntu's archive
(.github/actions/harness/action.yml:40) while the dev shell carries shellcheck 0.11.0; no test currently depends on
the difference (harness/machine-bound-tests lists none).

Verdict: already done; devenv rejected, since it adds a tool and a trusted cache for no capability the flake lacks.

## Claim 2: an overlay that removes force-push from git

The demo replaces git through an overlay with five patches that refuse `--force`, `--force-with-lease`, `+refspec`,
`--mirror` and forced WebDAV pushes ([ghuntley-nix-demo](../references.md#ghuntley-nix-demo)).

Against: a patched binary binds only the programs that run that binary. libgit2 or gix in a program, a git reached
by its store path, or a REST or GraphQL ref update with force set never pass through it. It also breaks north:
safe-push publishes with `--force-with-lease=REF:OID` as a compare-and-swap (crates/north-safe-push/src/publication/plan.rs:28),
restores with a create-only lease (crates/north-safe-push/src/restore.rs:101) and deletes tags under a lease, so the
demo's git would refuse every landing. Maintaining five patches against each git release is a cost of its own.

Evidence: north refuses force pushes in north-guard's command check (crates/north-guard/src/guard/tripwire/publish.rs:56)
and in safe-push's arguments (crates/north-safe-push/src/request.rs:4). Both are client-side, like the patch. The
layer that holds for every client and token is the server.

Verdict: reject the binary. Adopt the rule it stands in for: a ruleset on each repository's main refusing
non-fast-forward updates and deletion (north#973). Lease pushes that fast-forward are unaffected, because GitHub judges
the ref update, not the client's flag.

## Claim 3: NixOS VM tests assert hosts before deploy

Against: `runNixOSTest` boots a QEMU VM per node. It needs KVM on the checking host, costs tens of seconds to minutes
and hundreds of MiB per node, and cannot sit in a landing path that aims at a 60 s p90 (north#902). It tests a
modelled network, not the fleet's real ssh keys, aliases or checkouts.

Evidence from the host incidents of the last two weeks:

- A build host lacked ssh aliases to the others (north#912), and gex45 had no checkout-sync timer (firn#50): both are
  facts of the evaluated configuration.
- Control-plane timers ran in the wrong slice (north#706): `Slice=` is set in firn modules, also an evaluated fact.
- gex45 read a south checkout 23 commits behind (north#884): runtime state that no configuration test sees.

The configuration facts are caught in milliseconds by an assertion over the evaluated system, which south already
uses: its gex45-farm-slice check asserts the runner units' slice and weight from `nixosConfigurations.gex45.config`.
A VM test adds value only for behaviour that exists at run time across processes, such as a firewall that must drop
a packet, and none of our incidents was of that kind.

Verdict: reject VM tests for now. Each of the fixes above (north#706, north#912, firn#50) gets an evaluation-time
assertion in south's flake checks, where the hosts are declared, the pattern south's gex45-farm-slice check sets
(south#85).

## Claim 4: agents may use sudo because generations roll back

Against, and the verdict is reject. A NixOS rollback restores the system profile, the boot entries and `/etc`
generated from the store. It restores nothing in home directories, `/var`, transcripts, unpushed branches, backups
or anything behind a cloud API, which is where the incidents of north#956 put the risk. Root can also delete the
generations a rollback needs, rewrite the boot loader, read every deployed secret and start processes outside any
worker sandbox, so the rollback guarantee does not survive the power that relies on it.

Evidence: agents run as the login user, so what that user may do as root bounds what an agent may do, whatever
the worker sandbox confines. The readings and the decisions left to Tom are held in south's record
(agents/decisions/harness/nix-leverage-2026-11-09.md); one change that no workflow needed landed in firn (firn
d97235a26c).

Verdict: reject. Rebuilds already run without agent sudo: `firn rebuild --landed` asks a root-owned socket service
to build a committed public snapshot (firn's modules/landed-rebuild).

## Claim 5: an overlay patches a vulnerable dependency everywhere

Against: an override of a library deep in the graph, the post's OpenSSL example, misses the binary cache for
everything above it and rebuilds the world on our own hosts. An overlay reaches only what one nixpkgs instance builds:
north's Rust crates come from Cargo.lock and the JavaScript packages from bun.lock, which no overlay touches.

Evidence: firn already applies one overlay set to every host through `mkSystem`, and pins bun and betterleaks
releases with overrides there. That override did not reach north's dev shell, which imports its own nixpkgs: the bun
drift under claim 1. North's flake lock and firn's are three days apart (2026-11-02 and 2026-11-05), so each flake's
own lock is the place a patch has to go.

Verdict: already in use. A patch that must cover the fleet goes in firn and in each project flake that carries the
package; for crates and npm packages it goes in the lock file through the dependency gate.

## Claim 6: Nix caches incrementally worse than Bazel and Buck2

Against building Rust through Nix (crane or naersk): a derivation per crate rebuilds a whole crate on any change,
cannot use rustc's incremental cache or a shared sccache across derivations, and re-vendors on every Cargo.lock
change.

Evidence: north uses Nix only to pin the toolchain and tools of the dev shell. Incremental work is Cargo's warm
targets, sccache, and verdict reuse keyed on the files a check actually read
([verdict-reuse.md](verdict-reuse.md), north#634, north#902). Where Nix's input-addressed caching fits, small pure
checks, firn already uses it: each flake check derivation reruns only when its file set changes.

Verdict: already done; reject Nix-built Rust.

## Claim 7: Nix-built container images

Not applicable: north ships no container image, and firn's farm-runner image is built from GitHub's actions-runner
image on purpose, to run jobs as GitHub's runner does.

## Claim 8: does an agent need Docker, a microVM, or only Linux and Nix?

Two threats decide it: an agent deleting files outside its lane, and a compromised cargo or npm dependency reading
credentials during install or build.

Options, each argued against:

- Docker Sandboxes ([docker-sandboxes](../references.md#docker-sandboxes)): one microVM per agent behind a hypervisor,
  workspace shared at the same path through filesystem passthrough, egress and API keys through a host proxy that
  swaps a placeholder for the real credential. Against: it isolates one agent per VM, while north's workers share one
  git common directory, a capacity scheduler that leases cgroups, offload over ssh, and systemd user units; each VM
  would need its own Nix store or a share of the host's. It is a vendor tool whose Linux support names Ubuntu with KVM,
  not NixOS, and adopting it is a dependency decision for Tom. Its credential proxy is the idea worth keeping.
- microvm.nix ([microvm-nix](../references.md#microvm-nix), MIT): declarative VMs on a NixOS host with eight
  hypervisors and virtiofs or 9p shares. Against: the same redesign of shared state, KVM on every host, and memory per
  worker. A kernel boundary matters if the threat includes kernel exploits; neither threat here does.
- NixOS containers ([nixos-containers](../references.md#nixos-containers)): systemd-nspawn declared in the system
  configuration. The manual says they are not perfectly isolated from the host, and creating one per worker means
  giving agents a root path to the container manager, which widens what they can do.
- The Nix build sandbox ([nix-conf-manual](../references.md#nix-conf-manual)): a build sees only its declared inputs
  in private namespaces, with no network except for fixed-output fetches. It is the strongest of all for threat two,
  but it confines only derivations, and turning lane builds into derivations is the incremental-cache loss of claim 6.

What north already runs: the Landlock worker sandbox, which confines removal for every command a worker runs
([worker-sandbox.md](worker-sandbox.md)), and depbox, a bubblewrap namespace in which dependency code runs with no
credentials and no network outside `depbox fetch` (`north depbox`).

Threat one, deleting outside the lane: the Landlock domain refuses the removal whatever path the command computes,
and its enforcement on every host is north#956's first box. north#975 holds the same guarantee for work a command
hands to another local process, with north's own verbs that need one going through a single north broker.

Threat two, a dependency reading credentials: land checks build in depbox, and north#974 makes every path that runs
dependency code do the same. bun runs install scripts only for its trusted list and a project's
`trustedDependencies` ([bun-lifecycle](../references.md#bun-lifecycle)), which narrows the install-time threat
without removing it.

Recommendation: stay on Linux primitives, extend them to every path (north#974, north#975) and enforce the worker
sandbox on every host (north#956). Cost: no new tool, no VM and no new user; the work is in north's own sandbox and
slot code, and each step goes through a canary first, since Landlock has no permissive mode. Proposal for later, not
adopted: a credential broker outside the domain, as Docker's proxy is, so the worker's ruleset can also govern reads.

## The companion repository

ghuntley/nix-demo carries no license file and its README states no terms; it credits its git patches to
ghuntley/loom. Nothing was copied from it. The bun pin in flake.nix repeats firn's own override.

## Adopted changes

- north#973: GitHub refuses a non-fast-forward update or deletion of main in every repository.
- north#974: every path that runs dependency code runs it in depbox's namespace.
- north#975: the worker sandbox covers removals a worker command hands to another process.
- south#85: south's flake checks assert the host facts behind north#706, north#912 and firn#50.
- firn d97235a26c: the containers module drops a group membership no workflow used.
- flake.nix: the dev shell's bun is the release the hosts run.

Decisions left to Tom and the proposals not adopted are in south's record.
