# Where policy lives

Policy is the rules agents follow and the checks that hold them to it. Each kind has one home and one way to reach agents. The operator repository (`south` by default) holds the operator's private policy; [rationale/operator-repository.md](rationale/operator-repository.md) inventories what north reads from it and how a newcomer's repository takes its place.

- **Global agent rules:** `AGENTS.md` in the operator repository, each bullet registered in its `rules.toml`; published by `north config agents sync`.
- **Rules for `~/code`:** [harness/code/AGENTS.md](../harness/code/AGENTS.md); sync links it as `~/code/AGENTS.md`.
- **Rules for one repository:** that repository's `AGENTS.md`, such as [this one](../AGENTS.md), read in the checkout.
- **Procedures:** skills in [harness/skills/](../harness/skills/), linked by sync.
- **Guards on tool calls:** `north hook` verbs (`dispatch`, `language-guard`, `codex-conduct`, `firn-system-policy`, `prompt-grounding`), bound to events by [harness/policy-owners.toml](../harness/policy-owners.toml); `north agent-policy-contract --write` renders the Claude and Codex hook configs that run them.
- **Claude worker definitions and Codex prompts:** [harness/claude/agents/](../harness/claude/agents/) and [harness/codex/prompts/](../harness/codex/prompts/); `north check harness` reads the installed copies in `~/.claude/agents`.
- **Thresholds and priors:** [harness/defaults.toml](../harness/defaults.toml), each with its source, read by the commands that use them.
- **Models, prices and benchmarks:** [harness/model-catalog.toml](../harness/model-catalog.toml), read by routing. `north fleet usage --refresh` records `agy models` and reports any gemini key the catalog lacks or lists after agy retired it (`agents/agy-models-drift.txt` in the state directory).
- **Staffing tiers, accounts, project shares and targets:** `orchestration.toml` in the operator repository, read by `north config agents plan` and the spawn gate.
- **Recurring routines:** `routines/` in the operator repository, run by the routines timer.

A policy change lands like any other change ([landing.md](landing.md)). `north config agents sync` refuses to publish policy that fails its gate, including a measured drop in replay conformance against the operator repository's `operating-point.toml`; the "Policy regression gate" entry in [features/06-policy-measurement/](features/06-policy-measurement/) gives the rules.

Each hook names the runtime gap it fills and is deleted when Claude Code closes that gap; the audit of every installed Claude hook entry is [rationale/hooks-and-runtime.md](rationale/hooks-and-runtime.md). Landing is independent of provider or model: every fleet session uses the same `safe-push` checks.
