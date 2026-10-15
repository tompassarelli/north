# Testing on this machine

Read with the test rules of the `verification` skill
(`agent-machinery/skills/verification/SKILL.md`) when a test run, mutant or
landing on this machine needs its local tool names.

- The landing tool is `safe-push`. It refuses an added `MUTANT:<rule-id>` line, an added `skip`, `only`, `#[ignore]` or `xfail` without `Test-change:`, an added test without `Test-kind: <name> <kind> kills <mutant>`, and, over a `.safe-push` `ci.budget`, a landing without `CI-budget:` or `Main-red:`.
- Record a mutant with `mutant record <id>` in the scratch copy before claiming a kill; `safe-push` runs `mutant check <id> <base> <head>` and refuses an empty record or a patch that applies to neither.
- Run a timing check once under an exclusive lease, `machine-capacity run --class exclusive --owner OWNER --timeout-seconds N -- COMMAND` (15 minutes at most), because timing under peer load measures the peers.
- Run full `bun test` suites, `cargo build`, `bun wisp map build` and headless wisp renders through `machine-capacity run`; `heavy-command-guard` refuses them unwrapped.
- Check that a runner actually ran the after-landing tier on the landed SHA before moving work into it, because only the runners that exist here count.
- The test-lean reviews behind the delete-on-sight counts are `~/.local/state/agents/handoffs/test-lean-<repo>.md` for galileo, firn, north, smashcraft, wisp, south and proton-triage; `agent-test-failure-research.md` and `codex-lessons.md` sit in the same directory; the mutant incidents are north commits 174d7b7c and 4e8ea820.
