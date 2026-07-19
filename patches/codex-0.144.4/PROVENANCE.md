# Codex 0.144.4 downstream patch provenance

This bundle carries a North-specific downstream modification to OpenAI Codex. It is intentionally version-pinned and must not be applied to another Codex release without a fresh source review, regenerated schemas, and the full differential test bar below.

## Exact upstream

- Repository: https://github.com/openai/codex
- Release tag: `rust-v0.144.4`
- Annotated tag object: `632c07017ed17f00ca6d911b754683dee785af69`
- Peeled source commit: `8c68d4c87dc54d38861f5114e920c3de2efa5876`
- Nix source tree: `/nix/store/habmf96bdkai7d1k8kzq7rzxa79pjkfb-source`
- Source NAR hash: `sha256-NmYZxjNFPkRWN4rw+eeka10pJt6/oU3ZoLXBxj3dPRU=`
- Nix cargo hash: `sha256-S4dsZXfmKvJItL2XYKyxfhqdCMATEG6oPjrtVRwkuYc=`
- License: Apache-2.0
- Upstream `LICENSE` SHA-256: `d17f227e4df5da1600391338865ce0f3055211760a36688f816941d58232d8dc`
- Upstream `NOTICE` SHA-256: `9d71575ecfd9a843fc1677b0efb08053c6ba9fd686a0de1a6f5382fd3c220915`

`~/code/north/patches/codex-0.144.4/LICENSE.upstream` and `~/code/north/patches/codex-0.144.4/NOTICE.upstream` are byte-for-byte copies of those exact upstream files.

## North modification

The patch adds the administrator requirement `managed_hook_failure_mode = "continue" | "block"`.

- `continue` is the default and preserves Codex 0.144.4 behavior.
- `block` is accepted and effective only with `allow_managed_hooks_only = true`.
- The engine checks both predicates even for a directly constructed `ConfigLayerStack`; parser validation is not the sole authority boundary.
- In effective block mode, spawn, stdin, wait, timeout, missing-exit, nonzero-exit, malformed-output, and input-serialization failures from administrator-managed `PreToolUse` hooks deny tool dispatch before side effects.
- A technical failure produces one fixed, bounded administrator-policy denial. It does not echo the tool command, tool input, hook stderr, or other secret-bearing payload.
- A valid explicit exit-2 or JSON denial always wins over concurrent technical failures, regardless of configured order.
- Any successful hook input rewrite is suppressed when the aggregate decision blocks.
- `PostToolUse` is unchanged. It runs after tool side effects and this patch makes no rollback or fail-closed claim for it.
- App-server JSON and TypeScript expose the exact camel-case field `managedHookFailureMode`.

## Modified upstream files

Authored source and tests:

- `codex-rs/app-server-protocol/src/protocol/v2/config.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/tests.rs`
- `codex-rs/app-server-protocol/tests/schema_fixtures.rs`
- `codex-rs/app-server/src/request_processors/config_processor.rs`
- `codex-rs/config/src/config_requirements.rs`
- `codex-rs/config/src/requirements_layers/stack.rs`
- `codex-rs/config/src/requirements_layers/stack_tests.rs`
- `codex-rs/core/src/config/mod.rs`
- `codex-rs/core/src/hook_runtime.rs`
- `codex-rs/core/tests/suite/hooks.rs`
- `codex-rs/hooks/src/engine/mod.rs`
- `codex-rs/hooks/src/engine/mod_tests.rs`
- `codex-rs/hooks/src/events/pre_tool_use.rs`
- `codex-rs/hooks/src/lib.rs`
- `codex-rs/protocol/src/config_types.rs`

Regenerated artifacts:

- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`
- `codex-rs/app-server-protocol/schema/json/v2/ConfigRequirementsReadResponse.json`
- `codex-rs/app-server-protocol/schema/typescript/ManagedHookFailureMode.ts`
- `codex-rs/app-server-protocol/schema/typescript/index.ts`
- `codex-rs/app-server-protocol/schema/typescript/v2/ConfigRequirements.ts`

The generated artifacts cannot all carry source comments, so this manifest is their explicit modification notice. Authored Rust changes also carry a `Modified by North` notice.

## Application

Apply `~/code/north/patches/codex-0.144.4/managed-hook-failure-mode.patch` with `patch -p1` from the upstream `codex-rs` source root. North's Nix derivation asserts version `0.144.4`; a version mismatch must fail evaluation rather than attempt a fuzzy patch.

Patch SHA-256: `36e07d12702e31bffb82fbfe577a6f22c81424f1510a78ea3a2add9ca0879bc3`

No `Cargo.lock` change is included. The final patched lock was byte-compared with the exact upstream lock.

## Verification evidence

All current-source commands used the exact upstream source above plus this patch.

- `cargo test -p codex-hooks` → 127 passed, 0 failed.
- `cargo test -p codex-config managed_hook_failure_mode` → 3 passed, 0 failed.
- `cargo test -p codex-config block_failure_mode_requires_managed_hooks_only` → 1 passed, 0 failed.
- `cargo test -p codex-app-server-protocol --test schema_fixtures` → 3 passed, 0 failed; JSON and TypeScript regeneration parity and the exact `PreToolUse`/`PostToolUse` documentation boundary passed.
- `cargo test -p codex-app-server requirements_api_exposes_managed_hook_failure_mode_in_camel_case` → 1 passed, 0 failed.
- `RUST_MIN_STACK=33554432 cargo test -p codex-core --test all managed_pre_tool_use_technical_failure_blocks_before_execution -- --nocapture` → 1 passed, 0 failed.
- Independent fresh patched run at upstream CI's `RUST_MIN_STACK=8388608` → the same core marker test passed 1/1.
- Exact pristine 0.144.4 with only the marker test transplanted, at `RUST_MIN_STACK=8388608` → 0 passed, 1 failed. A preserved red-only marker existed at 108 bytes with SHA-256 `a7f491b245ff028e45fa443116effb566df95d50c56a5f15eba5d6820e8ec1b2`, proving pristine Codex executed the guarded command after the hook failed.
- `cargo fmt --all -- --check` → exit 0.
- `git diff --check` → exit 0.
- Byte comparison of patched and exact-upstream `Cargo.lock` → identical.

The core marker additionally asserts that the managed hook ran, the guarded tool marker stayed absent, the fixed policy reason was returned, and a long sentinel secret plus the full command were absent from the returned failure.
