# GitHub issues

Read this to create or edit issues, labels, parent links or a leaf's Done when.

## Creating and labeling

- Create issues only when Tom authorizes tickets or agenda decomposition, because issues are his backlog.
- Resolve the repository, read its instructions and templates, and fetch current labels plus comparable recent issues before edits, because conventions differ per repository.
- Match established scope, theme, priority and status labels at creation and omit dimensions without a fitting label, because invented labels fragment search.
- Label agent-proposed issues `priority:later` unless Tom ranks them, and infer no priority from recency, because ranking is Tom's.
- Reopen a standing bot issue instead of filing a new one, because duplicates split its history.
- Preserve legitimate labels and the requested body, checklist and state during metadata repairs, because a repair must not change content.
- Limit batches to named tickets or evidenced same-session omissions, because speculative batches flood the backlog.
- File one issue per independent fix when an audit or review finds several, never one issue for the whole audit, because each fix finishes independently; a split of a claimed task puts its parts as children of that issue (a `Parent: REPO#N` line in the body, bounded by `worker_children_per_session`), a finding goes to a child of its epic, and the `gh issue create` guard refuses a follow-up of a claimed issue that lacks that line and a title matching an open issue.
- Read each issue back to verify the requested content and labels, and report its link, because `gh` can succeed with dropped fields.

## Parents, children and Done when

- Open one issue per unit of work and link it to its parent when you create it (`createIssue` with `parentIssueId`, or `addSubIssue` on an existing issue), because the session tree follows the issue tree.
- Put the parts of a split as children of the issue that was split, and a finding as a child of its epic, because a part or finding written into a body cannot be claimed, staffed or closed on its own.
- Write a leaf's Done when as the check that closes it, written as ``Done when: `CMD` passes`` (or `prints X`, `prints nothing`), because the landing records the command that passed on the lane's tree and closes the leaf only when it equals the issue's own Done when.
- Count completion only from a leaf's own check or from its children all closed, never from a component check, because a passing part does not finish the issue.
- Give each leaf one verification class (code, headless run, farm field, native check, product proof, Tom's direction or feel judgment, time-gated) and name the run that passes it, because mixed classes stall on the slowest one.
- End a leaf that needs a place beyond a build host with `Needs: NAME, NAME` (`laptop`, `fleet`, a host name, `checkout:REPO`, a local need such as `native`), because north fleet dispatch places a launch on what the issue declares and on the need a worker recorded with `north fleet need`, never on its prose, so an issue with no `Needs:` line goes to any worker host.
- Link a leaf that another issue already owns to that issue and never copy it in, because copies drift.
- Keep a shared target (such as a 45-55% balance band or a frame budget) in the one issue that owns it; other issues' checks require only no regression against main, because one owner decides the target.
- File native, Tom-decision and time-gated proofs as their own child when the code they verify can land first, because they otherwise block landing.
- Rewrite an issue's Status and route in the same edit that changes its verification class, because the route follows the class.
- Write no clean-up leaves (tests turned down, scaffolding removed, nothing left for later), because the landing enforces them on every lane through the test-admission and mutant-kill land checks and `north land scan leftovers`.
