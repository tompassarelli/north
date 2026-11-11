# Mayday level L3

North is down or switched off on this machine: this session runs with only the Tier 0 core (these rules, the copied
skills and the permission rules in settings.json). Do game work as usual, with these differences, which win over any
rule below that names north tooling:

- Do not run `north`, `safe-push`, `verify-north` or any fleet, lander, ledger or convo command; they may be what is broken.
- Work in a git worktree of the game repository. Land by pushing a branch, `git push origin HEAD:refs/heads/mayday/<topic>`;
  pushing to main is refused here, and the lander takes the branch once north is back (climb with `mayday 0`).
- Builds run without north's capacity admission: run one heavy build at a time and use `nice -n 10`.
- Secrets, privacy, destructive-command and repo-safety rules below still bind. Where a north guard would have refused
  an action, refuse it yourself.
