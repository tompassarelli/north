# Tiers and landing fast

Read when placing a test or sweep in a tier, choosing what the landing gate or CI runs, or when a build, test or landing is slow.

## Tiers
- **Edit loop (under ~60 s, local)**: type-check plus the tests of what changed; a worker runs this before pushing.
- **Landing gate**: tests whose inputs changed, one seeded representative of each sweep family whose guarantee no ordinary test covers, and one seconds-long end-to-end path, fast enough to land changes 1.5x faster than they arrive, because a queue that grows never drains. A new sweep runs after landing unless it earns a gate slot.
- **After landing**: every sweep, full roster and wide property search on each landed SHA, on runners that exist; bisect a failure, confirm it on the parent, auto-revert the culprit. Check a tier ran before moving work into it, because a tier nobody runs protects nothing.
- **Canaries**: give each check family the landing gate and CI keep one recorded planted fault that CI applies and that must fail, because a check that silently stops checking still looks green.
- **Nightly / manual**: soaks, native pairs, cost censuses.

## Landing fast
- Reuse every build and test result whose inputs are unchanged, keyed by a content hash of those inputs, and never run one suite twice for one landing, because rerunning unchanged inputs is waste.
- Give each repeated stage between finished work and the owner's hands (build, test, land, install) a wall-time budget, and have the tool that already logs its time open or update one issue for an agent when a run exceeds it, because unreused work is found from timings already logged, not by the owner or a schedule; such budgets alert and never gate.
- Before landing, prove only that the change compiles and run the tests that exercise it by name filter; never run or offload the whole suite as a pre-landing step, because the landing (the lander, or safe-push's train where no lander runs) tests one candidate, main plus every queued lane, with every land check and splits by halves on red, so a pre-run only duplicates it and queues every lane behind one slot (on 2026-11-04 about 15 workers each queued their own full suite on one offload slot and waited 600 s per refused attempt, while one train run covered them all). A lane the landing returns red is fixed and re-pushed by its own worker. The exception is a change to the landing machinery itself (bin/safe-push, the lander, harness/scripts/land-checks*.sh, .safe-push, the land-check workflows), which runs its own tests first, because a broken landing blocks every lane.
- Call an external tool once per batch, not per file, row, test or branch; fall back per item only for exact diagnostics on failure, because process start dominates small calls.
- Bound cost in a deterministic unit (instructions, frames, allocations), never wall seconds, because wall time flakes under load; shrink inputs or move work after landing instead of raising a ceiling. Run a timing check once, under an exclusive capacity lease.
