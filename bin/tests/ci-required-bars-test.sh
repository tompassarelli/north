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

step_block() {
  local step="$1"
  awk -v step="      - name: $step" '
    $0 == step { inside = 1; next }
    inside && /^      - name: / { exit }
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
grep -Fq 'beagle_repository="$(north/bin/github-flake-input-pin north/flake.lock beagle-engine-source repository)"' <<<"$test_job"
grep -Fq 'beagle_ref="$(north/bin/github-flake-input-pin north/flake.lock beagle-engine-source revision)"' <<<"$test_job"
grep -Fq 'echo "beagle_repository=$beagle_repository"' <<<"$test_job"
grep -Fq 'echo "beagle_ref=$beagle_ref"' <<<"$test_job"

# The assignment form above fails closed only under errexit. Without this line
# the pin helper's non-zero exit is discarded, empty values reach
# $GITHUB_OUTPUT, and the job proceeds against an unpinned Fram while staying
# green. Assert it inside this step, not merely somewhere in the job.
resolve_step="$(step_block 'Resolve locked source inputs')"
[[ -n "$resolve_step" ]]
if ! grep -Fq 'set -euo pipefail' <<<"$resolve_step"; then
  echo 'the locked-source-input step must keep set -euo pipefail' >&2
  exit 1
fi
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
grep -Fq 'FRAM_TEST_CHECKOUT: ${{ github.workspace }}/beagle/branch-core' "$WORKFLOW"
# shellcheck disable=SC2016
grep -Fq 'ORCHESTRATION_HOME: ${{ github.workspace }}/north/orchestration' "$WORKFLOW"
grep -Fq 'FRAM_HOME=$GITHUB_WORKSPACE/beagle/branch-core' "$WORKFLOW"
grep -Fq 'FRAM_OUT=$GITHUB_WORKSPACE/beagle/branch-core/out' "$WORKFLOW"
grep -Fq 'NORTH_FRAMRPC_OUT=$GITHUB_WORKSPACE/beagle/branch-core/out' "$WORKFLOW"
grep -Fq 'BABASHKA_CLASSPATH=$GITHUB_WORKSPACE/north/out:$GITHUB_WORKSPACE/beagle/branch-core/out' "$WORKFLOW"
if grep -Eq '/home/tom/code/(fram|beagle)' "$WORKFLOW"; then
  echo 'CI must bind bare bb to the exact-ref GitHub checkout, never a local engine path' >&2
  exit 1
fi
# The patched executable's behavioral smoke must remain connected all the way
# from its reusable entrypoint to the x86_64 check and the release build job.
# shellcheck disable=SC2016
grep -Fq 'bash ${./bin/tests/codex-managed-hook-failure-smoke.sh}' "$ROOT/flake.nix"
grep -Fq 'codex-managed-hook-failure = codexManagedHookFailureSmoke;' "$ROOT/flake.nix"
grep -Fq "'path:.#checks.x86_64-linux.codex-managed-hook-failure'" "$WORKFLOW"
