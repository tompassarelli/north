# North agent guidance

North is the provider-neutral coordination substrate. Read
`docs/operating-manual.md` before nontrivial work. The fact graph is canonical;
`threads/` is a projection.

## Runtime boundaries

- Coordination, posture, clocks, telemetry, concerns, and supervision belong to North.
- Provider SDK/CLI code belongs only under `sdk/src/providers/`.
- Orchestration owns semantic task routing; provider adapters resolve semantic tiers to models.
- MCP is the shared data/tool boundary for interactive Claude Code and Codex sessions.
- Never add a provider model ID to provider-neutral orchestration code.

## Safe writes and verification

- Assume concurrent agents may be working in the same checkout.
- Use North concerns before editing overlapping code.
- Preserve unrelated dirty work.
- Run `cd ~/code/north/main/sdk && bun run check && bun run test` for SDK changes. The package
  script owns the hermetic preloads and test isolation; do not bypass it.
- A provider fallback is permitted only before side effects are observable.

Claude Code-specific compatibility remains documented in `CLAUDE.md`; this file
is the canonical cross-provider contract.
