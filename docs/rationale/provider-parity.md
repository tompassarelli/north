# Provider parity: one owner for every guarantee only a Claude hook holds

North#1041, 2026-11-10. A guarantee holds for every model provider only when it lives in a layer every provider passes through: the launch, the worker sandbox and its outside broker, safe-push, the lander, and the GitHub App's permissions. A provider hook may only give earlier feedback on it. harness/policy-owners.toml `guard[]` binds hooks for Claude and Codex alone, so each guarantee a Claude hook enforces that no shared layer holds needs an issue that moves it there.

The full list, guarantee by guarantee, names what a sandboxed worker of another provider can still do unrefused. North's privacy gate keeps that out of this public repository, so it lives in south (agents/decisions/harness/provider-parity-2026-11-10.md). This page keeps one row per guard unit and the issues that own its gaps.

| Guard unit | Hook-only guarantees | Owner |
| --- | --- | --- |
| language-guard | None: safe-push's language gate holds it at landing. | none needed |
| codex-conduct | None: it binds Codex sessions alone. | none needed |
| firn-system-policy | Its edit half. | north#1060 |
| prompt-grounding | None for a fleet worker, which the launch grounds; a coordinator in a Pi window, whose prompts no bridge hands to north yet. | north#1041 |
| north-hook | Its edit deciders for a worker, GitHub-write deciders, waiting rule and session-conduct deciders. Its command deciders refuse in `north --worker-sandbox` for every OpenCode and Pi worker command (crates/north-runtime/src/worker_sandbox/mod.rs), and a Pi window hands each bash, edit and write call to `north hook` through its north-fleet.ts bridge. | edit deciders: north#1060; GitHub writes: north#1056 and north#1037; waiting: north#1041, box "The waiting rule lives in the shared layer for every provider, Pi included" |

Advice-only classes refuse nothing, so they carry no guarantee to move.
