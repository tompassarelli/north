# Clean-up scans: which leftovers a landing can refuse

Every north issue used to carry three `Clean-up:` boxes (development tests turned down, scaffolding removed, nothing left
for later), each judged by a landing reviewer that most landings never get. On 2026-11-12, 336 of them were open across
112 issues, and 30 issues with no other work left stayed open on them. The owner decided on 2026-11-11 that the three
lines become land checks, and on 2026-11-12 that they run as deterministic scans, with no model call (north#1137).

## Replay

Each candidate rule ran over the lines added by the last 300 lanes landed on north main (111b7d71cc to 2fe8d016b0,
2026-11-11 to 2026-11-12). A rule ships only if at least 80% of its refusals are true leftovers. Where a rule refused
more than 20 lanes, 20 were sampled.

| Rule | Lanes refused | True leftovers | Verdict |
| --- | --- | --- | --- |
| `dbg!` | 0 | - | dropped: nothing to refuse |
| `println!`/`eprintln!` outside `src/cli/` | 72 | 0 of 20 | dropped: command output lives in every crate |
| the same, narrowed to debug formatting (`{:?}`) | 4 | 0 of 4 | dropped |
| an added file whose name nothing in the tree references | 38 | 0 of 20 | dropped: per-unit trees (`src/cli/reference/`, `src/cli/verbs/`, `structure/`) are read by directory |
| the same, narrowed to files whose directory is referenced nowhere either | 0 | - | dropped |
| a flag in usage or reference text that no Rust string matches | 0 | - | dropped |
| an environment variable the lane sets (`.env(`, `set_var(`, `export`) that nothing reads | 2 | 0 of 2 | narrowed: `GIT_PROTOCOL` is read by git |
| the same, narrowed to `NORTH_`, `SAFE_PUSH_` and `AGENTS_` names | 1 | 0 of 1 | dropped: the test sets the retired variable on purpose, to show the sandbox ignores it |
| `TODO`, `FIXME`, `XXX` | 2 | 0 of 2 | dropped: both are `mktemp` templates |
| "follow-up", "for now", "temporary" | 9 | 0 of 9 | dropped: prose, the fleet's follow-up message, a `temporary` variable |
| a Rust module file that no `mod`, `#[path]` or `include!` reaches | 1 | 1 of 1 | shipped |

The one true refusal: eb3111123f replaced the replay's `lane_build` with `tree_build` and left `lane_build.rs` behind
with no `mod` line, which 45972baeb9 deleted. The variable refusal looked true and was not: bbeb7b9bc0 stopped reading
`SAFE_PUSH_LAND_HOST_INPUTS`, and its test still sets the variable to show that a store entry passed through it no longer
reaches the sandbox.

## Tests turned down

The existing land checks already carry this intent. `land scan test-admission` refuses a `Test-contract:` trailer whose
category is not in the repository's list or whose test the lane does not add, and, over the verification time budget, a
lane that adds more tests than it deletes unless each added test pins a listed contract or is the one regression test of
a bug the lane fixes. After landing, fail-to-pass refuses a bug test that passes with the lane's code changes reverted,
and `test-admission kills` runs the recorded mutants. Of the 104 replayed lanes that added a Rust test, 94 grew the test
count and 30 carried a `Test-contract:` trailer. What stays uncovered is a test that pins nothing and lands while the
repository is under its time budget: whether a test pins a guarantee a person would notice is a judgment, and no scan
replaces it.

## What ships

`north land scan leftovers BASE..HEAD` runs the shipped rule on every lane at enqueue, charging the lane only for what
its merge base did not already have, and names the file and line it refuses.
