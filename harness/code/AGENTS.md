# Code layout

- Treat `~/code/<project>/` as a container with `main/`, `worktrees/<slug>/` and immutable `pins/<full-object-id>/`.
- Treat `~/code/resources/` as read-only third-party material and check licenses before reuse.
- Keep `~/code/clients/` confidential and runtime data directories outside project ownership.
- Use `repo-safety` for worktree creation, landing, rescue and pin retirement.
- Keep code within the repository's `structure.toml` and follow `program-craftsmanship` when the `code-health` gate or edit-time hook fires.
- Price an infra or tooling box with `iteration-economics` before staffing it: its brief states minutes saved per change and payback.
