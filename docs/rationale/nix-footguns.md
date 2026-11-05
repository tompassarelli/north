# Nix and dev-shell footguns: every failure class and the guard that closes it

Plan of 2026-11-05 (laptop time, +07) for north#553. Tom said Nix assumptions keep wrecking productivity (2026-11-05),
after three Nix or dev-shell classes refused or hung lander candidates in one night. This file holds the evidence from
2026-10-05 to 2026-11-05, the classes it falls into, the deterministic guard per class, the guards that exist and their
gaps, and the boxes in landing order. Nothing here is built yet.

## Sources and method

- Session transcripts on gex45: `~/.local/state/north/profiles/claude-{a,b,c,d}/projects/**/*.jsonl`, the retired
  `stale-claude-a-20261026` profile and `~/.claude/projects`, files changed since 2026-10-05, searched for `nix develop`,
  `nix-shell`, `command not found`, `exit 127`, `depbox`, `land-shell`, `dev shell`, `capacity gate`, `flake.lock`,
  `/nix/store`, `eval-cache`, `substituter`, `unable to download`, `is not valid`, `gc root`. Repeats of one cause in one
  session count once. A session prefix below is the first 8 characters of the transcript file name.
- The `claude-b` and `claude-c` profiles were rescanned on 2026-11-05: 4,643 jsonl files under their `projects`
  directories changed since 2026-10-05 (400 MB), searched for the capacity gate message, `land-shell`, `depbox`,
  `unable to download`, `nix-collect-garbage`, `NORTH_DEV_SHELL`, `IN_NIX_SHELL` and a missing `/nix/store` path, which
  gave 1,383 hits in 118 transcripts. Each of the 118 was then read for tool results with `command not found`, exit
  127, `unable to download`, `Cannot build`, `is not valid`, a missing linker, an eval-cache lock or an assistant
  sentence about a refusal, and only real failures were kept (a session that merely names these words is not one).
  Most rescan sessions repeat a cause already in a row, so they raise that row's session count; four rows are new
  (N10 exit in the shellHook, N11 land-shell home and `.git`, N12 missing system tools and `convo`). Rescan minutes
  use the 1 to 2 minutes per bare-tool failure below, except where a row gives its own.
- The laptop proxy transcript (`proxy-transcript-895929a7.jsonl`, to 03:23 on 2026-11-05) and the lander logs of
  2026-11-05 00:40 to 03:48 (`~/.cache/north-evidence/nix-evidence/`, `lander-journal-0230-0330.txt`).
- `gh issue list --state all --search "nix OR dev-shell OR depbox OR land-shell OR flake OR 127 OR shim"` on north, wisp,
  makgora and firn (the system repository; `tompassarelli/nixos-config` does not exist on GitHub), then the bodies.
- `git log --all --since=2026-10-05` grepped for nix, dev shell, depbox, land-shell, flake, shim and 127 in north and
  nixos-config, reading each fix commit's body.
- South `agents/incidents.jsonl`: 541 rows to 2026-10-29 and none Nix-specific; inc-407 (nexus autoUpgrade, 88 min) is
  the one related row.
- The live 127 of 2026-11-05 is taken from the nix-gate-127 session's diagnosis, not redone.

Minutes are wall time from the first failure to the working rerun, read from transcript timestamps; where a source
states a cost, that cost is used. Most bare-tool failures were fixed in the same turn and count 1 to 2 minutes each.
"Refused" means a landing candidate the lander or safe-push turned away.

## Evidence

One row per distinct incident; a pattern that recurred across many sessions is one row with its session count. Dates
are UTC unless a time carries +07. Ref is an issue, a commit, a lander candidate or a session prefix.

### N1. Nested dev shells and sandboxes

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-22 | north | north#124, 279cb3552 | depbox bwrap inside run-bounded | sandboxes nest | pid namespace mismatch, `open /proc/608/ns/ns` | workers ran safe-push outside the wrapper until the fix |
| 10-24 | north | north#172, ad0e2863 | `run-bounded -- git ls-remote` over ssh | a user namespace is transparent | the root-owned store ssh config read as owned by nobody; ssh refused it | wisp#43 landing blocked |
| 10-28 to 11-04 | north, smashcraft | 9822f8ba, b8214159, 74558b22, 25fc6e18, e9706fc9 | `safe-push.test.sh`, `run-bounded.test.sh`, and `run-bounded` itself under `nix develop` | bash is bash | `compgen: command not found`: the dev shell's bash lacks the completion builtins (e9706fc9, 11-01: `run-bounded: line 74`, 8 times, and `safe-push: command not found` beside it) | 5 incidents, about 21 min, 1 land check failed |
| 10-31 to 11-05 | north | 30+ sessions (fb78ac2b, 0bb9203a, 22319a32, 045f70ad, 534a0712, …); lander logs | any `land-shell --dev` or offload `nix develop` | the shellHook works everywhere | `/tmp/nix-shell.XXXX: line 2187 or 2189: north: command not found`; depbox hides `~/.local`, so `target-seed` is skipped | not fatal; every such build starts cold (unmeasured) |
| 11-05 03:00 +07 | north | n360-default-checks | `./bin/land-shell --dev -- target/debug/north land checks …` | the wrapper is harmless | every check exited 127: land-shell's depbox nested in the checks' own | one full portable run (727 s rerun) |
| 11-05 03:38 to 03:48 +07 | north lander | candidates bd9741afc (5 lanes), bc1e523bc (3 lanes); nix-gate-127 | the offload's `nix develop "$slot"`, then `check-owners.sh`, `citation-check.sh`, `experiments-check.sh` each `exec nix develop -c cargo run …` | a nested `nix develop` is harmless | `nix: command not found behind the north capacity gate`, exit 127, on every candidate (the nesting is the trigger; the gate's loop guard, N2, the cause) | 6 checks refused 2 candidates (8 lanes); the lander landed nothing from 3:42 AM (bc1e523bc also had a real E0425) |

### N2. PATH, shims and the real program behind the capacity gate

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-24 to 10-29 | north, smashcraft, wisp | about 68 sessions (149a1782, 88d46073, f1dfa3c4, 4527a4f5, …) | bare `cargo test`, `cargo clippy` | cargo is on the global PATH | `cargo: command not found`; cargo exists only in the dev shell | about 130 min, one rerun each |
| 10-27 to 11-04 | north, wisp, smashcraft, makgora | about 140 sessions (d63af5ec 15 min, 5119499b, 8017c75e, 56fe22bd, 12354466, …) | bare `cargo`, `cargo fmt`, `run-bounded 20m -- cargo build` | the PATH cargo is real | it is the gate shim: `cargo: command not found behind the north capacity gate`, 127 | about 250 min; 4 relay tests false-red (ad5fb29b) |
| 10-29 to 11-04 | north | 10 sessions (5fbb5c01, 95650e4a, 87bfb858, …), north#465 | backgrounded gated `cargo test` | the background exit is the test result | not-found line, `RELEASED`, `exited with code 0`: a false green | wasted waits; risk of landing untested code; fixed by 5c707090e |
| 10-30 | north | north#331, d85709144 | hourly `code-health-tick` | the bun shim finds a real bun | the unit PATH had no Nix profile; 127 | hourly failures |
| 10-30 | north | north#245, 408f5e72e | `capacity_north_path` tests in `nix develop` | the first bun on PATH is real | it is the shim; the fake PATH lacked readlink and bun | 2 tests failed |
| 10-30 | wisp | wisp#120, 100e77a0; 5119499b | `bun wisp net-accept --relay local` builds the relay | cargo is on PATH | only the gate shim: not found behind the gate, 12 times | every match failed in 0.04 s; a 73 min fix session |
| 10-26 to 11-04 | wisp, smashcraft, north | 118fd457, 7628c94f, b8380717, 5a3b5b24, a329536a, 8017c75e, … | gated cargo for the relay with the rustup toolchain | the toolchain can link | `error: linker cc not found`; gex45 has no cc outside the dev shell | 15 incidents, about 30 min measured (one after a 14 min lease wait); the 10 of the rescan unmeasured |
| 10-28, 11-04 | north | 36e16e34, 5e4e3864, 719d38b1, … | `rustfmt`, `shellcheck`, `actionlint`, `pre-commit` | on PATH | not found; agents skipped the check | 8 incidents, about 10 min; checks skipped |
| 11-05 00:42 to 02:48 +07 | north lander | candidates 09d33a422, 4923f605, 4652200d, bd0a52a7, eeb5c1f6; 63d9e7783 | inferred `bun test` in `safe-push.test.sh`'s train-deps fixture | the gate's lookup skips stubs | the stub says "capacity-gate", `is_shim` wants "capacity gate", so the gate exec'd the stub in a silent loop until the 2100 s deadline | 71 min confirmed (4923f605, bd0a52a7, eeb5c1f6), 133 min with the two probable; 3 candidates refused, one ticket withdrawn |
| 11-05 03:42 +07 | north lander | nix-gate-127 | the N1 nested `nix develop` | the loop guard won't skip a real nix | 63d9e7783's per-process guard skipped the real nix | the N1 row's cost; counted there |

### N3. Leased or offloaded runs that lose or never enter the dev shell

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-31 | north | north#391, 148518d45; 4a0a887d, cb3d7a88 | heavy `cargo test` offloaded laptop to gex45 | the remote side has cargo | not found behind the gate; the gated PATH lacked `~/.local/bin` | at least 1 refused build; the six-build measurement stalled |
| 10-31 | wisp | wisp-api-g1-land, g2-now | relay tests in fleet sessions | the service PATH equals the login PATH | the service lacked `~/.local/bin` | 3+ sessions, about 20 min |
| 11-01 | north, wisp | north#465, a7ab53869, 5c707090e; boxes 2f, 3d, wisp-water-crests | cargo through `capacity host run` and the lease wrappers | a lease enters the dev shell | bare PATH: 127, one exit 0 with no tests (false green), 3 window builds failed | 1 false green, about 5 failed runs, a 45 min fix |
| 11-04 07:00 to 07:35 +07 | north | three workers; 602d51c9b | `north capacity offload run --host gex45 -- cargo …` | cargo is on the slot's PATH | `cargo: command not found behind the north capacity gate` | 3 workers re-ran by hand; one suite counted as not run |
| 11-04 10:51 +07 | north lander | train 6c, fails-first-gate | offload `land-shell --dev -- cargo run … land-fails-first` | the landing variables are set remotely | `usage: north land-fails-first …`, exit 75 | train 6c stopped and relaunched as 6d (14 lanes) |
| 11-04 11:11 +07 | north | proxy preflight8 | offload `cargo clippy` with no shell wrapper | it runs in the dev shell | `exit 1` with no error lines | one rerun, about 2 min |
| 11-04 | north | north#546 (open); 7c98ac5c | `north capacity exec --dev-shell DIR -- cargo --version` | the flag enters the shell | it sets a marker and execs from the current PATH; 127 | blocks leased runs that use it; about 5 min |
| 11-04 16:05 | north | b7f35960 | a stress loop running the test binary outside `nix develop` | the binary is self-contained | `sccache rustc -vV … cannot find binary path` | about 5 min; a 20-run baseline was noisy |

### N4. Nix forms that skip the capacity lease

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-29 | north | north#359, be775aee0; d8e4d736 | `run-bounded … depbox run -- nix develop -c cargo test` | depbox takes a lease | its gate sat in a pid-namespace branch that never ran; no quota | about 30 min; load beside queued heavy builds |
| 10-30 | north | north#383, c2d33e669 | `nix develop --command cargo build --release -p wisp-window` over ssh | the cargo shim gates every build | the shell's cargo came before the shim; 10 cores in an ssh login scope | gex45 load 105, then 59, uncappable |
| 10-30 | north | north#387 (open); review bb041f4e | `nix build`, `nix -L develop`, `nix --extra-experimental-features … develop`, `nix-shell --run` | the gate parses every Nix form | only `nix develop -c` with `develop` first is parsed | found in review; open |
| 11-04 17:47 | north | 2365ce64 | a timing baseline through depbox | it ran under a lease | the depbox gate took no lease; it ran in the session cgroup under a 1-core cap | run 1 invalid, about 15 min |

### N5. Offline, eval-cache and substituter assumptions

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-26 to 10-28 | north | 4befdb3a (10-28 05:03), ae42fdb6 (10-26 20:20), 9bb7abdb (10-27 12:50) | offline `nix develop` in depbox | the shell is cached | `unable to download …nixpkgs/archive/34ab9907….tar.gz: Could not resolve hostname` | 3 sessions, about 5 min (ae42fdb6 fell back to the rustup toolchain) |
| 10-29 11:47 | north | 1d5635a3 | `depbox run -- nix develop -c cargo metadata --offline` after a Cargo.lock change | the crates are fetched | `attempting to make an HTTP request, but --offline` | about 2 min |
| 10-30 | nixos-config | firn#23, 6f902bcf | `firn check` (`flake check --no-build`) on a fresh store | `--no-build` realises nothing | stylix's base16 needs realisation | main red |
| 10-29 to 11-05 night | north | every offload slot; 53a87de2 (10-29 19:50), d32fedaa (10-31 09:50) | `nix develop` in 6 slots at once, or two shells at once | — | `error (ignored): SQLite database …eval-cache-v6… is busy` | no failure; misled error greps (12 lines in one log) |

### N6. `nix develop` against `nix shell`, `nix run`, `nix-shell -p` and direnv

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-11 | north | ca45505e9 (north#208, #209) | capacity sizing of `nix develop -c X` | a Nix run is one job shape | the wrapper hid the inner command; 0.9- and 2-core jobs mixed | estimate replay at 65%; 537 of 2146 rows dropped |
| 10-24 | wisp | 9af2a6f1 | `mutants.sh`: `nix shell nixpkgs#cargo nixpkgs#rustc nixpkgs#gcc -c cargo test` | same as the project shell | an unpinned toolchain; no error | silent drift |
| 10-28 03:54 | south | de1e7f97 | `depbox run -- nix develop -c true` in south | every repo has a dev shell | `does not provide attribute devShells.x86_64-linux.default` | a probe |
| 10-30 | wisp | wisp#129, b0abf604; 5c256002, a329536a | wisp-window build in north's shell | it has pkg-config and libudev | it did not (`pkg-config: command not found` in the two rescan sessions); workers used `nix shell nixpkgs#pkg-config` and a hand-set `PKG_CONFIG_PATH` | every native-window worker repeated it |
| 11-04 13:29 +07 | nixos-config | keyring-hibernate | a formatter call | the flake has a formatter | `does not provide attribute 'formatter.x86_64-linux'` | negligible |

### N7. Project flakes against the system closure

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-11 | nixos-config | a21bcb8f, inc-407 | nexus `system.autoUpgrade` | empty flags still use the flake | forcing `[]` fell back to channels | part of an 88 min chain |
| 10-24 | firn | firn#32, a48c7cf2 | `firn rebuild` from a south worktree | any toplevel flake.nix is the system flake | south was taken as the system repo | a failed rebuild |
| 10-24, 10-25 | firn | firn#31 (open), 53acabf0 | concurrent `firn rebuild --landed` | the service uses the installed south flake | it used the south path from its start; generations 1401 to 1403 dropped a timer | open |
| 10-25 | firn, wisp | firn#38 (open), wisp#174 | laptop relay build | cargo, rustc and cc are on the agent PATH | they exist only in store paths or dev shells | every laptop Rust worker re-finds them |
| 10-29 | north | north#351, cbf0bd365 | agent-config-check on gex45 | shellcheck is on PATH | only in the north flake; 0 hooks linted, passing | red and unread for days |
| 10-29 | north | north#331, nixos-config 3ac81d4e | `lane-gc.service` | the landed config is live | the deployed closure ran the deleted `bin/lane-gc`; 127 | 13 failed runs |
| 10-29 | nixos-config | b0758a15 | nixos-config land check on gex45 | a light check | it built the laptop's system closure on gex45 with 1.7 GiB free | 2 landings blocked |
| 10-29 | nixos-config | e73b8d69 | commit with the pre-commit hook | its tools are present | `pre-commit: command not found`; it called a check north no longer ships | about 10 min |
| 10-30 | wisp | wisp#130 | `bun wisp play --standalone --window native` | NixOS has the Wayland, X11 and Vulkan libraries | they are only on the dev shell's `LD_LIBRARY_PATH` | not stated |
| 11-04 10:31 +07 | nixos-config | 2aa6ed03 | the Codex module's `--policy-only` | its file list matches north | it listed the retired `lib/spawn_capacity.py` | broke `--policy-only` |
| 11-05 02:06 +07 | nixos-config, north | north-land.service | `north land serve --all` | the live runtime has `--all` | usage error, exit 64, crash loop | killed a candidate check, orphaned a slot 27 min |

### N8. Store paths and host impurity in tests and goldens

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-16 | north | d95c1b3dc | depbox cargo build on Ubuntu CI | cargo is in the store | Ubuntu has it in `~/.cargo/bin`, which the sandbox hid | main red |
| 10-16 | north | d2f972052 | deletion-guard fixture | `/nix/var` exists | Ubuntu CI has none | main red |
| 10-24 | north | north#161, 54143fd25 | `depbox run -- nix develop -c cargo test` | the lane target is fresh | another checkout's binaries were reused; the fix keyed `RUSTFLAGS` on the checkout | 3+ failed land checks |
| 10-28 | north | north#272, 747d84a50 | main Cargo run on Ubuntu | `/run/current-system` exists, and the stub runs under bash | NixOS-only path in a fixture; `exec --` under dash, 127 | main red on every run until the fix |
| 10-28, 11-03 | north | edea698a, f7025b9e, 6d98b770 | tests with `env -i PATH=<minimal>` | git, grep, readlink and dirname sit in a standard place | not found; NixOS keeps them in `/run/current-system/sw/bin` (6d98b770: `~/.local/bin/north: line 6: readlink: command not found`) | 3 incidents, about 11 min |
| 11-04 | north | 472d13847 | runtime promotion in a `runtime-<sha8>` lane | the shell's env doesn't depend on path | the checkout path in `RUSTFLAGS` recompiled every registry crate | 10 to 25 min per promotion |
| 11-04 | north | ad3b7ad28 (north#360) | land checks with `CARGO_TARGET_DIR=target` | sibling artifacts are reused | the key applied to the own target: 146 crates per landing | 47 to 333 s per landing |

### N9. GC roots, pins and flake.lock drift

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-14 | wisp | wisp#7, 1d6de454b7 | `bun wisp lan fresh MAP --pair 0` | a binary linked to a store StormLib stays valid | GC collected it; map-pack exited 127, reported as a missing `war3map.w3i` | not stated |
| 10-26 | north | north#227, #228, #229; 90618140a, 2d43dfc83 | `nix-store --gc` on gex45 | promoted runtimes are rooted | the glibc loader went; every `north` call exited 127 | an outage until `nix-store -r`; 15 runtimes rooted by hand; collections frozen |
| 10-27 23:18 | north | 6430b666, ecd62e9e2 | `run-bounded 20m -- nix-collect-garbage`, then offline mutants in depbox | the dev shell survives GC | `Cannot build …nix-shell-env.drv` (an mpfr tarball needed) | 2 min, then a 40 min fix |
| 10-24 20:40, 10-27 21:23 | nixos-config, south | 36e16e34; 332222a5 | safe-push drift land check; a whiterabbit check from a south worktree | eval works | `path '…base16-schemes-…drv' is not valid` (9 times in 332222a5, retried with the eval cache off) | 1 landing refused; about 9 min in 332222a5 |
| 10-28 to 11-04 | smashcraft, makgora, wisp | firn#47 (open); 58dd2a85, 763da75a, 83e2126f | relay or pad helper build with rustup 1.96 or 1.97 | the toolchain is self-contained | its `ld.lld` wrapper execs a collected `rustup-1.29.0/nix-support/ld-wrapper.sh` | 4 incidents, about 25 min; 4 relay tests red |
| 10-30 | nixos-config | firn#23, ec1323d5 | CI statix | the transitive nixpkgs node is the flake's | a different node gave a different statix | main red |
| 11-04 | nixos-config | 228d3a62 (wisp#230) | Bun 1.4.4 | bump the nixpkgs lock | a younger nixpkgs-master lock fails the dependency cooldown | a hand override |

### N10 to N12

| Date | Repo | Ref | Command | Assumed | What happened | Cost |
| --- | --- | --- | --- | --- | --- | --- |
| 10-31 to 11-05 | north | the N1 shellHook row | `north target-seed \|\| true` in the shellHook | the hook is pure | it hides a missing `north` | counted in N1 |
| 10-26 | smashcraft | 39918a18 (19:29), 16c3809f (21:04) | `nix develop` for the map-pack helper build on gex45 | the shellHook only sets up the shell | it ran `exit 1` with `Missing pinned mdl-exporter4 input`: the Blender add-on store is absent on gex45, so the whole shell failed | 2 sessions, unmeasured; both fell back to plain nixpkgs cargo |
| 10-28, 11-04 | north | f1749039, 35c3c25c | scripts in `/tmp` or `~/.cache` through depbox | the same filesystem view | `No such file`, exit 127 | 2 incidents, about 10 min (N11) |
| 10-29 | north | 59081361 | a mutant sweep and `~/.local/state` patches in `land-shell --dev` | the sandbox shows the same home and a writable `.git` | `~` resolved differently, `~/.local/state` was hidden, and `git checkout` failed on the read-only `.git`, so mutants stacked and the results were void | about 5 min (14:24 to 14:28); one sweep voided (N11) |
| 10-24 to 11-04 | wisp, smashcraft, north, makgora | about 65 sessions (694b2a05, 9af2a6f1, f1dfa3c4, eb491a98, 3bdff76b, db3da684, …) | `machine-capacity run`, as skills and hook messages say | it is on PATH | `machine-capacity: command not found` on gex45 | about 130 min; a map build never ran (N12) |
| 10-24 to 11-03 | smashcraft, wisp, makgora | about 10 sessions (b6d0cb51, f7cca1d8, …) | `node …/machine-capacity.mjs` | node is on PATH | not found; only bun is installed | about 19 min (N12) |
| 10-24 to 11-02 | north, smashcraft, wisp | about 28 sessions (332222a5, a203f8fa, 442f9b9e, 03b0668b, c243fb93, …) | `file`, `strings`, `xxd`, `sudo`, `crontab`, `fuser`, `pstree`, `tailscale`, `vulkaninfo`, `aplay`, `grim`, `xdotool` | the usual Linux tools are on the host PATH | `<tool>: command not found` from `/run/current-system/sw/bin/bash`; file (11 sessions), strings (6) and tailscale (4) most often | about 28 min at 1 min each (N12) |
| 10-31 to 11-01 | north, wisp, smashcraft | 5 sessions (5a3b5b24, 5ff5d895, aadc0a07, c70316bc, a203f8fa) | `convo`, as the todo skill says | the skill's program is on PATH | `bash: line 1: convo: command not found` on gex45 | about 5 min (N12) |
| 10-24, 10-25 | north | north#178, b9c5ff48 | `safe-push.test.sh` land check | PATH is stable during a run | a profile switch replaced `~/.nix-profile` mid-run | 2 failed landings (N12) |

## Totals by class

Rows are the distinct incidents above; sessions count every session that hit a row's pattern. Minutes add the stated
or measured costs, and the rescan sessions at 1 to 2 minutes each, so an unmeasured cost (cold builds, hourly failures, days of a false green) adds nothing and
is named instead.

| Class | Rows | Sessions | Minutes | Landings and runs |
| --- | --- | --- | --- | --- |
| N1 nested shells and sandboxes | 6 | about 37 | about 80 (21 compgen, 12 rerun, 45+ of lander stall from 3:42 AM) | 2 candidates (8 lanes) refused, 1 check failed; every shellHook build cold |
| N2 PATH and shims behind the gate | 10 | about 230 | about 630 | 3 candidates refused or withdrawn by the exec loop, 10 false greens, hourly unit failures |
| N3 leased or offloaded runs | 8 | about 15 | about 140 | 1 false green, a 14-lane train relaunched, 1 refused offload build |
| N4 forms that skip the lease | 4 | 4 | about 45 | 0; gex45 load 105 once, uncappable |
| N5 offline, eval-cache, substituter | 4 | 7 | about 7 | firn main red once |
| N6 develop, shell, run, direnv | 5 | 7 | under 5 | sizing replay at 65%, silent unpinned toolchains |
| N7 project flake against system closure | 11 | about 15 | about 125 (88 nexus chain, 27 orphaned slot, 10) | 2 landings blocked, 13 failed unit runs, a lander crash loop, 0 hooks linted for days |
| N8 store paths and host impurity | 7 | 7 | about 11, plus 10 to 25 per runtime promotion and 47 to 333 s per landing until 11-04 | main red 3 times, 3+ failed land checks |
| N9 GC roots, pins, lock drift | 7 | about 9 | about 80, plus a gex45 outage | 1 landing refused, main red once, collections frozen |
| N10 shellHook side effects | 1, plus the counts in N1 and N8 | 2 | unmeasured | 2 fallbacks to plain cargo |
| N11 sandbox visibility | 2 | 3 | about 15 | 1 mutant sweep voided |
| N12 PATH differs by context | 5 | about 108 | about 180 | 2 failed landings |
| Total | 70 | about 440 | about 1,320 | 5 candidates and 2 landings refused, 1 train relaunched, 11 false greens, 3 red mains in north |

By cost the order is N2, N3, N7 and N12 (most of them minutes lost one session at a time), then N1 and N2's lander
cases, which stopped every landing at once. N2, N3 and N12 together are three quarters of the minutes and share one
cause: a program exists only inside a dev shell that the caller is not in. That is why D1 (one entry) and D8 (the
gate enters the shell itself) come first among the guards.

## Classes and their guards

One rule runs through every guard below: a footgun is closed where its wrong assumption is first observable, by the
program that holds the fact, and its message names the fix. Most classes here share one root. North has five places
that enter a dev shell on their own (`bin/land-shell --dev`, the offload's `RUN` script in
`crates/north-runtime/src/land_offload/ship.rs:61`, `capacity::dev_shell::enter`, three `exec nix develop` check
scripts), each with its own idea of "already inside", none with `--offline`, and one GC
root among them. So the first guard is architecture: one entry, `north dev-shell run`, that every other caller goes
through (box D1). Several classes below close by moving their callers onto it.

### N1. Nested dev shells and sandboxes

What agents assume: wrapping a command in `land-shell --dev`, `depbox run` or `nix develop -c` is harmless when it is
already wrapped. What happens: depbox `--clearenv` drops `IN_NIX_SHELL` and `NORTH_DEV_SHELL`, so every layer enters the
shell again. The offload enters `nix develop "$slot"`, then the land check's `land-shell --dev` runs depbox and a second
`nix develop`, then `check-owners.sh` runs a third. Each layer runs the shellHook, and inside depbox the hook's
`north target-seed` finds no `north` because depbox hides `~/.local`, so the seed is silently skipped and the build
starts cold. Two sandboxes nested inside each other exit 127 (n360-default-checks, 2026-11-05), and a namespace nested
under `run-bounded` broke `/proc` and ssh's ownership check (north#124, north#172).

Guard, earliest point: the entry itself. `north dev-shell run` marks the environment with the shell's identity (the dev
shell derivation path, not a file hash) and runs the command directly when the mark matches; depbox passes the mark
through its `--setenv` list. depbox refuses to start inside a depbox, naming the outer one and saying to run the
command directly. A lint over `harness/scripts`, `bin`, `harness/lib` and the `land.check` lines of `.safe-push`
refuses a literal `nix develop`, `nix-shell` or `land-shell --dev` inside a script that a land check already wraps, with
"use `north dev-shell run --` (it is a no-op inside the shell)" (boxes D1, D2, D3).

### N2. PATH, shims and the real program behind the capacity gate

What agents assume: the first `cargo`, `bun` or `nix` on PATH is the real one, or a missing one fails loudly. What
happens: on gex45 `~/.local/bin/cargo` is the gate shim and the only real cargo lives inside project dev shells, so a
bare `cargo` outside the shell prints `cargo: command not found behind the north capacity gate` and exits 127. The
gate's real-program lookup has failed both ways: it took a stub that says "capacity-gate" for the real bun, because
`is_shim` (`crates/north-core/src/capacity/gate.rs:105`) greps for "capacity gate" with a space, and exec'd it in a
loop that never printed (34 minutes, 2026-11-05); the fix for that (63d9e7783) skipped every program already exec'd in
the process, so a nested `nix develop` found no nix (nix-gate-127, every candidate from 3:42 AM). Backgrounded gated
runs printed the not-found line and still reported exit 0 until 5c707090e.

Guards: the gate knows the project and the shell, so when no real program exists on PATH and the working directory's
flake has a dev shell, it runs the command through `north dev-shell run` instead of exiting 127 (box D4); this turns
the largest single cost in the evidence into a working command. Shim identity becomes one constant the shim generator
and `is_shim` share, checked against the canonical path of the `capacity-gate-shims` derivation, never prose in a
header (box D5). The loop guard keys on program and arguments (nix-gate-127's lane, box D0). A test matrix pins exit
127, never 0, for a missing program through every entry (box D7).

### N3. Leased or offloaded runs that lose or never enter the dev shell (north#465, north#546)

What agents assume: a command run through `north capacity host run`, `capacity exec --dev-shell`, a lease wrapper or an
offload slot sees the same programs and variables as the caller's dev shell. What happens: each entry built its own
environment. Offloads ran bare until 602d51c9b, leases until a7ab53869 and 5c707090e, the offload did not export the
landing variables until train 6c failed on them, and `capacity exec --dev-shell DIR` only sets a marker and execs from
the current PATH, so cargo still resolves to the shim (north#546, open).

Guard: every leased, offloaded and land-check run enters through `north dev-shell run`, and the variables a run carries
across a host or sandbox boundary are one list in one module that depbox, the offload `RUN` script and `capacity host
run` all read (box D1 for `--dev-shell`, box D6 for the list). A test matrix runs the same probe (`command -v cargo`,
`printenv SAFE_PUSH_LAND_BASE`, a missing program) through every entry and asserts identical output (box D7).

### N4. Nix forms that skip the capacity lease (north#387)

What agents assume: the gate leases every Nix build. What happens: the heavy-command hook
(`crates/north-guard/src/guard/heavy_command.rs:134`) and the gate (`gate.rs:126`) each parse only `nix develop … -c X`
with `develop` as the first word. `nix -L develop`, `nix --offline develop`, `nix --extra-experimental-features …
develop`, `nix shell P -c`, `nix run`, `nix build`, `nix flake check`, `nix-build` and `nix-shell --run` run unleased;
inside an interactive dev shell the shell's own cargo comes before the shim, so an ssh `cargo build` took 10 cores in
an uncappable login scope (north#383). A leased `nix build` still builds in nix-daemon's cgroup, which the lease's
quota does not bound.

Guard: one Nix command parser, shared by the hook and the gate and driven by one fixture table, that strips global
flags and classifies every form above by its inner command or as a `nix-build` class (box D8). For daemon builds, the
gate passes `--cores` and `--max-jobs` from the lease's size, because the daemon, not the lease scope, runs the
builder. The dev shell's shellHook puts the shim directory first on PATH, so an interactive shell's cargo is gated too
(box D8).

### N5. Offline, eval-cache and substituter assumptions

What agents assume: a shell that worked yesterday enters offline today, and `--offline` or `--no-build` needs nothing
fetched. What happens: depbox `run` cuts the network, so a dev shell whose closure was collected cannot be rebuilt
(`unable to download …nixpkgs/archive/…tar.gz`, `Cannot build …nix-shell-env.drv`); a Cargo.lock change needs crates
nobody fetched; `firn check`'s `--no-build` still realises stylix's base16 (firn#23). Six offload slots share one
eval-cache SQLite file and print `error (ignored): SQLite database … is busy`, which is noise but made error greps lie.

Guard: `north dev-shell run` checks for the shell's GC-root profile before entering offline and, when it is missing,
realises it once with the network under a lease, then enters with `--offline --no-update-lock-file`; it never fails
with a download error inside a sandbox that has no network (box D1). depbox `run` compares Cargo.lock with the lock it
last fetched for and, when they differ, names `depbox fetch -- cargo fetch --locked` instead of letting cargo try the
network (box D3). The eval-cache contention stays: it is ignored by Nix and costs nothing; the lander's failure summary
drops lines Nix itself marks `(ignored)` (box D7).

### N6. `nix develop` against `nix shell`, `nix run`, `nix-shell -p` and direnv

What agents assume: any of these gives "the toolchain". What happens: `nix shell nixpkgs#cargo nixpkgs#rustc` or
`nix-shell -p pkg-config` uses unpinned nixpkgs packages, not the project's pinned toolchain; workers on wisp#129 used
`nix shell nixpkgs#pkg-config` and a hand-set `PKG_CONFIG_PATH` because north's dev shell lacked the library, and wisp's
`mutants.sh` builds with an ad-hoc toolchain. Asking a flake for an output it lacks (`formatter`, a south dev shell)
fails at eval. North uses no direnv; nix-direnv is enabled host-wide but no failure came from it.

Guard: a PreToolUse rule in `north hook` denies `nix shell nixpkgs#…`, `nix run nixpkgs#…` and `nix-shell -p …` when
the working directory's flake has a dev shell, with "this project pins its tools in flake.nix's dev shell; add PKG
there and run through `north dev-shell run --`", and the same rule runs as a lint over scripts (box D9).
`north dev-shell run` in a flake without a dev shell says so and runs the command bare, instead of an eval error (box
D1).

### N7. Project flakes against the system closure (firn)

What agents assume: what a project's dev shell or the landed repository has, the deployed system has, and the other way
round. What happens: a deployed unit ran a `north` flag or `bin/` entry the runtime no longer has, or did not have yet
(`north land serve --all` crash-looped the lander and orphaned a slot for 27 minutes; `lane-gc.service` failed 13
times); `agent-config-check` found no shellcheck outside the north flake and linted zero hooks for days without failing
(north#351); the laptop agent PATH has no cargo or cc (firn#38); runtime libraries live only on the dev shell's
`LD_LIBRARY_PATH` (wisp#130); firn took a south checkout for the system flake (firn#32) and a service kept a stale
south path (firn#31, open).

Guards: a check that needs a tool or finds zero inputs exits non-zero naming which, never passes empty (box D10); a
north landing that removes or renames a subcommand or flag refuses while a deployed unit in nixos-config still invokes
it, extending b7290d292 from `bin/` entries to `north` arguments, and firn's check refuses a unit that invokes a flag
the installed runtime's `--help` lacks (box D11). Which tools belong in the system closure at all is a judgment, so it
stays prose (see below).

### N8. Store paths and host impurity in tests and goldens

What agents assume: NixOS paths and the checkout path are constants. What happens: fixtures that named
`/run/current-system`, `/nix/var` or a Nix-store cargo turned main red on Ubuntu CI three times (d95c1b3dc, d2f972052,
north#272); tests with `env -i PATH=<minimal>` lost git and grep, which NixOS keeps in `/run/current-system/sw/bin`.
The shellHook put the checkout path into `RUSTFLAGS`, so every per-commit runtime lane and every land check recompiled
every registry crate (472d13847: 10 to 25 minutes per promotion; ad3b7ad28: 47 to 333 seconds per landing).

Guard: a lint over `tests/`, `crates/*/tests` and `*.test.sh` refuses a literal `/nix/store/<hash>`,
`/run/current-system`, `/nix/var` or `~/.nix-profile` outside recorded-log fixtures on an allowlist, and tests get one
helper that builds a minimal PATH from `command -v` at start (box D12). `tests/it/dev_shell_target.rs` already pins the
target keying; it gains the case that two checkouts at different paths share `RUSTFLAGS` for their own `target` (box
D12).

### N9. GC roots, pins and flake.lock drift

What agents assume: what is in the store stays there, and a lock node named nixpkgs is the flake's nixpkgs. What
happens: `nix-store --gc` collected the glibc loader of unrooted runtimes and every `north` call on gex45 exited 127
(north#227); `nix-collect-garbage` took depbox's dev shell and offline builds could not rebuild it; GC took StormLib
under wisp's map-pack (wisp#7) and the store path nixpkgs' rustup bakes into its `ld.lld` wrapper (firn#47, open, four
relay tests red); a collected `base16-schemes` derivation refused a nixos-config landing. depbox keys its dev-shell root
on a hash of tracked `*.nix`, `flake.lock` and `rust-toolchain*` files, but the shell also embeds `bin/cargo`, so an
edit there keeps the stale root. A transitive nixpkgs node gave CI a different statix (firn#23).

Guards: a PreToolUse rule denies `nix-collect-garbage`, `nix-store --gc`, `nix store gc` and `nix-env
--delete-generations` on agent hosts, naming `north nix gc`, which first roots every promoted runtime, every live
worktree's dev shell and every rustup toolchain's store references, then collects (box D13). `north doctor nix` reports
each live worktree's dev shell as rooted or not, enters each offline, checks every store path a rustup toolchain wrapper
names, and flags a flake.lock with more than one nixpkgs node (box D14). depbox's root moves into `north dev-shell run`
and keys on the dev shell's derivation path (box D1).

### N10. shellHook side effects

What agents assume: a dev shell's hook is a pure function of the flake. What happens: north's hook runs
`north target-seed || true`, which fails silently wherever `north` is hidden (every depbox, 30+ sessions of cold
builds), and computes `RUSTFLAGS` from the checkout path (N8's cost).

Guard: `target-seed` moves out of the hook into `north dev-shell run`, which runs outside the sandbox and fails loudly;
a lint over `flake.nix` refuses `|| true` and any program in a shellHook that the shell's own packages do not provide
(box D2).

### N11. Sandbox visibility

What agents assume: a script in `/tmp` or `~/.cache` is visible inside depbox. What happens: depbox mounts tmpfs on
`/tmp` and `$HOME`, so the script exits 127 as "No such file".

Guard: depbox resolves its command's path before `exec` and refuses one outside the checkout and its read-only binds,
naming the path and the binds (box D3).

### N12. PATH differs by context

What agents assume: the PATH of an ssh login, a fleet service, a systemd timer, a hook and a skill example are the
same. What happens: `machine-capacity` and `node`, which skills and messages tell agents to run, are not installed on
gex45 (about 75 sessions); the `code-health-tick` unit had no Nix profile on PATH and its bun shim exited 127 hourly; a
fleet service lacked `~/.local/bin`; a home-manager switch mid-run swapped `~/.nix-profile` under a land check
(north#178, fixed by pinning PATH in the land-check runner, now `north land-tests --select`).

Guard: a lint in `north config agents` resolves every program that a skill, a hook message or a deny message tells an
agent to run against the installed runtime's `bin/` and the host's system closure, and refuses one that resolves
nowhere (box D15). Fleet service units take PATH from the same generated list as the login profile, which nixos-config
tests (box D15).

## Context by activity class

Guards stop known wrong forms; most Nix mistakes here were not a wrong form but a missing fact (you are already in the
dev shell; only the gate's PATH applies; this host has no cargo outside the flake). The lead asked for those facts at
the moment of the activity, as well as the guards (2026-11-05). Three parts, one source.

- Cards are data, one file: `harness/nix/cards.toml`, one entry per class (N1 to N12) with its matchers (Bash command
  forms, Edit or Write paths, and the environment marks `NORTH_DEV_SHELL`, `SAFE_PUSH_LANDING`, an offload slot path or
  a lease cgroup), at most 10 lines of north facts the agent cannot know, the incident class it prevents, and the deny
  forms where the class has a deterministic wrong form. Examples: N1 "inside an offload slot or land check you are
  already in the dev shell; never nest `nix develop`, `land-shell --dev` or depbox; run the tool directly"; N2 "behind
  the capacity gate only the gate's PATH applies: `cargo` here is the gate shim, and the real one exists only inside
  the project's dev shell"; N3 "a capacity run enters the dev shell only through `north dev-shell run` (`--dev-shell`
  until D1)".
- The hook: a `nix_activity` step in `north hook` (PreToolUse) matches Bash commands that name `nix develop`,
  `nix-shell`, `nix run`, `nix build`, `nix shell`, `nix-collect-garbage`, `nix-store`, `direnv`, `land-shell` or
  `depbox`, and Edit or Write on `*.nix`, `flake.lock`, `.envrc` or a `harness/scripts/*.sh` or `bin/*` file that calls
  nix. It injects the matched card as additional context once per session per class (the same once-per-session
  pattern firn-system-policy uses for its DIGEST), because a card repeated on every call costs context and teaches
  nothing new. It denies only a card's deny forms, with the card as the reason.
- The lookup: `north nix explain <nix subcommand | class>` prints the card, then the matching section of the locally
  installed Nix manual, read at run time from `nix <cmd> --help` (which renders `man nix3-<cmd>`), cut to the options
  the card names. Nothing from the Nix manual is vendored into the repository, so its license never applies to north.
  The hook and agents call the same function, and every card ends with "more: `north nix explain CLASS`".

Each card names its incident class as written in the incidents ledger (`nix-nested-shell`, `nix-gate-path`, and so on;
the table is queued in ebb86c4c2, north#112), and the card's landing is recorded as that class's guard
(`incident_guards`), so a later row of the class reads as a recurrence after the guard. Each card also gets an
`experiments.toml` entry (the registry is in the experiments-registry lane): metric, the class's incident rows per 7
days; predicted direction, down; review date, 14 days after landing. `north experiments judge` then keeps the card, or
files its removal when the class recurs at the same rate, so a card that changes nothing is pruned instead of
accumulating.

## Staying current with Nix

Cards and skill text are written against one Nix (2.34.9 on gex45 on 2026-11-05) and one nixpkgs (north's lock,
nixos-unstable of 2026-08-31). The lead asked that the machinery follow current Nix patterns and notice by itself when it
falls behind (2026-11-05). The nix-current-practice session's gap list against upstream decides which current
patterns the cards teach; its outdated or risky items are migrated in the boxes under "Current practice" below.

No new checker: staleness rides on the two that exist.

- Stamp (this plan). `harness/nix/cards.toml`, `harness/skills/nix-development/SKILL.md` and the firn skill carry
  `nix-version: 2.34` (major.minor) and `nixpkgs-lock: 2026-08-31` in their frontmatter beside `grounded:`.
- Followed sources (this plan). South's `agents/watch.toml` gains two `[[source]]` entries that the daily watch-review
  already reviews when due: `nix-release-notes` (repo `https://github.com/NixOS/nix`, path
  `doc/manual/source/release-notes`, LGPL-2.1, read and summarised, never copied) and `nixos-release-notes` (repo
  `https://github.com/NixOS/nixpkgs`, path `nixos/doc/manual/release-notes`, MIT). A watch cycle that finds a change
  touching a card's subcommands records it in `docs/rationale/watch/` like any other source.
- Trigger (a box on north#112, not this plan). `north skill-review-queue`
  (`crates/north-runtime/src/timers/skill_review.rs`, weekly on nexus) already flags a skill more than 30 days past
  `grounded:`; it also flags a stamped file when `nix --version` on a host moves major.minor past `nix-version:`, when
  the flake.lock it names moves past `nixpkgs-lock:`, or when watch-review records a change to a Nix source after
  `grounded:`, and files the regrounding box the queue already files.

## Current practice

From the nix-current-practice session's gap list against upstream of 2026-11-04
(`docs/rationale/nix-current-practice.md`, landing from its own lane). Upstream Nix is 2.35.4 and every host runs
2.34.9 on NixOS 26.05, a supported pair; flakes and `nix-command` are still experimental with no stabilisation date; `nix fmt` runs the flake's
`formatter` output; `nix-shell`, `nix-env` and channels are not deprecated. The cards therefore teach flakes with
`nix develop -c`, `--offline --no-update-lock-file` for agent runs, `nix fmt` once a formatter exists, and no channels.
What north already does right stays: dev shells entered with `nix develop -c`, depbox's `print-dev-env --profile` root
with `--no-update-lock-file`, `--no-write-lock-file` in agent-config-check, pure eval, `accept-flake-config = false`,
`require-sigs`, and no first-party `nix-shell`, `nix-env` or `nix-channel` call.

Marked risky or outdated, each with its box:

- Risky: firn's `trusted-users` includes `@wheel` (`modules/nix-settings/default.nix:41`), which is root-equivalent;
  only the laptop's user is in wheel. D22.
- Risky: firn's CI pins `nix_path nixpkgs=channel:nixos-25.05`, end of life while hosts run 26.05, and
  `install-nix-action` v27 (`.github/workflows/build.yml:13,15`, `tests.yml:33`). D23.
- Risky: makgora's shellHook exits 1 when the mdl addon is missing (`flake.nix:13`), so every `nix develop -c` fails
  on a host without that store. D24, and D7's shellHook lint runs in every repository.
- Risky: firn's `elephant` input does not follow nixpkgs (`flake.nix:32`) and drags an unfollowed nixos-unstable of
  2025-10-12 into the lock. D25, and D14's lock check refuses it.
- Risky: depbox prunes dev-shell roots idle 7 days (`bin/depbox:46`) under a 30-day GC, so an idle checkout's shell can
  be collected and its next offline build fails. D6 keys a root's life on its worktree existing, not on idle days.
- Outdated: north's and wisp window's nixpkgs is nixos-unstable locked 2026-08-31, a second nixpkgs beside the hosts'
  26.05 that shares no store paths with them; makgora pins nixpkgs by a raw rev in the URL, invisible to
  `nix flake update` and the dep gate. D25.
- Outdated: no repository declares a `formatter`, so `nix fmt` does nothing and firn checks nixfmt by hand. D26.
- Outdated: firn never sets `nix.channel.enable = false`, so gex45's `NIX_PATH` still ends in root's channels. D27.
- Outdated: the shellHooks of north and wisp window run `north target-seed || true`. D7, extended to wisp.
- Outdated: firn trusts `devenv.cachix.org` with no devenv consumer (`modules/nix-settings/default.nix:24`). D22.
- Outdated: north's dev shell ships python3 for `bin/threads.test.sh` and `bin/agent-instruction-check`; it leaves when
  those port to Rust, which the non-Rust inventory owns ([non-rust-inventory](non-rust-inventory.md)), not this plan.

## Existing guards and their gaps

- Hook wiring (`nixos-config/main/modules/north-profile/claude-hooks.json`): Bash runs firn-system-policy, then
  `north hook`. Gap: no Nix-specific step beyond the two below, and no context injection for Nix work.
- firn-system-policy (`nixos-config/main/tools/firn/src/system_policy.rs:233`): denies raw `nixos-rebuild`,
  `darwin-rebuild` and `nh` switch or boot, naming `firn rebuild`. Gaps: matches only at a word start after a shell
  separator, so `/run/current-system/sw/bin/nixos-rebuild switch`, `env … nixos-rebuild`, `bash -c '…'`,
  `sudo -E nixos-rebuild` and `nix run nixpkgs#nixos-rebuild -- switch` pass; nothing covers `switch-to-configuration`,
  `nix-env -i`, `nix profile install` or `nix-channel`. It fails open on a missing binary or oversized payload. Its
  matcher would move onto the shared Nix parser in D8.
- heavy-command guard (`crates/north-guard/src/guard/heavy_command.rs:16`, retired as a script, now in `north hook`):
  routes or refuses heavy cargo, tsc and bun runs; for Nix it unwraps only `nix develop … -c X` with `develop` as the
  first word (`:134`). Gaps: every other Nix form in N4, any global flag before `develop`, and no fixture row for Nix
  at all (`fixtures/heavy_command.json`).
- The capacity gate (`crates/north-core/src/capacity/gate.rs:28`) and its shims (`bin/cargo`, installed as cargo, bun
  and nix in `~/.local/bin`, and as cargo and bun in the dev shell's `capacity-gate-shims`): leases classified
  commands and execs the real program. Gaps: the same Nix blind spots as the hook; `is_shim` matches header prose with
  a space, which a hyphenated stub defeated; the per-process loop guard from 63d9e7783 also skips a legitimate nested
  nix; the shell fallback skips only itself, so two shim copies ahead of the real program could exec each other when
  north is unreachable (by reading, not reproduced); with no real program it exits 127 even where the project's dev
  shell has one; a leased `nix build` runs in nix-daemon's cgroup outside the lease.
- `capacity::dev_shell` (`crates/north-core/src/capacity/dev_shell.rs:52`): wraps leased runs in the project dev shell
  unless the first word is `nix`. Gaps: a stray `IN_NIX_SHELL` from another project or `nix-shell -p` counts as inside;
  `env X=1 nix …`, `depbox run -- nix develop`, `land-shell --dev` and `bash -c 'nix develop …'` get wrapped again; no
  `--offline`; `--dev-shell DIR` alone enters nothing (north#546); `project_shell` accepts any flake whose text
  contains "devShell", comments included.
- land-shell (`bin/land-shell`) and depbox (`bin/depbox`): run land checks offline in a bwrap sandbox, `--dev` inside
  `nix develop`; depbox asks the gate for a lease (be775aee0) and roots the dev shell under
  `~/.local/state/depbox/shell-roots` (ecd62e9e2). Gaps: no nesting check; `--clearenv` drops the shell mark, so
  nesting re-enters; `nix develop` without `--offline` inside a no-network sandbox; the root key misses `bin/cargo`,
  which the shell embeds; `~/.local` is hidden, so the shellHook's `north` is missing; scripts under `/tmp` vanish.
- The offload `RUN` script (`ship.rs:61`): enters `nix develop "$slot"` unless the command starts with the literal
  text `nix develop`. Gaps: every `.safe-push` check starts with `CARGO_TARGET_DIR=` or `bash`, so each enters the
  shell here and again in land-shell; no `--offline`, no root, nix from PATH only.
- `crates/north-landing/src/landing/land_test_selection.rs` (`pinned_path`): pins PATH directories to their store targets, which closed the mid-run profile
  switch (north#178). No gap found.
- Runtime GC roots (90618140a, 2d43dfc83) and the deployed-unit check (b7290d292): root every store path a promoted
  runtime loads, and refuse a landing that deletes a `bin/` entry a deployed unit runs. Gaps: dev shells of live
  worktrees and rustup toolchains are unrooted; the unit check knows `bin/` names, not `north` subcommands or flags.
- dep-gate (`crates/north-landing/src/dep_gate.rs:744`): age-gates changed flake.lock nodes. Gap: nothing flags a
  second nixpkgs node.
- `tests/it/dev_shell_target.rs`: pins target keying only.
- There is no `north doctor`, and no Nix health probe anywhere.

## Text that stays in skills

Only what no program can check, each with why:

- firn: "The system closure carries no project toolchain; a tool one project needs goes in that project's flake, and a
  tool a system unit runs goes in the unit's own closure." Whether a tool is a project tool or a system tool is a
  judgment about who consumes it (shellcheck is both), so no lint can decide it; D10 and D15 catch the consequences.
- nix-development: "When a dev shell lacks a library or tool, add it to the flake's dev shell in the same lane, and say
  so in the report." The hook can deny `nix shell nixpkgs#…` (D9), but whether the missing piece belongs in this flake
  or another project's is a judgment.

Everything else this file names moves into a hook, a lint, a runtime check or a probe, and the existing prose that
repeats it (machine-capacity's "pass the bare command, not `nix develop -c …`") is cut when its guard lands.

## Boxes

In landing order. Each is at most 45 minutes, runs in its own lane, and its first Done when line replays a recorded
failure. A replay "at f1745de2e" runs against the main of 2026-11-05 03:17 AM, before any fix here.

### D0. The gate's loop guard keys on program and arguments (owned by nix-gate-127, north#360)

Already in flight; listed so the order below holds. Its lane keys `NORTH_CAPACITY_GATE_EXECUTED` on the process and a
hash of program and arguments, so the looping stub is still caught and a nested `nix develop` reaches the real nix,
with `a_check_script_that_runs_nix_develop_inside_a_dev_shell_reaches_the_real_nix_behind_the_gate`. D1 stops the same
replay without relying on D0, by removing the nesting.

### D1. One dev-shell entry: `north dev-shell run` (N1, N3)

Category: tooling. Scope: cross-module. Surface: rust. Verify: local-test. Files: a `dev_shell` module beside
`crates/north-core/src/capacity/dev_shell.rs` (which it absorbs), `src/cli/`, `ops/land_offload/ship.rs` (`RUN`),
`harness/scripts/check-owners.sh`, `citation-check.sh`, `experiments-check.sh` if landed.
`north dev-shell run [--dir D] -- CMD` finds the flake's `devShells.<system>.default`, takes its derivation path as the
shell's identity, runs CMD directly when `NORTH_DEV_SHELL` already holds that identity, and otherwise runs
`nix develop D --offline --no-update-lock-file -c env NORTH_DEV_SHELL=<id> CMD`, with nix found on PATH or at the
system path. In a flake without a dev shell it says so on stderr and runs CMD bare. The offload's `RUN` and the three
check scripts call it instead of `nix develop`.

Done when:
- Replay nix-gate-127: in a gex45 offload slot, `bash harness/scripts/check-owners.sh` behind the capacity gate exits
  127 with `nix: command not found behind the north capacity gate` at f1745de2e and exits 0 at the lane head, without
  D0.
- `north dev-shell run -- north dev-shell run -- sh -c 'echo $NORTH_DEV_SHELL'` prints one derivation path, and
  `NORTH_DEV_SHELL_TRACE=1` shows one entry and one direct run.
- A tests/it case runs a check script that calls `north dev-shell run` inside an entered shell behind the gate and
  asserts it runs once, with the real cargo.

### D2. Leases enter through `north dev-shell run` (N3, north#546)

Category: bug. Scope: single-module. Surface: rust. Verify: local-test. Files: `crates/north-core/src/capacity/host/run.rs`,
`capacity/dev_shell.rs` callers, `capacity exec`.
`capacity exec --dev-shell DIR` and `dev_shell::enter` become calls to D1's entry; a stray `IN_NIX_SHELL` no longer
counts as inside, and a flake counts as a project shell only when it has the `devShells` output, not when its text
contains the word.

Done when:
- Replay north#546: on gex45, `north capacity exec --dev-shell ~/code/north/main -- cargo --version` from a plain ssh
  shell prints a `cargo 1.` line (at f1745de2e: `cargo: command not found behind the north capacity gate`, exit 127).
- With `IN_NIX_SHELL=impure` exported from `nix-shell -p hello`, `north capacity host run -- cargo --version` in a north
  worktree still prints the pinned toolchain's version.

### D3. Nix activity cards in `north hook` (all classes)

Category: tooling. Scope: cross-module. Surface: rust. Verify: local-test. Files: `harness/nix/cards.toml`, a
`guard/nix_activity.rs` step in `crates/north-guard/src/guard/`, `hook_rules.rs`, a fixture of recorded commands.
The cards, matchers and deny forms of "Context by activity class" above, one card per class, at most 10 lines each.
Deny forms in this box: a land-check runner wrapped in `land-shell` or depbox (`land-shell --dev -- … north land
checks`, `depbox run -- … safe-push`), and `nix-collect-garbage`, `nix-store --gc`, `nix store gc` on gex45 and the
laptop (collections stay frozen until D13). Every other match injects context once per session per class.

Done when:
- Replay n360-default-checks: the hook on `./bin/land-shell --dev -- target/debug/north land checks --base origin/main
  --head HEAD` returns deny with the N1 card as its reason (at f1745de2e it passes, and every check exits 127).
- A Bash call `nix develop -c cargo test` in a north worktree receives the N1 and N2 cards as additional context once,
  and a second such call in the same session receives none.
- A fixture of 20 recorded commands from this file's evidence matches each to the class listed here.

### D4. `north nix explain` (all classes)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: `src/cli/`, the D3 card module.
`north nix explain <nix subcommand | class>` prints the card, then the sections of `nix <cmd> --help` (run on this
host) that hold the options the card names. Nothing from the Nix manual is committed. D3's cards end with the lookup.

Done when:
- Replay 4befdb3a (an offline `nix develop` that tried to download nixpkgs): `north nix explain develop` prints the N5
  card and the `--offline` and `--no-update-lock-file` paragraphs from this host's `nix develop --help`.
- `rg -n 'nix3-|Synopsis' harness crates` finds no copied manual text.

### D5. depbox is idempotent and shows what it hides (N1, N5, N10, N11)

Category: bug. Scope: single-module. Surface: shell. Verify: local-test. Files: `bin/depbox`, `bin/land-shell`,
`bin/safe-push.test.sh` or a depbox test.
`depbox run` inside a `run` depbox runs the command directly; `fetch` inside `run` refuses, naming the outer sandbox.
It binds the installed runtime tree read-only, so `north` resolves inside, passes `NORTH_DEV_SHELL`, refuses a
command path outside the checkout and its binds with the path and the bind list, and names
`depbox fetch -- cargo fetch --locked` when Cargo.lock differs from the one it last fetched for.

Done when:
- Replay n360-default-checks: `./bin/land-shell --dev -- ./bin/land-shell --dev -- true` exits 0 with one sandbox (at
  f1745de2e it exits 127).
- Replay the shellHook miss: `./bin/land-shell --dev -- true 2>&1 | rg 'north: command not found'` prints nothing.
- Replay f1749039: `depbox run -- /tmp/x.sh` exits 2 naming `/tmp/x.sh` as hidden.
- Replay 1d5635a3: after a Cargo.lock edit, `depbox run -- cargo metadata --offline` names the fetch command.

### D6. Offline entry with its own GC root (N5, N9)

Category: bug. Scope: cross-module. Surface: rust. Verify: local-test. Files: D1's module, `bin/land-shell`, `bin/depbox`
(drops `root_shell`).
D1's entry roots the shell's profile under `~/.local/state/north/dev-shells/<id>` and, when the root is missing,
realises it once with the network under a lease before entering offline. `land-shell --dev` runs
`north dev-shell run --prepare` outside the sandbox, then enters offline inside it. The root keys on the derivation path,
so an edit to `bin/cargo` gets a new root. A root lives as long as a worktree whose flake produces it exists, not for a
fixed number of idle days, so the 30-day GC cannot collect an idle checkout's shell.

Done when:
- Replay 6430b666: with the shell's root removed and its env path deleted (`nix-store --delete` on that one path),
  `./bin/land-shell --dev -- true` exits 0 (at f1745de2e: `unable to download` or `Cannot build …nix-shell-env.drv`).
- After a one-line edit to `bin/cargo` in a scratch lane, `ls ~/.local/state/north/dev-shells` shows a new root.

### D7. The shellHook is pure (N10)

Category: tooling. Scope: cross-repo. Surface: nix. Verify: local-test. Files: `flake.nix` in north and wisp `window/`,
D1's module, a lint in `north land scan` that runs over every repository's flake.
`north target-seed` moves from the shellHook into D1's entry, which runs it outside the sandbox and reports a failure
instead of `|| true`. A lint refuses `|| true` in a `flake.nix` shellHook, any program there that the shell's own
packages do not provide, and `exit`, because a hook that exits fails every `nix develop -c` (makgora, D24).

Done when:
- Replay 045f70ad (a cold compile after a silent seed miss): in a fresh lane, `./bin/land-shell --dev -- cargo build
  --locked --offline` compiles no registry crate, read from `--message-format json`.
- The lint refuses f1745de2e's `flake.nix` naming `north target-seed || true`.

### D8. With no real program, the gate enters the project dev shell (N2)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: `crates/north-core/src/capacity/gate.rs`,
`gate_tests.rs`.
When `real_program` finds nothing and the working directory's flake has a dev shell the process is not in, the gate
runs the command through D1's entry instead of failing;
with no dev shell it still exits 75 (infrastructure, as 52bb6f1b set for the shims and gate) with the name.

Done when:
- Replay d63af5ec (bare `cargo` on gex45): `cargo --version` from a plain ssh shell in `~/code/north/main` prints a
  `cargo 1.` line (at f1745de2e: `cargo: command not found behind the north capacity gate`).
- The same in `~/code/wisp/main` prints wisp's pinned version; in `/tmp` it still exits 75 naming `cargo`.

### D9. One shim identity (N2)

Category: bug. Scope: single-module. Surface: rust. Verify: local-test. Files: `gate.rs` (`is_shim`), `bin/cargo`,
`flake.nix` (the `capacity-gate-shims` derivation).
`is_shim` matches one constant marker line that the shim script carries and the gate defines, or a canonical path
inside a `capacity-gate-shims` output, never free prose.

Done when:
- Replay the 2026-11-05 34-minute hang: a tests/it case puts a stub whose header says "capacity-gate" ahead of the real
  bun and asserts the gate execs the real bun without the loop guard firing (at f1745de2e the gate picks the stub).

### D10. One list of what crosses a boundary, and an entry matrix (N2, N3)

Category: tooling. Scope: cross-module. Surface: rust. Verify: local-test. Files: a `capacity::carry` constant read by
`ship.rs`, `capacity/host/run.rs` and `bin/depbox` (through `north capacity carry --list`), a tests/it matrix.
The variables a run carries into a lease, an offload or a sandbox are one list. The matrix runs one probe
(`command -v cargo`, `printenv SAFE_PUSH_LAND_BASE`, a missing program's exit code) through the gate shim,
`capacity host run`, `capacity exec --dev-shell`, offload run, `land-shell --dev` and `depbox run`. The lander's
failure summary drops lines Nix marks `(ignored)`.

Done when:
- Replay train 6c's `usage: north land-fails-first`: an offload run of `printenv SAFE_PUSH_LAND_BASE` during a landing
  prints the base sha.
- Replay north#465's false green: the matrix asserts exit 127, never 0, for a missing program through every entry.
- Every entry resolves `cargo` to the same toolchain path.

### D11. One Nix command parser for the hook and the gate (N4, north#387)

Category: bug. Scope: cross-module. Surface: rust. Verify: local-test. Files: a `nix_command` module shared by
`guard/heavy_command.rs` and `capacity/gate.rs`, `fixtures/heavy_command.json`.
It strips global flags (`-L`, `--offline`, `--extra-experimental-features X`, `--option K V`), unwraps `env`,
`bash -c` and `sh -c`, and classifies `develop`, `shell`, `run`, `build`, `flake check`, `nix-build` and
`nix-shell --run|--command` by their inner command or as a Nix build.

Done when:
- Replay north#383: from a Bash call on gex45, `nix -L develop -c cargo build -p north-core` runs in an
  `agent-capacity-*` scope (`systemctl --user status` during the run).
- The fixture holds every form above with its class, and the hook and the gate read the same rows.

### D12. Nix builds and interactive shells stay inside the lease (N4)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: `gate.rs`, `flake.nix` shellHook.
A leased Nix build passes `--cores` and `--max-jobs` sized from its lease, because the daemon builds outside the lease
scope. The shellHook puts the shim directory first on PATH.

Done when:
- Replay north#383's interactive case: `nix develop` over ssh, then `cargo build`, runs in an `agent-capacity-*` scope.
- A leased `nix build .#shellcheck --rebuild` shows the lease's core count as `--cores` in the daemon's builder
  environment (`NIX_BUILD_CORES`).

### D13. `north nix gc` (N9)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: `src/cli/`, the runtime-root module
from 90618140a, the D3 card for N9.
Before collecting, it roots every promoted runtime, every live worktree's dev shell and every store path a rustup
toolchain wrapper names, prints the roots, then runs the collection with `--max-freed` if given. The N9 deny names it.

Done when:
- Replay 6430b666: the hook denies `run-bounded 20m -- nix-collect-garbage` naming `north nix gc`.
- `north nix gc --dry-run` on gex45 lists the live dev shells and the rustup `ld-wrapper.sh` store path of firn#47 as
  roots.

### D14. `north doctor nix`, and a lock check at landing (N5, N9)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: `src/cli/`, a `.safe-push` land check
on `flake.lock`.
The probe reports each live worktree's dev shell as rooted or not, enters each offline, checks every store path a
rustup toolchain's wrappers name, and lists flake.lock nodes that bring a second nixpkgs. The lock check refuses a
second nixpkgs node unless the lock entry is on an allowlist with its reason.

Done when:
- Replay firn#47: on gex45 the probe names the missing `rustup-1.29.0/nix-support/ld-wrapper.sh` and the toolchain
  that references it.
- Replay firn#23: the lock check refuses that fix's parent lock, naming the transitive nixpkgs node.

### D15. Checks fail closed (N7)

Category: bug. Scope: single-module. Surface: rust. Verify: local-test. Files: `agent-config-check` and every check
under `harness/scripts` that runs an external tool.
A check whose tool is missing, or that finds zero inputs where its config names some, exits non-zero naming which.

Done when:
- Replay north#351: `agent-config-check` with no shellcheck on PATH exits non-zero naming shellcheck (at its parent
  it reported 0 hooks linted and passed).

### D16. Units cannot outrun the runtime (N7)

Category: tooling. Scope: cross-repo. Surface: rust. Verify: local-test. Files: the b7290d292 land scan in north,
`firn check` in nixos-config.
North's scan extends from deleted `bin/` entries to removed or renamed `north` subcommands and flags that a nixos-config
unit invokes; firn's check refuses a unit that invokes a flag the installed runtime's `--help` lacks.

Done when:
- Replay the 2026-11-05 lander crash loop: firn's check refuses a `north-land.service` with `north land serve --all`
  against a runtime whose `north land serve --help` has no `--all`.
- Replay north#331: north's scan refuses a lane removing `bin/lane-gc` while the deployed unit runs it.

### D17. Every program a skill names resolves (N12)

Category: tooling. Scope: cross-repo. Surface: rust. Verify: local-test. Files: `north config agents` checks,
`harness/skills/*/SKILL.md`, hook deny messages, the nixos-config fleet service PATH.
A lint resolves each program a skill, hook message or deny message tells an agent to run against the runtime's `bin/`
and the system closure of each host class. Fleet service units take PATH from the login profile's list, which a
nixos-config test pins.

Done when:
- Replay 694b2a05 (`machine-capacity: command not found`): the lint refuses f1745de2e's
  `harness/skills/machine-capacity/SKILL.md:18` for gex45, and passes once the program is installed or the text names
  one that is.
- `systemctl --user show -p Environment north-fleet-*.service` on gex45 lists `~/.local/bin` first.

### D18. Tests name no host paths (N8)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: a lint in `north land scan`, a
`test_env` helper for tests, `tests/it/dev_shell_target.rs`.
The lint refuses `/nix/store/<hash>`, `/run/current-system`, `/nix/var` and `~/.nix-profile` in test code outside an
allowlist of recorded-log fixtures. Tests that need a minimal PATH build it from `command -v` through the helper. The
dev-shell test gains the case of two checkouts at different paths sharing `RUSTFLAGS` for their own `target`.

Done when:
- Replay north#272: the lint refuses the fixture at 747d84a50's parent, naming `/run/current-system`.
- Replay 472d13847's cost: the dev-shell test fails on a shellHook that keys an in-checkout target on its path.

### D19. No literal Nix entry or ad-hoc toolchain in scripts (N1, N6)

Category: tooling. Scope: single-module. Surface: rust. Verify: local-test. Files: a lint in `north land scan`, `.safe-push`,
D3's deny forms.
After D1, D2 and D5 have moved every caller, the lint refuses `nix develop`, `nix-shell` and `land-shell --dev` in
`harness/scripts`, `bin`, `harness/lib` and `land.check` lines, except D1's module, and refuses `nix shell nixpkgs#`,
`nix run nixpkgs#` and `nix-shell -p` in a repository whose flake has a dev shell. D3 gains the same deny for Bash.
Landed as `north land scan nix-entry` (2026-11-05). `bin/land-shell` is the shell face of D1's entry (depbox around
`north dev-shell run`, with the literal fallback before any north exists), so it is exempt from the entry forms. `land-shell
--dev` is refused only in `harness/scripts` and `harness/lib`, where a land check already runs inside the shell: the
top-level check lines and `bin/head-north`'s bootstrap keep it for its depbox sandbox and its Actions passthrough, which
`north dev-shell run` lacks.

Done when:
- Replay nix-gate-127's trigger: the lint refuses f1745de2e's `check-owners.sh:9` and `citation-check.sh:8`.
- Replay wisp#129: the hook denies `nix shell nixpkgs#pkg-config -c cargo build` in a wisp worktree, naming the dev
  shell.

### D20. Each card is a measured guard (all classes; after ebb86c4c2 and the experiments-registry lane land)

Category: tooling. Scope: single-module. Surface: docs. Verify: local-test. Files: `harness/nix/cards.toml`,
`docs/rationale/experiments.toml`, the incidents fixtures.
Each card's incident class is a class in the incidents ledger with the card's landing as its guard, and an experiment
entry measures that class's rows per 7 days, judged 14 days after landing.

Done when:
- Replay the 2026-11-05 night: `north incident import` of this file's N1 and N2 rows, then `north incident report`,
  lists them under `nix-nested-shell` and `nix-gate-path` with D1 and D3 as their guards.
- `north experiments check` passes with one entry per card.

### D21. Nix version stamps and watched Nix sources (staying current)

Category: tooling. Scope: cross-repo. Surface: docs. Verify: none. Files: `harness/nix/cards.toml`,
`harness/skills/nix-development/SKILL.md`, the firn skill in nixos-config, south `agents/watch.toml`.
The stamps and the two watch sources of "Staying current with Nix". The skill-review-queue trigger that reads the
stamps is a box on north#112.

Done when:
- `rg -n '^nix-version: 2\.34$' harness/skills/nix-development/SKILL.md harness/nix/cards.toml` and the firn skill
  each print one line, and each file has `nixpkgs-lock: 2026-11-02`.
- `rg -n 'id = "nix-release-notes"|id = "nixos-release-notes"' agents/watch.toml` on south's origin/main prints two
  lines.

### D22. Narrow Nix trust in firn (current practice)

Category: security. Scope: single-module. Surface: nix. Verify: local-test. Files: nixos-config
`modules/nix-settings/default.nix`.
`trusted-users` becomes `root` only and `devenv.cachix.org` leaves the substituters and trusted keys. Before the
change, `rg -n -- '--option (extra-)?substituters|--option trusted|--impure' ~/code/*/main` must show no agent command
that needs a trusted user; the box stops and reports any it finds.

Done when:
- Replay the gap: `nix config show trusted-users` on the laptop prints `root` (at nixos-config's main of 2026-11-05 it
  prints `root @wheel`), and `nix config show substituters` lists no devenv cache.
- `./bin/land-shell --dev -- cargo build --locked --offline` in north still passes on the laptop.

### D23. firn CI on the hosts' Nix and nixpkgs (current practice)

Category: bug. Scope: single-module. Surface: ci. Verify: ci. Files: nixos-config `.github/workflows/build.yml`,
`tests.yml`.
CI drops `nix_path` (the flake pins nixpkgs) and moves `install-nix-action` to its current major.

Done when:
- Replay firn#23's class: `rg -n 'channel:nixos-25.05|install-nix-action@v27' .github/workflows` prints nothing, and
  the next main run's statix and eval steps pass.

### D24. makgora's shellHook never fails the shell (current practice)

Category: bug. Scope: single-module. Surface: nix. Verify: local-test. Files: makgora `flake.nix`, the build step that
needs the mdl addon.
The hook prints that the addon is missing and returns; the step that needs the addon fails with that message.

Done when:
- Replay: on gex45 with the addon store absent, `nix develop ~/code/makgora/main -c true` exits 0 (at makgora's main of
  2026-11-05 it exits 1), and the addon's build step exits non-zero naming the missing addon.

### D25. One nixpkgs per host (current practice)

Category: tooling. Scope: cross-repo. Surface: nix. Verify: local-test. Files: `flake.nix` and `flake.lock` in north,
wisp `window/` and makgora; nixos-config `flake.nix` (`elephant`).
North, wisp window and makgora take nixpkgs from `github:NixOS/nixpkgs/nixos-26.05`, the hosts' branch, so dev shells
share store paths with the system and the dep gate sees the lock; makgora's raw-rev URL becomes a branch with a lock;
firn's `elephant` follows nixpkgs. The rust toolchain stays pinned by rust-overlay and `rust-toolchain.toml`.

Done when:
- Replay firn#23's second node: D14's lock check passes on all four locks, and `jq '[.nodes | keys[] |
  select(startswith("nixpkgs"))] | length' flake.lock` prints 1 in each.
- In north, `./bin/land-shell --dev -- cargo test --locked --offline` passes on gex45 and the laptop.

### D26. `nix fmt` works in every repository (current practice)

Category: tooling. Scope: cross-repo. Surface: nix. Verify: local-test. Files: `flake.nix` in north, wisp, makgora and
nixos-config; nixos-config `docs/verification.md`.
Each flake declares `formatter.x86_64-linux = pkgs.nixfmt-tree`, and firn's separate nixfmt check becomes
`nix fmt -- --ci`.

Done when:
- `nix fmt -- --ci` exits 0 in each of the four checkouts (at their main of 2026-11-05 it reports no formatter), and
  the keyring-hibernate case (`does not provide attribute 'formatter.x86_64-linux'`) cannot recur.

### D27. No channels on the hosts (current practice)

Category: tooling. Scope: single-module. Surface: nix. Verify: local-test. Files: nixos-config host modules.
`nix.channel.enable = false`, and `NIX_PATH` comes from the flake's nixpkgs (`nix.nixPath`) for the few tools that read
`<nixpkgs>`; `rg -n '<nixpkgs>' ~/code/*/main` lists them first.

Done when:
- `ssh gex45 'echo $NIX_PATH'` names no `channels` directory (on 2026-11-05 it ends in
  `/nix/var/nix/profiles/per-user/root/channels`), and `nix-instantiate --eval -E '<nixpkgs>'` on gex45 prints the
  flake's nixpkgs.
