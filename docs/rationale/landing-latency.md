# Landing latency: where a worker's commit waits before main

Measured on Smashcraft, 2026-10-28, while about 15 workers waited on their landings during a live
playtest. The numbers for that day are in South (agents/decisions/smashcraft/landing-speed-2026-10-28.md);
this file keeps the method and what each fix removed.

## Method

One landing is a worker commit and the queue branch that carries it. Join four records by commit:

- Commit time: the author date of each commit on main (`git log origin/main --format='%at %ct %s'`),
  which rebases and the Autoland lane trailer keep. Commit to main is `%ct - %at`; leave out
  Autoland's own `Regenerate reports` commits.
- Queue branch pushed: `~/.local/state/safe-push/pushes.tsv` (time, repository, `claude/land-*`
  branch, tip).
- Autoland stages: the run titles `Autoland test SLOT BRANCH SHA` and `Autoland train`
  (`gh run list --workflow autoland.yml --json displayTitle,createdAt,updatedAt,conclusion`), and the
  `autoland` commit statuses on each tip (`queued`, `testing alone on`, `passed alone on`,
  `landed as`, or a failure with its reason).
- Worker side: `pre-push: NAME passed (N s)` lines and `DEFER_*` admissions in the session
  transcripts (`north fleet show NAME`).

Report the median and p75 of commit to main for the window, then split the tail by stage: commit
to queue push, push to test start, the test, and test end to main, plus how many tests handed the
lane back to its worker and why.

## What the 2026-10-28 profile showed

- A clean landing costs about 7 minutes: a minute to the test slot and a 6-minute suite, after
  which the test run lands the lane itself.
- The tail came from hand-backs, not from queueing: a quarter of Autoland's tests returned the lane
  to its worker (conflicts with main, a stale Classic effect record, a red main), and each one
  restarts the worker's rebase, pre-push and test.
- Tips that passed alone while main moved on their paths waited for a train, which reran the suite.
- Rerunning a stopped `safe-push --to main` repeated every check even when Autoland already held
  the commit.
- Workers launched with briefs that declare no `Surface:` stayed on the laptop, where the gaming
  hold defers heavy and native work for as long as Tom plays.
- North's own landings could not offload their land checks while Tom played: the offload needs a
  runtime newer than the one installed, and the runtime build is itself heavy work the hold defers.

## Fixes and their rule

- A pre-push check whose fix needs the local machine runs before the hand-off as well as in
  Autoland (smashcraft `localFix`), because Autoland can only hand such a lane back.
- `safe-push --to main` resumes the wait when origin already holds `claude/land-<lane>-<HEAD>` at
  HEAD, because the checks already ran for that exact commit.
- Placement treats a box with no `Surface:` as `Needs: native, private` while this host is in game
  reserve, so it goes to a machine that serves them instead of waiting out the game.
