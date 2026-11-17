# DeepSeek overlay

Each line comes from a rejection or fix the Sonnet lander made on a DeepSeek commit; the issue in brackets is where it was seen.

- Run a new test against origin/main without your change before you keep it. If it passes there it pins nothing and the landing refuses it. (north#864, north#924)
- Do not raise the test count. The repository's verification budget is 20 s, so put a new assertion into the existing test that covers the same guarantee. (north#966)
- Before you restore code that is missing, run `git log -S'<text>' origin/main`. If a commit removed or reverted it on purpose, leave it out and say so in your report. (north#947)
- Read every Done-when box first. When every open box is date-gated ("not before DATE"), a live-host reading, or a person's judgement, make no commit and report `Stop: wait until DATE` in a few lines. (north#862, north#872, north#863)
- Tests read time, hosts and processes only through injected fixtures: no /proc, no wall clock, no real host state. The ambient-input check refuses the rest. (north#962, north#952)
- Commit messages carry no cost or price figures; the privacy scan refuses them. (north#910)
- Never tick a box or edit an issue. Report each box's exact command and its output; the lander ticks the box from your commit's `Item:` and `Box:` lines once it lands.
- Hand in your own lane with safe-push when it changes no ring-0 path and adds no dependency: the review-accept scan safe-push runs lets such a lane through. A lane that needs a review accept is refused with "lane-only session: the lane changes ring-0 paths or adds dependencies (...), so it needs a review accept and must come from a Claude session; end with Done: <worktree> <branch> <sha>"; then commit in your lane with the brief's `Item:` and `Box:` lines in the message and report Done: with that line. (north#1041, north#1650)
- A box's guarantee is met only by running its Done-when command, not by reading code. Say "not run" when you could not run it.
- When an issue is a tracker with no code box open, make no commit and say so in one line. (north#870)
