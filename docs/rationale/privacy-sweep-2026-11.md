# Privacy sweep, November 2026

North's own privacy and secret scans were run over its whole history and its tree on 2026-11-13, before north is shown more widely (north#1281, box "Privacy sweep"). The secret scan finds nothing. The deny-list scan finds private values in old commits and in earlier versions of files, and the files as they stand hold none of them. Nothing here rewrites history: the owner's recorded answer to the 2026-11-09 GitHub audit keeps north's history unrewritten, so each finding below is listed for the owner to decide on.

Owner: north#1281

## What ran

All scans ran from a checkout at origin/main b19e39f7fe, which `git rev-list origin/main` counts as 7659 commits (`git rev-list --all` counts 7893; the other 234 are local lane, offload and rescue refs).

| Scan | Command | Over | Result |
| --- | --- | --- | --- |
| Secrets | `north land scan secrets --all` | every commit `git rev-list --all` prints | exit 0, 0 findings |
| Deny-list | `north land scan denylist "" origin/main main` | every commit and every file version of origin/main | exit 1: 4297 commit findings, 183 file findings, 41 private-file hits |
| Deny-list, tree as it stands | the same command over one commit holding origin/main's tree | the files as they stand | 0 deny-list findings, 7 private-file hits |
| Deny-list, other refs | the same command from origin/main to one commit merging every other ref | the 232 commits only local refs hold | 0 deny-list findings; the private-text judge could not run (exit 75, no verdict) |
| Author emails | `north land scan commit-email --all` | every commit | exit 1 |
| Private files | `north land scan private-file --range origin/main` | every commit | exit 1, 47 hits |

The private-text judge, which asks a model, was not run over the whole history: it costs subscription usage per range and its one run here gave no verdict. The rules below are the deny-list's reading; the rule that names a mailbox provider is shown as `private-mail-credit`, because its true name trips the scan on this page.

## Deny-list findings

One row per rule. "Commits" counts commits of origin/main whose commit object (author, committer or message) matches; "files" counts paths one of whose versions in history matches, and the tree as it stands matches none. No value is printed here.

| Rule | Commits | Files | Commit dates |
| --- | ---: | ---: | --- |
| `private-account-login-map` | 0 | 3 | - |
| `private-anthropic-ids` | 368 | 3 | 2026-06-27 to 2026-10-22 |
| `private-artifact-url` | 0 | 2 | - |
| `private-bnet-account-map` | 0 | 8 | - |
| `private-client-name` | 1 | 45 | 2026-07-25 to 2026-07-25 |
| `private-client-project` | 3 | 52 | 2026-07-22 to 2026-07-25 |
| `private-cloudflare-email` | 1940 | 0 | 2026-06-17 to 2026-10-15 |
| `private-hive-user` | 0 | 1 | - |
| `private-host-gex45` | 0 | 3 | - |
| `private-host-ip-wireguard` | 0 | 1 | - |
| `private-host-nexus` | 0 | 10 | - |
| `private-local-repo` | 0 | 7 | - |
| `private-operator-phone` | 2 | 2 | 2026-10-15 to 2026-10-15 |
| `private-personal-email` | 1940 | 0 | 2026-06-17 to 2026-10-15 |
| `private-plan-price` | 0 | 6 | - |
| `private-mail-credit` | 3 | 11 | 2026-10-12 to 2026-10-15 |
| `private-routine-id` | 0 | 3 | - |
| `private-session-id` | 40 | 8 | 2026-10-13 to 2026-10-16 |
| `private-timezone` | 0 | 3 | - |
| `private-tz-offset` | 0 | 15 | - |

## Commit findings, small classes

These are the commits for each rule with fewer than 100 commits, by their first 12 characters.

- `private-client-name` (1): 419330dc5f61
- `private-client-project` (3): 419330dc5f61 93253e461e09 fa06b4a03d57
- `private-operator-phone` (2): 387159ac3926 66e1835551b0
- `private-mail-credit` (3): 387159ac3926 66e1835551b0 ecebf97a5466
- `private-session-id` (40): 024b12cb57e9 0386ba6ca0fd 0864dfe3072e 08a16d5a48ea 0c0ee13afc49 2944a36f8824 447c3b0a92f7 4725af32b43c 4e871021074a 52ace3b5ef55 60d42734c6aa 64b40df7bd21 6bd80a6b896b 6ff7c194d779 7152736a9f86 71eead2a1e15 72f9eb68f17f 7b43fa4abb9f 7e96a2888c1c 8654888ab344 9263e280c8bc 9402483be1ea 9eeb047c4197 a0271e6840fa a4247b3f2bef a8656cf70ec0 b18930b63c33 ba2970f50c3e c1d9b729d2a6 c3252c5ae908 c4d05358e3a1 c5528f1666b7 d0998be73eb0 d400b5a3376c de27467a1491 ded48f199e55 e92a994ba86b f9ca4a876aea facfc52647ba fda4608fb3c7

The two large classes are listed by the command that prints them: `private-personal-email` (the owner's personal mailbox address as author or committer, which `git log --format='%h %ae %ce' origin/main` shows) and `private-anthropic-ids` (an Anthropic environment or session identifier in a commit message, which the 2026-11-13 refusal of north#1281 box "Operator 18" stops adding). `private-cloudflare-email` matches the same 1940 commits as `private-personal-email`.

## File findings

Each path is listed once per rule. A path marked `now` still exists in origin/main; the scan matched one of its earlier versions, and its current content is clean.

- `private-account-login-map`:
  - `harness/skills/cloud-workers/SKILL.md`
  - `harness/skills/workers/references/cloud.md` (now)
  - `socrates/skills/cloud-workers/SKILL.md`
- `private-anthropic-ids`:
  - `harness/skills/cloud-workers/references/remote-trigger.md`
  - `harness/skills/workers/references/cloud.md` (now)
  - `socrates/skills/cloud-workers/references/remote-trigger.md`
- `private-artifact-url`:
  - `harness/skills/iteration-economics/SKILL.md`
  - `harness/skills/workers/references/payback.md` (now)
- `private-bnet-account-map`:
  - `harness/lib/usage.py`
  - `harness/skills/warcraft-modding/SKILL.md` (now)
  - `harness/skills/warcraft-modding/references/accounts-and-login.md`
  - `harness/skills/warcraft-modding/references/lan-pair-batching.md` (now)
  - `socrates/lib/usage.py`
  - `socrates/skills/warcraft-modding/SKILL.md`
  - `socrates/skills/warcraft-modding/references/accounts-and-login.md`
  - `socrates/skills/warcraft-modding/references/lan-pair-batching.md`
- `private-client-name`:
  - `bin/north-graph`
  - `bin/north-timelog`
  - `bin/tern-timelog`
  - `bin/tests/north-arm-assign-test.sh`
  - `cli/clock-audit.clj`
  - `cli/north-invoice.clj`
  - `cli/north-timelog.clj`
  - `cli/tern-invoice.clj`
  - `cli/tern-timelog.clj`
  - `cli/tests/client-clock-authority-integration-test.sh`
  - `cli/tests/client-clock-bridge-admission-test.clj`
  - `cli/tests/linear-reservation-integration-test.clj`
  - `clock_test.clj`
  - `docs/harness-architecture.md`
  - `docs/linear-sync.md`
  - `docs/operating-manual.md`
  - `observatory/web/board-eddy-dist.css`
  - `observatory/web/board-wake-dist.css`
  - `observatory/web/eddy-observatory.css`
  - `observatory/web/schema-editor.css`
  - `observatory/web/wake-observatory.css`
  - `profiles/tom/AGENTS.md`
  - `profiles/tom/hooks/launch-critical-worktree-guard.test.py`
  - `profiles/tom/hooks/launch-critical-worktree-guard.test.sh`
  - `profiles/tom/hooks/north-clock-guard.py`
  - `profiles/tom/hooks/north-clock-guard.test.sh`
  - `realize-the-model.sh`
  - `scripts/migrations/realize-the-model.sh`
  - `sdk/src/clock.ts`
  - `sdk/src/integrations/linear/types.ts`
  - `sdk/test/clock.test.ts`
  - `sdk/test/completion-outcome.test.ts`
  - `sdk/test/guard-log.test.ts`
  - `sdk/test/linear-cli.test.ts`
  - `sdk/test/linear-mcp-broker.test.ts`
  - `sdk/test/linear-mcp-facade.test.ts`
  - `sdk/test/linear-sync.test.ts`
  - `streams/distillations/2026-07-02-crossover-session.tier1.md`
  - `tests/clock_test.clj`
  - `web-v1-archive/web/board-wake-dist.css`
  - `web-v1-archive/web/schema-editor.css`
  - `web-v1-archive/web/wake.css`
  - `web/web/board-wake-dist.css`
  - `web/web/schema-editor.css`
  - `web/web/wake.css`
- `private-client-project`:
  - `bin/north-graph`
  - `bin/north-timelog`
  - `bin/tern-timelog`
  - `bin/tests/north-arm-assign-test.sh`
  - `cli/clock-audit.clj`
  - `cli/north-invoice.clj`
  - `cli/north-timelog.clj`
  - `cli/tern-invoice.clj`
  - `cli/tern-timelog.clj`
  - `cli/tests/client-clock-authority-integration-test.sh`
  - `cli/tests/client-clock-bridge-admission-test.clj`
  - `cli/tests/dashboard-concern-projection-test.clj`
  - `cli/tests/linear-reservation-integration-test.clj`
  - `clock_test.clj`
  - `docs/harness-architecture.md`
  - `docs/linear-sync.md`
  - `docs/operating-manual.md`
  - `observatory/web/board-eddy-dist.css`
  - `observatory/web/board-wake-dist.css`
  - `observatory/web/eddy-observatory.css`
  - `observatory/web/schema-editor.css`
  - `observatory/web/wake-observatory.css`
  - `profiles/tom/AGENTS.md`
  - `profiles/tom/hooks/launch-critical-worktree-guard.test.py`
  - `profiles/tom/hooks/launch-critical-worktree-guard.test.sh`
  - `profiles/tom/hooks/north-clock-guard.py`
  - `profiles/tom/hooks/north-clock-guard.test.sh`
  - `profiles/tom/hooks/tripwire-guard.sh`
  - `profiles/tom/hooks/tripwire-guard.test.sh`
  - `realize-the-model.sh`
  - `scripts/migrations/realize-the-model.sh`
  - `scripts/migrations/telemetry-alias-migrate.clj`
  - `sdk/src/clock.ts`
  - `sdk/src/integrations/linear/types.ts`
  - `sdk/src/worktree.ts`
  - `sdk/test/clock.test.ts`
  - `sdk/test/completion-outcome.test.ts`
  - `sdk/test/guard-log.test.ts`
  - `sdk/test/linear-cli.test.ts`
  - `sdk/test/linear-mcp-broker.test.ts`
  - `sdk/test/linear-mcp-facade.test.ts`
  - `sdk/test/linear-sync.test.ts`
  - `sdk/test/worktree.test.ts`
  - `streams/distillations/2026-07-02-crossover-session.tier1.md`
  - `telemetry-alias-migrate.clj`
  - `tests/clock_test.clj`
  - `web-v1-archive/web/board-wake-dist.css`
  - `web-v1-archive/web/schema-editor.css`
  - `web-v1-archive/web/wake.css`
  - `web/web/board-wake-dist.css`
  - `web/web/schema-editor.css`
  - `web/web/wake.css`
- `private-hive-user`:
  - `harness/skills/warcraft-modding/references/hive-workshop.md` (now)
- `private-host-gex45`:
  - `harness/defaults.toml` (now)
  - `harness/skills/workers/SKILL.md` (now)
  - `harness/skills/workers/references/notes.md` (now)
- `private-host-ip-wireguard`:
  - `profiles/tom/hooks/tripwire-guard.test.sh`
- `private-host-nexus`:
  - `harness/defaults.toml` (now)
  - `harness/skills/cloud-workers/SKILL.md`
  - `harness/skills/cloud-workers/scripts/cloud-sessions.sh`
  - `harness/skills/machine-capacity/SKILL.md` (now)
  - `harness/skills/machine-capacity/references/admission-details.md` (now)
  - `harness/skills/machine-capacity/scripts/capacity-clamp.recorded.test.ts`
  - `harness/skills/machine-capacity/scripts/machine-capacity.mjs`
  - `harness/skills/workers/SKILL.md` (now)
  - `harness/skills/workers/references/cloud.md` (now)
  - `harness/skills/workers/scripts/cloud-sessions.sh`
- `private-local-repo`:
  - `agent-machinery/skills/verification/references/testing-sources.md`
  - `bin/test-health`
  - `harness/lib/test-health-core.ts`
  - `harness/skills/machine-capacity/references/testing-on-this-machine.md` (now)
  - `harness/skills/testing/references/sources.md`
  - `socrates/lib/test-health-core.ts`
  - `socrates/skills/testing/references/sources.md`
- `private-operator-phone`:
  - `harness/skills/cloud-workers/SKILL.md`
  - `harness/skills/workers/references/cloud.md` (now)
- `private-plan-price`:
  - `harness/skills/cloud-workers/SKILL.md`
  - `harness/skills/gemini-review/SKILL.md`
  - `harness/skills/workers/references/cloud.md` (now)
  - `harness/skills/workers/references/gemini-review.md`
  - `socrates/skills/cloud-workers/SKILL.md`
  - `socrates/skills/gemini-review/SKILL.md`
- `private-mail-credit`:
  - `bin/safe-push.test.sh`
  - `docs/features.md`
  - `docs/harness.md`
  - `harness/lib/placement.test.sh`
  - `harness/lib/usage.test.sh`
  - `harness/skills/cloud-workers/SKILL.md`
  - `harness/skills/workers/SKILL.md` (now)
  - `harness/skills/workers/references/cloud.md` (now)
  - `socrates/lib/usage.test.sh`
  - `socrates/skills/cloud-workers/SKILL.md`
  - `tests/placement_gate.rs`
- `private-routine-id`:
  - `harness/skills/cloud-workers/references/remote-trigger.md`
  - `harness/skills/workers/references/cloud.md` (now)
  - `socrates/skills/cloud-workers/references/remote-trigger.md`
- `private-session-id`:
  - `bin/agents.test.sh`
  - `bin/worker-ledger.test.sh`
  - `harness/skills/todo/references/records-and-dependencies.md` (now)
  - `harness/skills/warcraft-modding/references/warcraft-startup.md` (now)
  - `profiles/tom/hooks/corpus-scan-guard.test.sh`
  - `socrates/hooks/corpus-scan-guard.test.sh`
  - `socrates/skills/todo/references/records-and-dependencies.md`
  - `socrates/skills/warcraft-modding/references/warcraft-startup.md`
- `private-timezone`:
  - `cli/agents-cli.clj`
  - `sdk/src/fable-window.ts`
  - `sdk/test/linear-cli.test.ts`
- `private-tz-offset`:
  - `bin/capacity-watchdog`
  - `bin/laptop.test.sh`
  - `bin/todo-mailbox-rotate.test.sh`
  - `bin/todo-mailbox-status.test.sh`
  - `cli/tests/runtime-attestation-test.clj`
  - `harness/lib/fixtures/status-excerpts-2026-10-12.json`
  - `harness/lib/paperclip-core.test.ts`
  - `harness/lib/tick_precheck.test.py`
  - `harness/scripts/defaults-measure.test.py`
  - `harness/skills/todo/references/attempts-and-observations.md` (now)
  - `harness/skills/todo/references/records-and-dependencies.md` (now)
  - `socrates/lib/fixtures/status-excerpts-2026-10-12.json`
  - `socrates/lib/tick_precheck.test.py`
  - `socrates/skills/todo/references/attempts-and-observations.md`
  - `socrates/skills/todo/references/records-and-dependencies.md`

## Private-file hits

`north land scan private-file` lists a path whose bytes equal a file in the operator repository's history. Seven are in the tree as it stands: `crates/north-flow/src/flow/usage_yield/tests.rs`, `crates/north-health/src/health/failed_starts_tests.rs`, `crates/north-lander/tests/it/hold_alarm.rs`, `crates/north-providers/src/ledger/outcome/replay_tests.rs`, `evals/b/ladder`, `examples/operator/.north-operator-schema` and `src/cli/operator/template/schema-version`. The last two are the operator template's own marker files, which hold the same bytes as the owner's checkout by design. The rest of the 47 hits are earlier versions:
- `crates/north-core/src/fleet/standing-orders.md`
- `crates/north-dispatch/src/dispatch/cells_tests.rs`
- `crates/north-dispatch/src/dispatch/claim_tests.rs`
- `crates/north-dispatch/src/dispatch/deepseek/tests.rs`
- `crates/north-dispatch/src/dispatch/freeze_tests.rs`
- `crates/north-dispatch/src/dispatch/grounds_tests.rs`
- `crates/north-dispatch/src/dispatch/host_needs_tests.rs`
- `crates/north-dispatch/src/dispatch/order_tests.rs`
- `crates/north-dispatch/src/dispatch/stops_tests.rs`
- `crates/north-dispatch/src/dispatch/tests.rs`
- `crates/north-dispatch/src/dispatch/tier_tests.rs`
- `crates/north-dispatch/src/dispatch/tries_tests.rs`
- `crates/north-fleet/src/fleet/standing-orders.md`
- `crates/north-lander/src/land/build_lag_tests.rs`
- `crates/north-pace/src/credit/marks/tests.rs`
- `crates/north-pace/src/credit/readings/tests.rs`
- `crates/north-pace/src/spend/tests.rs`
- `crates/north-pace/src/usage/credit_table/tests.rs`
- `crates/north-pacing/src/pacing/breaker_tests.rs`
- `crates/north-pacing/src/pacing/governor_tests.rs`
- `crates/north-providers/src/ledger/tests.rs`
- `crates/north-providers/src/providers/claude/api_pool_tests.rs`
- `crates/north-providers/src/providers/claude/api_probe/tests.rs`
- `crates/north-providers/src/providers/claude/fleet_registry_tests.rs`
- `crates/north-providers/src/providers/claude/launch_tests.rs`
- `crates/north-providers/src/providers/claude/runs/tests.rs`
- `crates/north-providers/src/providers/opencode/tests.rs`
- `crates/north-providers/src/providers/pi/tests.rs`
- `crates/north-providers/tests/opencode_lane.rs`
- `crates/north-threads/src/threads/runs/tests.rs`
- `crates/north-tune/src/tune/tests.rs`
- `harness/skills/warcraft-modding/references/accounts-and-login.md`
- `socrates/skills/warcraft-modding/references/accounts-and-login.md`
- `src/fleet/standing-orders.md`

## Author emails

`git log --all --format='%ae %ce'` shows the owner's personal mailbox address as author or committer on 1940 commits of origin/main, the GitHub noreply address on the rest apart from agent and bot addresses, and a placeholder address on one scratch commit. The landing's commit-email check refuses a new commit with any other address, so none is added.

## Other places

- `/home/tom` appears in 68 tracked files of origin/main, mostly test fixtures. It is a username and a path, not on the deny-list, so no scan counts it a finding.

## What the owner decides

Each of the three below is a question about history or about the owner's own files; the tree as it stands needs no change.

1. The commits and old file versions above stay in history unless the owner rewrites it. Recommended: leave history as it is and publish north from origin/main, as the 2026-11-09 answer decided, because a rewrite breaks every lane, ref and recorded SHA, and the scan shows the files as they stand are clean. The alternative, publishing a fresh single-commit history of the tree, drops every finding and every SHA the issues cite.
2. The deny-list rules for the personal address and the account identifiers now stop new ones at landing, so no further action keeps the count from growing.
3. The private-file hits in the tree as it stands are Rust test files and the template's marker files; name them in the operator repository's `agents/approved-moves.toml` if the owner wants the landing's scan quiet, or leave them as they are.
