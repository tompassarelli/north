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

On every measured range and every full history the new scanner is faster than the one it replaces; the numbers are in [decisions/betterleaks-scan-times.md](decisions/betterleaks-scan-times.md). Range scans of 3 commits take 17 to 21 ms, of 40 commits 26 to 92 ms; the full history of north takes 6.9 s, down from 16.3 s.

## Why a pinned binary, not an override of nixpkgs

The config only needs rules and path allowlists that 1.3.1 also understands; the pin is for speed alone (start-up 100 ms to 18 ms, and `--trust-local-git-config`, which 1.3.1 lacks). Drop the pin when nixpkgs reaches 1.10.
