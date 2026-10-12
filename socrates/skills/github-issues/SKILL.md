---
name: github-issues
description: >-
  Create or edit GitHub issues, including issue labels, using the repository's existing ticket and tagging conventions.
grounded: 2026-10-10
written: 2026-10-10
metadata:
  kind: playbook
---

# GitHub issues

- Create issues only when Tom authorizes tickets or agenda decomposition.
- Resolve the repository, read its instructions/templates and fetch current labels plus comparable recent issues before edits.
- Match established scope/theme/priority/status labels at creation; omit dimensions without a fitting label.
- Infer no priority from recency or completion from a component check.
- Preserve legitimate labels and requested body/checklist/state during metadata repairs.
- Limit batches to named tickets or evidenced same-session omissions.
- Read each issue back to verify requested content/labels and report its link.
- Count completion only from the issue's acceptance checks.
- Give each Done-when box one verification class (code, headless run, farm field, native check, product proof, Tom's direction or feel judgment, time-gated) and name the run that passes it.
- Make a Done-when box machine-checkable where possible, as a named command and its expected output, so an auto-close job can run it on main.
- File one issue per fix when an audit or review finds several, never one issue for the whole audit.
- Link a box that another issue already owns to that issue; never copy it in.
- Run `issue-box-lint REPO#N` after writing boxes; it flags any box that hands Tom a mechanical check.
- Keep a shared target (such as a 45–55% balance band or a frame budget) in the one issue that owns it; other issues' boxes require only no regression against main.
- File native, Tom-decision and time-gated proofs as their own issue when the code they verify can land first.
- Rewrite an issue's Status and route in the same edit that changes a box's verification class.
- Label agent-proposed issues `priority:later` unless Tom ranks them; reopen a standing bot issue instead of filing a new one.
