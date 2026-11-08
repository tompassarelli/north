# Testing on this machine

Read with the test rules of the `verification` skill
(`harness/skills/verification/SKILL.md`) when a test run, mutant or
landing on this machine needs its local tool names.

- To check a doubted kill on demand, record the mutant with `mutant record <id>` in the scratch copy and run `north test mutant verify <base> <head>`, which refuses an empty record or a patch that applies to neither and, where `.safe-push` sets `mutant.run`, runs every claimed test once on the change and on each mutant under one lease, so do not hand-run kills; the runner writes `pass<TAB><name>` or `fail<TAB><name>` lines to `<tree>/.mutant-results` for each tree in `MUTANT_TREES`.
- Run a timing check once under an exclusive lease, `machine-capacity run --class exclusive --owner OWNER --timeout-seconds N -- COMMAND` (15 minutes at most), because timing under peer load measures the peers.
- A command routed through admission must not also write files or the git index, because an offload runs all of it on the build host; run the edit or `git add` first, then the build alone.
- Set `AGENT_CAPACITY_SUBSLICE=TAG` (lowercase letters and digits) on any test that starts real capacity scopes or sets a mode, because `run`, `session` and `mode` apply the profile's CPUQuota and weights to their slices; with it they go to `agent-capacity-TAG.slice` and `native-TAG.slice` nested inside the live slices, which they never change.
- Check that a runner actually ran the after-landing tier on the landed SHA before moving work into it, because only the runners that exist here count.
- Each check family the landing gate and CI keep has a planted fault in `harness/canaries/` (a patch plus the line its check must print, in `canaries.conf`); the Canary workflow applies each on every push to main through `harness/scripts/canary.sh FAMILY`, which reads the family's command from its workflow step or `.safe-push` line, so a check that stops checking turns main red. A new check family adds a canary; a canary whose killing test you delete moves to another test in the same change.
- The change graph's test set for a committed change is `cargo nextest run $(north graph ask affected --nextest origin/main HEAD)`: it prints `-p` for each package and a nextest filter naming each traced test and each test the change adds, nothing when no test reaches the change (run none), and exits 3 when the change is untraced (a toolchain input such as flake.nix), where the changed crates' tests by name filter stand in; safe-push runs the same set before it queues the lane.
