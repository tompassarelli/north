# Betterleaks against gitleaks: measured scan times

Measurements behind north#642 (see [betterleaks.md](../betterleaks.md)), taken on the laptop on 2026-11-06 with gitleaks 8.30.1 on its default config and betterleaks 1.10.0 on north's own config.

Median of 3 to 9 runs.

Range scan of `origin/main~N..origin/main` (gitleaks 8.30.1 with its default config; betterleaks 1.10.0 with the config above):

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

## Checked on 2026-11-06

Current-main scans with the compiled-in config: the full history of north, south, nixos-config, firn and wisp reports no finding, and makgora's reports only the two historic test keys above; its current tree, scanned the way the lander scans it (from inside the directory), reports none. The north test `every_guarded_secret_type_is_caught_in_a_range_and_clean_text_passes` plants an age key, an OpenSSH key, a GitHub App key, a GitHub token (classic and fine-grained), and Hetzner, DigitalOcean, Cloudflare and Anthropic tokens and passes. Hosts: gex45 resolves `betterleaks` to the pinned 1.10.0 in `/run/current-system/sw/bin`; south enables the `betterleaks` module on nexus, gex45 and ccx33, and the laptop takes it from nixos-config's module.


Where 1.10.0 comes from: the nixos-config flake overlays `pkgs.betterleaks` with the upstream release tarball (a fetch and one `install`, no compile), and its devshell takes the same derivation, so a host switch copies one small path and no landing waits on a build. Observed on 2026-11-06 from the laptop: `betterleaks version` prints 1.10.0 from `/run/current-system/sw/bin/betterleaks` on whiterabbit, nexus, gex45 and ccx33.
