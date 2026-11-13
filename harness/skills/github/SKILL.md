---
name: github
description: >-
  GitHub via `gh`: issues with parent links and children, labels, a leaf's Done when; Actions workflows and runs; many agents within job and API limits.
grounded: 2026-11-12
gh-version: 2.103.3
written: 2026-11-12
metadata:
  kind: playbook
---

# GitHub

## Which reference

- Creating or editing an issue, its labels, its parent and children or a leaf's Done when: read [issues](references/issues.md).
- Writing a workflow, dispatching or waiting on runs, many agents calling GitHub, or a rate-limit refusal: read [actions](references/actions.md).

## Shared rules

- Read the repository's instructions before writing to it, because each repository sets its own labels, templates and CI shape.
- Spend API calls and Actions jobs as a shared budget, because one account serves every repository and agent.
- Read back what you wrote (issue, label, run state), because a successful `gh` exit does not show the remote result.
- Use `gh --help` and its subcommand help for flags, because they track the installed version.
