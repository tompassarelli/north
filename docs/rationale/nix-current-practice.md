# Current Nix practice, November 2026, against how Tom's repositories use Nix

Research of 2026-11-04 for north#553. Tom asked on 2026-11-05 (laptop time) that the Nix machinery follow current
upstream patterns and be regrounded continuously. Every upstream claim below cites the live page it came from and
was read on 2026-11-04; the model's own knowledge was not used for any version, status or date. Repository
references are to origin/main on 2026-11-04: north `f1745de2e`, nixos-config (firn) `3f9d625d`, wisp `dd4c2504`,
makgora `69e0798a2`. Sources are listed in [references.md](../references.md#nix-practice).

## Upstream state

### Releases

- Nix 2.35 is the current release line, latest patch 2.35.4 of 2026-10-23; 2.34 is past its line's end
  (2026-07-13, latest 2.34.9 of 2026-10-13); 2.33 and 2.32 are end of life
  ([endoflife.date](https://endoflife.date/api/nix.json)). The 2.35 notes date 2.35.0 to 2026-06-22, endoflife.date to
  2026-07-13 ([rl-2.35](https://nix.dev/manual/nix/latest/release-notes/rl-2.35)).
- Upstream supports the latest release and the version the stable NixOS release ships, so 2.34 stays supported
  through NixOS 26.05 ([release notes index](https://nix.dev/manual/nix/latest/release-notes/)).
- NixOS 26.05 "Yarara" shipped 2026-05-30 with support until 2026-12-31; 26.11 "Zokor" is next
  ([announcement](https://nixos.org/blog/announcements/2026/nixos-2605/)).


### What changed in the last 12 months

- 2.32 (2025-10-06): clients and daemons older than Nix 2.0 dropped; substituted flake inputs are no longer
  recopied; `nix flake check` skips substitutable derivations; derivation JSON uses basenames and the
  `nix derivation` commands are formally unstable; experimental `external-builders`
  ([rl-2.32](https://nix.dev/manual/nix/latest/release-notes/rl-2.32)).
- 2.33 (2025-12-10): `nix registry resolve`; `nix flake clone` for every input type; channel URLs moved to
  `https://channels.nixos.org/`; `download-buffer-size` defaults to 1 MiB; `nix path-info --json` without
  `--json-format` is deprecated; derivation JSON version 4
  ([rl-2.33](https://nix.dev/manual/nix/latest/release-notes/rl-2.33)).
- 2.34 (2026-02-27): Rust installer in beta; `lint-url-literals`, `lint-short-path-literals` and
  `lint-absolute-path-literals` (`ignore|warn|fatal`), with `no-url-literals` stabilised into the first and
  `warn-short-path-literals` deprecated; `narinfo-cache-meta-ttl`; mTLS substituters; `nix store gc --dry-run`
  ([rl-2.34](https://nix.dev/manual/nix/latest/release-notes/rl-2.34)).
- 2.35: flake and `fetchTarball` sources are hashed without a store copy first; mimalloc (5-12% faster
  evaluation); opt-in `http3`; `filetransfer-retry-*` settings (`download-attempts` renamed to
  `filetransfer-retry-attempts`); `nix flake check --out-link`/`--print-out-paths` and no out links by default;
  `nix store delete --skip-alive --also-referrers`; `nix realisation` renamed `nix store build-trace`
  ([rl-2.35](https://nix.dev/manual/nix/latest/release-notes/rl-2.35)).
- NixOS 25.11 made Python `nixos-rebuild-ng` the default and disabled `command-not-found`, which works only on
  channel-based systems; 26.05 removed the Bash `nixos-rebuild` and `system.rebuild.enableNg`, and added a
  channel-free `system.nix` entry point beside `configuration.nix` and `flake.nix`
  ([NixOS release notes](https://nixos.org/manual/nixos/stable/release-notes)).

### Flakes and the new CLI

- Both are still experimental: `flakes` and `nix-command` are listed among the 21 experimental features of the
  2.35.4 manual ([experimental features](https://nix.dev/manual/nix/latest/development/experimental-features)).
- The stabilisation tracker "CLI stabilization effort" is open; its last concrete progress is replacements for
  `nix-store --export/--import` (2025-09-12), and its first batch is `nix daemon`, `nar`, `hash`, `key`, `config`
  and `store` ([NixOS/nix#7701](https://github.com/NixOS/nix/issues/7701)). RFC 136 (accepted 2023) is the
  incremental plan, ending in a separate flakes-design step
  ([RFC 136](https://github.com/NixOS/rfcs/blob/master/rfcs/0136-stabilize-incrementally.md)).
- LWN on 2026-10-21: no target completion date; 78.9% of the 2025 community survey's respondents use flakes;
  Determinate Nix ships flakes as stable and on by default, while Lix treats them as frozen, to be extracted into a
  plugin ([LWN](https://lwn.net/Articles/1094226/)).
- nix.dev calls flakes "an experimental extension format with outstanding issues" and points authors of new Nix
  code at npins or fetchers instead; it still documents flakes for existing flake projects
  ([nix.dev flakes](https://nix.dev/concepts/flakes)). nix.dev remains the official documentation home with no
  successor ([nix.dev](https://nix.dev/)).
- What replaced what: `nix develop` and `nix shell` replace `nix-shell`, `nix build` replaces `nix-build`,
  `nix profile` stands beside `nix-env`. None of the old commands is deprecated in the manual: the `nix-env` page
  carries no warning and calls `nix profile` experimental
  ([nix-env](https://nix.dev/manual/nix/latest/command-ref/nix-env)). Channels are not deprecated either; NixOS
  only adds channel-free routes (`system.nix`, flakes) and drops features that need channels.

### Patterns

- Dev shells: flake `devShells` entered with `nix develop`, or `shell.nix` with `nix-shell`; nix.dev's direnv
  guide uses `shell.nix` with `use nix` and asks for nix-direnv first
  ([nix.dev direnv](https://nix.dev/guides/recipes/direnv)). nix-direnv 3.2.0 (2026-07-28) is current; 3.1.1 added
  GC-root refresh so a collector does not delete active shells, and `nix_direnv_disallow_fallback`
  ([nix-direnv releases](https://github.com/nix-community/nix-direnv/releases)).
- Formatting: `nix fmt` is an alias of `nix formatter run` and runs the flake's `formatter.<system>` output; the
  manual's example is `nixfmt-tree`, the official formatter
  ([nix fmt](https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-fmt)).
- Lock files: Nix writes `flake.lock` itself and pins direct and transitive inputs; `nix flake lock` adds missing
  entries and `nix flake update` updates them; a Git flake reads only files added to Git, and a dirty tree warns
  ([nix flake](https://nix.dev/manual/nix/latest/command-ref/new-cli/nix3-flake)). Flakes evaluate purely by default,
  and nix.dev warns that pure mode still does not guarantee reproducibility
  ([nix.dev flakes](https://nix.dev/concepts/flakes)).
- Trust and caches: `trusted-users` "is essentially equivalent to giving that user root access"; `require-sigs`
  defaults on; `accept-flake-config` defaults off; `allow-import-from-derivation` defaults on; `lint-url-literals`
  defaults to `ignore` with unquoted URLs being deprecated
  ([nix.conf](https://nix.dev/manual/nix/latest/command-ref/conf-file)).
- CI: `cachix/install-nix-action` v31.11.3 (2026-10-27) bundles Nix 2.35.4 and drops the compromised eMudhra
  certificates ([install-nix-action releases](https://github.com/cachix/install-nix-action/releases)).

## Installed Nix

- gex45: `nix --version` printed `nix (Nix) 2.34.9` on 2026-11-04.
- The laptop (whiterabbit) and nexus: gex45 has no ssh route to either (`laptop` does not resolve; nexus refuses
  gex45's key), so the version is the one their deployed configuration evaluates to:
  `nix eval --raw .#nixosConfigurations.{whiterabbit,nexus,gex45}.config.nix.package.version` in south printed
  `2.34.9` for all three; gex45's observed binary matches its evaluation. A host not rebuilt since its last lock
  bump could differ; the next session on each host should replace this line with its own `nix --version`.
- 2.34.9 is the NixOS 26.05 version, supported, one minor behind the 2.35 line.

## Gap list

Ratings: current (matches upstream practice), outdated (works, but upstream moved), risky (can break or widen
trust).

### Version and features

- All three hosts run 2.34.9 from nixos-26.05 (above). Current: the stable NixOS version is supported; 2.35 brings
  only speed and fetch changes we do not depend on.
- `modules/nix-settings/default.nix:20-23` (firn) enables `nix-command flakes`. Current and required, since both
  remain experimental; gex45 shows `fetch-tree` added implicitly by `flakes`.

### Dev shells

- north `flake.nix:12-20`, wisp `window/flake.nix:16-24`, makgora `flake.nix:11-20`: flake `devShells.default`
  entered with `nix develop`. Current.
- north `bin/land-shell:10`, `harness/scripts/citation-check.sh:8`,
  `harness/scripts/check-owners.sh:9`,
  `crates/north-runtime/src/land_offload/ship.rs:66-67`: `nix develop -c CMD`. Current.
- north `bin/depbox:43-50` roots the dev shell with `nix print-dev-env --profile`, keyed on the tracked `*.nix`,
  `flake.lock` and toolchain files, with `--no-update-lock-file`. Current: the same GC-root idea nix-direnv 3.1.1
  adopted, and it keeps the weekly `nix.gc` of `modules/nix-settings/default.nix:11-15` from deleting the shell.
  Risky edge: roots unused for 7 days are deleted (`bin/depbox:46`), and the 30-day GC can then collect an idle
  checkout's shell, which on an offline host means a failed build (observed on gex45 before depbox rooted shells).
- north `flake.nix:18` and wisp `window/flake.nix:22` run `north target-seed` in `shellHook`. Outdated against the
  skill's minimal-hook rule: every `nix develop -c` pays it and a failing seed is hidden by `|| true`.
- makgora `flake.nix:13-18`: `shellHook` runs `exit 1` when the pinned addon is missing, so every
  `nix develop -c CMD`, including commands that never use Blender, fails on a host without the build-input store.
  Risky.
- north `flake.nix:13` puts `python3.withPackages [ websockets ]` in the dev shell. Not a Nix-practice gap; it
  conflicts with the no-Python rule and should leave with its last caller.
- firn `modules/direnv/default.nix:9-16` enables direnv with nix-direnv. Current. No first-party repository has an
  `.envrc` (the ones under `repos/effect/` are the vendored Effect tree), which is right for agents that call
  `nix develop -c` explicitly.

### Flake structure and formatting

- No repository declares a `formatter` output (north `flake.nix`, firn `flake.nix`, wisp `window/flake.nix`,
  makgora `flake.nix`), so `nix fmt` does nothing; firn runs `nixfmt --check` from its own checks
  (`docs/verification.md:13`, `README.md:13`). Outdated: add `formatter.x86_64-linux = pkgs.nixfmt-tree` where Nix
  is edited (firn first) so `nix fmt` and the check agree.
- north `flake.nix:10-24` nests lambdas to bind `system`, `pkgs` and the toolchain, and hardcodes
  `x86_64-linux`. Works; harder to read than a `let` block (as wisp `window/flake.nix:10-15` does) and no
  `checks` output, so `nix flake check` proves only evaluation.
- firn `flake.nix:443-470`: the `nix-syntax` check parses every file with `nix-instantiate --parse`. Current;
  the legacy command is not deprecated. A stricter variant is `lint-url-literals = fatal` (2.34).

### Lock files and pins

- north `flake.nix:4` and wisp `window/flake.nix:4` follow `nixos-unstable`, both locked to 2026-08-31 with
  rust-overlay of 2026-09-01, while hosts run nixos-26.05 locked to 2026-11-02. Current pattern, outdated lock: two
  months behind, and a second nixpkgs to download and keep beside the system's. Gated updates go through
  `crates/north-landing/src/dep_gate.rs:735` (`lock_candidates`), which is the right place to keep the age bounded.
- makgora `flake.nix:2` pins nixpkgs by a raw revision in the URL (`github:NixOS/nixpkgs/e1d7762c…`, 2026-09-16).
  Outdated: `nix flake update` cannot move it, so the lock policy and dep gate never see it age; a branch URL with
  the lock as the pin is the usual form.
- firn `flake.nix:32` pins elephant by revision without `inputs.nixpkgs.follows`, and walker follows elephant
  (`flake.nix:38-40`), so `flake.lock` carries an unfollowed nixos-unstable of 2025-10-12. Risky in a small way: a
  year-old nixpkgs in the system closure that no update touches.
- firn `flake.nix:219-226` and `427-434` import `nixpkgs-unstable` and `nixpkgs-master` as overlays. Common and
  current; each import costs evaluation time and `master` is unreviewed by Hydra.
- `--no-update-lock-file` (north `bin/depbox:50`) and `--no-write-lock-file`
  (`crates/north-core/src/agent_config_check/codex.rs:195`) keep agents from rewriting locks. Current.

### Pure evaluation, channels and NIX_PATH

- Flakes evaluate purely (gex45 `nix config show`: `pure-eval = true`). Current.
- gex45 `NIX_PATH=nixpkgs=flake:nixpkgs:/nix/var/nix/profiles/per-user/root/channels`: the system flake's
  nixpkgs is first, but the root channels profile is still on the path because firn never sets
  `nix.channel.enable = false`. Outdated: a flake-built host can drop channels, which 26.05's `system.nix` and the
  25.11 `command-not-found` change both point toward.
- firn `.github/workflows/build.yml:15` and `.github/workflows/tests.yml:33` set
  `nix_path: nixpkgs=channel:nixos-25.05`, an end-of-life channel, for any `<nixpkgs>` lookup on CI. Risky: a test
  that reaches `<nixpkgs>` runs against 25.05 while the hosts run 26.05.
- firn `.github/workflows/build.yml:13` pins `cachix/install-nix-action` v27; `tests.yml:31` pins v31.11.3.
  Outdated: v27 predates the current Nix line and the certificate fix.

### Substituters and trust

- firn `modules/nix-settings/default.nix:24-37` adds five Cachix caches (nix-community, walker, walker-git,
  devenv, quickshell) with their keys, `require-sigs` left on. Current form; `devenv.cachix.org` has no devenv
  consumer in any of the four repositories, so it widens trust for nothing.
- firn `modules/nix-settings/default.nix:41-44` sets `trusted-users = [ "root" "@wheel" ]`. Risky on the laptop only: the manual calls a
  trusted user root-equivalent, and the users module default (`modules/users/users.nix:50-56`) puts Tom in wheel
  there, so any agent on the laptop can add a substituter or import unsigned paths; gex45 and nexus override the
  groups without wheel (south `hosts/gex45/configuration.nix:15`, `hosts/nexus/configuration.nix:11`; gex45 `id`
  shows no wheel). `accept-flake-config` stays false (gex45), which is current.
- `keep-outputs` and `keep-derivations` (`modules/nix-settings/default.nix:38-39`) and `auto-optimise-store`
  (`:17`). Current.

### Deprecated tools

- No first-party code calls `nix-shell`, `nix-env` or `nix-channel` for work; firn's
  `native/firn_rebuild_foreign_host.test.sh:11` only lists `nix-env` among tools a foreign host must have, and
  north's `crates/north-core/src/capacity/dev_shell.rs:26` only recognises a `nix-shell` parent. Current.

## Follow-ups

The gap list goes to the Nix footguns plan, whose guards should target these patterns. The highest-value changes,
in order: keep laptop agents out of `trusted-users` (drop `@wheel` or Tom from wheel there), move firn CI off the 25.05 channel and
v27 action, set `nix.channel.enable = false`, bring north's and wisp's locks and makgora's pin under the dep gate's
age bound, and add `formatter` outputs. The nix-development skill's grounding (2026-10-15) should cite this note
until the watch entries reground it.
