# Betterleaks: one secret scanner, tuned for the guarded secrets and fast landings

Tom asked on 2026-11-06 for betterleaks (MIT, https://github.com/betterleaks/betterleaks) in place of the previous scanner wherever a repository or host scans for secrets, configured so it adds as little time as possible to builds and landings (north#642).

## What runs

- The binary is the released static `betterleaks` 1.10.0 for x86_64 Linux, fetched by hash in nixos-config's flake (`betterleaksRelease`), so nothing waits on a Go build; hosts without that platform fall back to nixpkgs' `betterleaks`. nixpkgs carries 1.3.1.
- One config, `crates/north-base/src/platform/betterleaks-secrets.toml`, is compiled into the north binary and handed to every scan through `BETTERLEAKS_CONFIG_TOML`. It drops betterleaks' default rule set and lists only what is guarded: age secret keys, PEM and OpenSSH private keys (ssh keys, GitHub App keys), GitHub tokens, Hetzner, DigitalOcean, Cloudflare and Anthropic tokens, and firn's Nix password and WiFi rules. Build outputs, caches, lockfiles, binary media and the vendored `repos/effect/` are skipped by path. A repository's own `.betterleaks.toml` is not read, so a scanned branch cannot loosen its own scan.
- The private deny-list (`south/privacy/public-denylist.toml`) keeps its own config and is passed with `--config`.
- Flags every scan passes: `--regex-engine stdlib` (the default re2 engine is WebAssembly and costs ~80 ms at start on 1.3.1 and ~10 ms on 1.10.0 before reading a byte), `--trust-local-git-config` on history scans (without it 1.10.0 builds a sanitised copy of the repository first, 90 ms on north's 3.6 GiB repository; our scans only ever read our own checkouts), `--redact`, `--log-level error`.
- The JSON report keeps the previous scanner's field names (`RuleID`, `File`, `StartLine`, `Fingerprint`), checked against 1.3.1 and 1.10.0 output.
- `--git-workers` gave no gain on any measured range, so it is not passed. Inline allow comments are honoured on range scans as before and ignored by the deny-list scan, which passes the flag that upstream still spells after its old name.

## Measured

On every measured range and every full history the new scanner is faster than the one it replaces. The figures are medians of 3 to 9 runs on the laptop on 2026-11-06, with gitleaks 8.30.1 on its default config against betterleaks 1.10.0 on north's own config.

Range scan of `origin/main~N..origin/main`:

| repository | commits | gitleaks | betterleaks |
|---|---|---|---|
| north | 3 | 44 ms | 21 ms |
| north | 40 | 114 ms | 92 ms |
| nixos-config | 3 | 49 ms | 20 ms |
| nixos-config | 40 | 82 ms | 34 ms |
| south | 3 | 53 ms | 17 ms |
| south | 40 | 55 ms | 26 ms |

Full history of `origin/main`:

| repository | commits | gitleaks | betterleaks |
|---|---|---|---|
| south | 675 | 967 ms | 205 ms |
| nixos-config | 2632 | 2090 ms | 1347 ms |
| north | 4581 | 16346 ms | 6858 ms |

Why not nixpkgs' 1.3.1: on the same ranges it took 104 to 160 ms for 3 commits (gitleaks 31 to 51 ms), because it loads the WebAssembly regex engine and a tokenizer before scanning, and spends 90 ms at start even for an empty range. 1.10.0 starts in 18 ms.

Full history of makgora (7789 commits, 21 s) reports two findings, both TLS test keys in the first import of the vendored Effect packages (commit 66c1182e); the other repositories report none.

Checked on 2026-11-06: current-main scans with the compiled-in config of the full history of north, south, nixos-config, firn and wisp report no finding, and makgora's reports only the two historic test keys above; its current tree, scanned the way the lander scans it (from inside the directory), reports none. The north test `every_guarded_secret_type_is_caught_in_a_range_and_clean_text_passes` plants an age key, an OpenSSH key, a GitHub App key, a GitHub token (classic and fine-grained), and Hetzner, DigitalOcean, Cloudflare and Anthropic tokens and passes. On the build hosts `betterleaks` resolves to the pinned 1.10.0 in `/run/current-system/sw/bin`, and the nixos-config module enables it on the other hosts.

Where 1.10.0 comes from: the nixos-config flake overlays `pkgs.betterleaks` with the upstream release tarball (a fetch and one `install`, no compile), and its devshell takes the same derivation, so a host switch copies one small path and no landing waits on a build. Observed on 2026-11-06 from the laptop: `betterleaks version` prints 1.10.0 from `/run/current-system/sw/bin/betterleaks` on four hosts.

## Why a pinned binary, not an override of nixpkgs

The config only needs rules and path allowlists that 1.3.1 also understands; the pin is for speed alone (start-up 100 ms to 18 ms, and `--trust-local-git-config`, which 1.3.1 lacks). Drop the pin when nixpkgs reaches 1.10.
