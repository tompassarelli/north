---
name: github
description: >-
  GitHub via `gh`: issues, labels, Done-when boxes; Actions workflows and runs; many agents within job and API limits.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# GitHub

## Which reference

- Creating or editing an issue, its labels or its Done-when boxes: read [issues](references/issues.md).
- Writing a workflow, dispatching or waiting on runs, many agents calling GitHub, or a rate-limit refusal: read [actions](references/actions.md).

## Shared rules

- Read the repository's instructions before writing to it, because each repository sets its own labels, templates and CI shape.
- Spend API calls and Actions jobs as a shared budget, because one account serves every repository and agent.
- Read back what you wrote (issue, label, run state), because a successful `gh` exit does not show the remote result.
- Use `gh --help` and its subcommand help for flags, because they track the installed version.
