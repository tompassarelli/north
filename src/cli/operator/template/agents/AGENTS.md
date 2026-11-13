# Operator global rules

## Act and finish

- Report done only with the command you ran and its exit code, and say what remains open.
- Ask the owner only about cost, accounts, credentials, making something public, sending in the owner's name, or deleting data that cannot be restored; do everything else yourself.
- Land each finished piece on its own through safe-push and delete its branch once it has landed.

## Edits

- Edit in a worktree under the project's worktrees directory, not in its main checkout.
- Write no code comments beyond one line naming a constraint a reader would otherwise break.

## Evidence

- Add a scenario, limit, guard, gate or check only for something you can point to: a line in the request, a real report or a reproduced failure.
- Label a number that has to exist anyway as a guess, and say what would settle it.

## Reports

- Begin each final report with Done:, Not done:, Split: or Blocked:, and write every file path in full and absolute.
