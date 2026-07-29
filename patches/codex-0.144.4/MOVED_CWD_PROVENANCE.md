# Codex 0.144.4 moved-CWD patch provenance

This North patch is applied only to the exact OpenAI Codex 0.144.4 source
identity pinned in `PROVENANCE.md` and `flake.nix`.

## Behavior and safety boundary

Codex records a semantic working-directory pathname in each turn. If that
directory is renamed while a session remains active, the pathname becomes
stale even though the Codex process still has a valid current-directory inode.
Upstream starts hook processes in the stale pathname before applying a tool's
own working directory, so every hooked tool fails to launch.

`moved-cwd-hook-launch.patch` changes only the process working directory used
to start a hook:

- an existing semantic turn directory is still used unchanged;
- when that pathname no longer exists, Codex uses its live process current
  directory if it resolves to an existing directory;
- the hook JSON retains the original semantic `cwd`;
- tool authorization and filesystem sandbox roots are unchanged;
- if neither directory is usable, hook launch still fails and North's managed
  fail-closed policy still denies the tool.

The modified upstream file is
`codex-rs/hooks/src/engine/command_runner.rs`. Unit tests cover the moved
directory fallback and preservation of the genuine fail-closed failure path.

Patch SHA-256:
`09c1ea4ec9e6f91cc7a21b88da49546c678355f45594c79388494ec5e893290a`.
