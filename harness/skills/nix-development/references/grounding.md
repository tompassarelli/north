# Grounding

Each review rereads upstream and lists what changed and what held, newest first.

## 2026-11-05

Read on 2026-11-05 from NixOS/nix `7ee0a4d`, NixOS/nixpkgs `0600874` and NixOS/nix.dev `8c3b8c3`, against north's `docs/rationale/nix-current-practice.md` (2026-11-04):

- Changed: `formatter` joins the outputs to declare once the project formats Nix, because `nix fmt` is `nix formatter run` of that output and no repository declares one (https://github.com/NixOS/nix/blob/7ee0a4d5bca3e8398032998f4293b37060d1e08a/src/nix/formatter-run.md, 2026-11-05).
- Held: a Git flake reads only files added to Git and warns on a dirty tree (https://github.com/NixOS/nix/blob/7ee0a4d5bca3e8398032998f4293b37060d1e08a/src/nix/flake.md, 2026-11-05); unreleased notes extend tracked-only reads to Mercurial checkouts (https://github.com/NixOS/nix/blob/7ee0a4d5bca3e8398032998f4293b37060d1e08a/doc/manual/rl-next/local-repo-detection.md, 2026-11-05).
- Held: Nix 2.35.0 (2026-06-22) hashes flake sources without a store copy and `nix flake check` makes out links only with `--out-link`; no rule here depends on either (https://github.com/NixOS/nix/blob/7ee0a4d5bca3e8398032998f4293b37060d1e08a/doc/manual/source/release-notes/rl-2.35.md, 2026-11-05).
- Held: flakes stay experimental and nix.dev sends new non-flake code to npins, so following the project's existing manifests and pins stands (https://github.com/NixOS/nix.dev/blob/8c3b8c3a71e61d060a05449f48c12d5d3b46335f/source/concepts/flakes.md, 2026-11-05).
- Held: NixOS 26.11 notes retire `nixexprs.tar.xz` after 2027-12-31, which touches only channels.nixos.org tarball inputs, not `github:NixOS/nixpkgs` (https://github.com/NixOS/nixpkgs/blob/0600874e4ad3ada61aad5fc1cb17ae712a9c06d8/nixos/doc/manual/release-notes/rl-2611.section.md, 2026-11-05).
