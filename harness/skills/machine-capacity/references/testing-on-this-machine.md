# Testing on this machine

Read with the test rules of the `verification` skill
(`agent-machinery/skills/verification/SKILL.md`) when a test run, mutant or
landing on this machine needs its local tool names.

- The landing tool is `safe-push`. It refuses an added `MUTANT:<rule-id>` line, an added `skip`, `only`, `#[ignore]` or `xfail` without `Test-change:`, an added test without `Test-kind: <name> <kind> kills <mutant>`, and, over a `.safe-push` `ci.budget`, a landing without `CI-budget:` or `Main-red:`.
- Record a mutant with `mutant record <id>` in the scratch copy before claiming a kill; `safe-push` runs `north mutant verify <base> <head>`, which refuses an empty record or a patch that applies to neither and, where `.safe-push` sets `mutant.run`, runs every claimed test once on the change and on each mutant under one lease, so do not hand-run kills; the runner writes `pass<TAB><name>` or `fail<TAB><name>` lines to `<tree>/.mutant-results` for each tree in `MUTANT_TREES`.
- Run a timing check once under an exclusive lease, `machine-capacity run --class exclusive --owner OWNER --timeout-seconds N -- COMMAND` (15 minutes at most), because timing under peer load measures the peers.
- Run full `bun test` suites, `cargo build`, `bun wisp map build` and headless wisp renders through `machine-capacity run`; `heavy-command-guard` refuses them unwrapped. It routes cargo test, tsc --build and multi-file bun test through admission automatically; cargo check and single test files remain local.
- Set `AGENT_CAPACITY_SUBSLICE=TAG` (lowercase letters and digits) on any test that starts real capacity scopes or sets a mode, because `run`, `session` and `mode` apply the profile's CPUQuota and weights to their slices; with it they go to `agent-capacity-TAG.slice` and `native-TAG.slice` nested inside the live slices, which they never change.
- Check that a runner actually ran the after-landing tier on the landed SHA before moving work into it, because only the runners that exist here count.
- The test-lean reviews behind the delete-on-sight counts are `~/.local/state/agents/handoffs/test-lean-<repo>.md` for each repository south:agents/references/test-lean.md lists; `agent-test-failure-research.md` and `codex-lessons.md` sit in the same directory; the mutant incidents are north commits 174d7b7c and 4e8ea820.
