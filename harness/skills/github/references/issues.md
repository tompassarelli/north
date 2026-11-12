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
- Make a box machine-checkable where possible, written as ``Done when: `CMD` passes`` (or `prints X`, `prints nothing`), because after-landing runs that command for the box a `Box:` trailer names and ticks it with the `Evidence:` stamp the policy climber counts as checker-verified, while a box ticked with only its landed sha counts as self-ticked.
- Run `north check issue-boxes REPO#N` after writing boxes, because it flags any box that hands Tom a mechanical check.
- End a box that needs a place beyond a build host with `Needs: NAME, NAME` (`laptop`, `fleet`, a host name, `checkout:REPO`, a local need such as `native`), because north fleet dispatch places a launch on what its boxes declare and on the need a worker recorded with `north fleet need`, never on a box's prose, so a box with no `Needs:` line goes to any worker host; one box that needs nothing places the issue.
- Link a box that another issue already owns to that issue and never copy it in, because copies drift.
- Keep a shared target (such as a 45-55% balance band or a frame budget) in the one issue that owns it; other issues' boxes require only no regression against main, because one owner decides the target.
- File native, Tom-decision and time-gated proofs as their own issue when the code they verify can land first, because they otherwise block landing.
- Rewrite an issue's Status and route in the same edit that changes a box's verification class, because the route follows the class.
- Write no clean-up boxes (tests turned down, scaffolding removed, nothing left for later), because the landing enforces them on every lane through the test-admission and mutant-kill land checks and `north land scan leftovers`, and a landing that ticks an issue's last box closes it.
