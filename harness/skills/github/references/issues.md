# GitHub issues

Read this to create or edit issues, labels or Done-when boxes.

## Creating and labeling

- Create issues only when Tom authorizes tickets or agenda decomposition, because issues are his backlog.
- Resolve the repository, read its instructions and templates, and fetch current labels plus comparable recent issues before edits, because conventions differ per repository.
- Match established scope, theme, priority and status labels at creation and omit dimensions without a fitting label, because invented labels fragment search.
- Label agent-proposed issues `priority:later` unless Tom ranks them, and infer no priority from recency, because ranking is Tom's.
- Reopen a standing bot issue instead of filing a new one, because duplicates split its history.
- Preserve legitimate labels and the requested body, checklist and state during metadata repairs, because a repair must not change content.
- Limit batches to named tickets or evidenced same-session omissions, because speculative batches flood the backlog.
- File one issue per independent fix when an audit or review finds several, never one issue for the whole audit, because each fix finishes independently; pieces of one claimed task are boxes on its issue, and the `gh issue create` guard refuses a split of it and a title matching an open issue.
- Read each issue back to verify the requested content and labels, and report its link, because `gh` can succeed with dropped fields.

## Done-when boxes

- Count completion only from the issue's acceptance checks, never from a component check, because a passing part does not finish the issue.
- Give each box one verification class (code, headless run, farm field, native check, product proof, Tom's direction or feel judgment, time-gated) and name the run that passes it, because mixed classes stall on the slowest one.
- Make a box machine-checkable where possible, as a named command and its expected output, so an auto-close job can run it on main.
- Run `north issue-box-lint REPO#N` after writing boxes, because it flags any box that hands Tom a mechanical check.
- Link a box that another issue already owns to that issue and never copy it in, because copies drift.
- Keep a shared target (such as a 45-55% balance band or a frame budget) in the one issue that owns it; other issues' boxes require only no regression against main, because one owner decides the target.
- File native, Tom-decision and time-gated proofs as their own issue when the code they verify can land first, because they otherwise block landing.
- Rewrite an issue's Status and route in the same edit that changes a box's verification class, because the route follows the class.
