---
name: nix-development
description: >-
  Create or repair project-local Nix environments and consumed flake outputs outside nixos-config.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Project Nix

- Use existing manifests, flake locks, commands and supported platforms, because the project's pins define what is reproducible; route system configuration to `firn`, which owns it.
- Follow the project's typed source and immutable compiler pin for new Tom-owned Nix semantics, so generated Nix stays reviewable and reproducible.
- Leave working plain Nix unchanged outside the requested semantic edit, because rewrites risk behavior nobody asked to change.
- Declare only consumed shells, packages, apps, checks or modules and reuse native toolchain manifests, so the flake exposes no surface nobody maintains.
- Include the real build's compiler, package manager, linker, headers and tools with minimal deterministic shell hooks, because a shell missing one of them passes eval and fails the build.
- Generate and validate source, then run the actual project command inside the declared environment, because an environment that evaluates is not proof it builds.
- Stage named new Git-flake inputs or use an explicit path URL, because Git flakes ignore untracked files.
- Separate evaluation, input, filtering and lock failures from project failures, so you fix the Nix layer and the project at their own causes.
- Keep credentials, unrequested caches and global settings and ad-hoc PATH installations outside the change, because they leak secrets or make the result differ per machine.
- Report the working output and platform limits, so the next reader knows what was proven and where.

Use [environment design](references/environment-design.md) or [evaluation](references/evaluation.md) for a named unresolved detail.
