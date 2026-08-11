#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/ci.yml"
DEADMAN_WORKFLOW="$ROOT/.github/workflows/ci-deadman.yml"

job_block() {
  local job="$1"
  awk -v job="$job" '
    $0 == "  " job ":" { inside = 1; next }
    inside && /^  [A-Za-z0-9_-]+:$/ { exit }
    inside { print }
  ' "$WORKFLOW"
}

lint_job="$(job_block lint)"
test_job="$(job_block test)"
package_job="$(job_block package-x86_64-linux)"

[[ -n "$lint_job" ]]
[[ -n "$test_job" ]]
[[ -n "$package_job" ]]
grep -Fq 'shellcheck --severity=warning' <<<"$lint_job"
grep -Fq 'nix flake check --all-systems --no-build' <<<"$test_job"
grep -Fq "'path:.#packages.x86_64-linux.default'" <<<"$test_job"
if grep -Eq '^    needs:' <<<"$lint_job"; then
  echo 'lint must remain an independent root job' >&2
  exit 1
fi
if grep -Eq '^    needs:' <<<"$test_job"; then
  echo 'test must remain an independent root job so lint cannot gate correctness' >&2
  exit 1
fi
grep -Fxq '    needs: test' <<<"$package_job"
if grep -Fq 'shellcheck' <<<"$test_job$package_job"; then
  echo 'shellcheck must remain isolated from correctness and package jobs' >&2
  exit 1
fi

[[ -f "$DEADMAN_WORKFLOW" ]]
grep -Fq -- "- cron: '37 */6 * * *'" "$DEADMAN_WORKFLOW"
grep -Fq 'actions: read' "$DEADMAN_WORKFLOW"
grep -Fq 'issues: write' "$DEADMAN_WORKFLOW"
grep -Fq "ALERT_AFTER_SECONDS: '172800'" "$DEADMAN_WORKFLOW"
grep -Fq 'actions/workflows/ci.yml/runs?branch=main&event=push&status=success&per_page=1' "$DEADMAN_WORKFLOW"
grep -Fq '::error title=CI dead-man switch::' "$DEADMAN_WORKFLOW"
grep -Fq "'{\"state\":\"closed\",\"state_reason\":\"completed\"}'" "$DEADMAN_WORKFLOW"

shell_bars=(
  bin/tests/north-on-spawn-stress-test.sh
  bin/tests/north-on-tooluse-stress-test.sh
  bin/tests/session-role-alias-test.sh
  bin/tests/north-mark-delegated-test.sh
  bin/tests/identity-alias-test.sh
  bin/tests/native-identity-test.sh
)
clojure_bars=(
  cli/tests/agent-identity-publication-integration-test.clj
  cli/tests/agents-cli-test.clj
  cli/tests/spawn-notify-listener-warning-test.clj
  cli/tests/json-show-indexed-test.clj
  cli/tests/json-children-indexed-test.clj
  cli/tests/dashboard-doctor-exit-test.clj
  cli/tests/wip-cli-test.clj
  cli/tests/live-feed-integration-test.clj
  cli/tests/message-routing-test.clj
  cli/tests/presence-online-integration-test.clj
  cli/tests/map-contract-test.clj
  cli/tests/message-audience-integration-test.clj
  cli/tests/native-listener-liveness-integration-test.clj
  cli/tests/north-listen-reconnect-test.clj
  cli/tests/pending-pagination-integration-test.clj
  cli/tests/pred-cli-test.clj
  cli/tests/spawn-process-integration-test.clj
  cli/tests/delegate-intake-e2e-test.clj
  cli/tests/peer-command-integration-test.clj
  cli/tests/worktree-allocation-integration-test.clj
  cli/tests/worktree-janitor-integration-test.clj
  cli/tests/maintenance-large-corpus-test.clj
)

for entrypoint in "${shell_bars[@]}"; do
  [[ -x "$ROOT/$entrypoint" ]]
  grep -Fq "bash $entrypoint" "$WORKFLOW"
done
for entrypoint in "${clojure_bars[@]}"; do
  [[ -f "$ROOT/$entrypoint" ]]
  grep -Fq "bb $entrypoint" "$WORKFLOW"
done

# These are literal workflow expressions, not shell expansions in this process.
# shellcheck disable=SC2016
grep -Fq 'FRAM_TEST_CHECKOUT: ${{ github.workspace }}/fram' "$WORKFLOW"
# shellcheck disable=SC2016
grep -Fq 'ORCHESTRATION_HOME: ${{ github.workspace }}/north/orchestration' "$WORKFLOW"
grep -Fq 'FRAM_HOME=$GITHUB_WORKSPACE/fram' "$WORKFLOW"
grep -Fq 'FRAM_OUT=$GITHUB_WORKSPACE/fram/out' "$WORKFLOW"
grep -Fq 'NORTH_FRAMRPC_OUT=$GITHUB_WORKSPACE/fram/out' "$WORKFLOW"
grep -Fq 'BABASHKA_CLASSPATH=$GITHUB_WORKSPACE/north/out:$GITHUB_WORKSPACE/fram/out' "$WORKFLOW"
if grep -Fq '/home/tom/code/fram' "$WORKFLOW"; then
  echo 'CI must bind bare bb to the exact-ref GitHub checkout, never a local Fram path' >&2
  exit 1
fi
# The patched executable's behavioral smoke must remain connected all the way
# from its reusable entrypoint to the x86_64 check and the release build job.
# shellcheck disable=SC2016
grep -Fq 'bash ${./bin/tests/codex-managed-hook-failure-smoke.sh}' "$ROOT/flake.nix"
grep -Fq 'codex-managed-hook-failure = codexManagedHookFailureSmoke;' "$ROOT/flake.nix"
grep -Fq "'path:.#checks.x86_64-linux.codex-managed-hook-failure'" "$WORKFLOW"
