#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORKFLOW="$ROOT/.github/workflows/ci.yml"
RELEASE_WORKFLOW="$ROOT/.github/workflows/release.yml"
DEADMAN_WORKFLOW="$ROOT/.github/workflows/ci-deadman.yml"

job_block() {
  local job="$1"
  local workflow="${2:-$WORKFLOW}"
  awk -v job="$job" '
    $0 == "  " job ":" { inside = 1; next }
    inside && /^  [A-Za-z0-9_-]+:$/ { exit }
    inside { print }
  ' "$workflow"
}

step_block() {
  local step="$1"
  local workflow="${2:-$WORKFLOW}"
  awk -v step="      - name: $step" '
    $0 == step { inside = 1; next }
    inside && /^      - name: / { exit }
    inside && /^  [A-Za-z0-9_-]+:$/ { exit }
    inside { print }
  ' "$workflow"
}

workflow_trigger_block() {
  local workflow="$1"
  awk '
    $0 == "on:" { inside = 1; next }
    inside && /^[A-Za-z0-9_-]+:/ { exit }
    inside { print }
  ' "$workflow"
}

dispatch_input_block() {
  local input="$1"
  local workflow="$2"
  awk -v input="      $input:" '
    $0 == input { inside = 1; next }
    inside && /^      [A-Za-z0-9_-]+:$/ { exit }
    inside && /^[^ ]/ { exit }
    inside { print }
  ' "$workflow"
}

lint_job="$(job_block lint)"
test_job="$(job_block test)"
package_job="$(job_block package-x86_64-linux)"
lock_step="$(step_block 'Lock and packaged-helper boundaries')"
release_trigger="$(workflow_trigger_block "$RELEASE_WORKFLOW")"
release_tag_input="$(dispatch_input_block release_tag "$RELEASE_WORKFLOW")"
candidate_commit_input="$(dispatch_input_block candidate_commit "$RELEASE_WORKFLOW")"
release_preflight_job="$(job_block preflight "$RELEASE_WORKFLOW")"
release_publish_job="$(job_block publish "$RELEASE_WORKFLOW")"
release_checkout_step="$(step_block 'Checkout exact candidate' "$RELEASE_WORKFLOW")"
release_authored_step="$(step_block 'Verify authored identity and notes' "$RELEASE_WORKFLOW")"
release_identity_step="$(step_block 'Resolve annotated candidate identity' "$RELEASE_WORKFLOW")"
release_green_step="$(step_block 'Verify exact green main gate' "$RELEASE_WORKFLOW")"
release_history_step="$(step_block 'Refuse gaps in public final release history' "$RELEASE_WORKFLOW")"

[[ -n "$lint_job" ]]
[[ -n "$test_job" ]]
[[ -n "$package_job" ]]
[[ -n "$lock_step" ]]
grep -Fq 'shellcheck --severity=warning' <<<"$lint_job"
grep -Fq 'nix flake check --all-systems --no-build' <<<"$test_job"
grep -Fq "'path:.#packages.x86_64-linux.default'" <<<"$test_job"
grep -Fq 'bash bin/north-release-preflight "v$(jq -r '\''.version'\'' sdk/package.json)"' <<<"$lock_step"
grep -Fxq '          bash bin/tests/north-release-preflight-test.sh' <<<"$lock_step"
grep -Fq 'beagle_repository="$(north/bin/github-flake-input-pin north/flake.lock beagle-engine-source repository)"' <<<"$test_job"
grep -Fq 'beagle_ref="$(north/bin/github-flake-input-pin north/flake.lock beagle-engine-source revision)"' <<<"$test_job"
grep -Fq 'echo "beagle_repository=$beagle_repository"' <<<"$test_job"
grep -Fq 'echo "beagle_ref=$beagle_ref"' <<<"$test_job"

[[ -n "$release_trigger" ]]
[[ -n "$release_tag_input" ]]
[[ -n "$candidate_commit_input" ]]
[[ -n "$release_preflight_job" ]]
[[ -n "$release_publish_job" ]]
[[ -n "$release_checkout_step" ]]
[[ -n "$release_authored_step" ]]
[[ -n "$release_identity_step" ]]
[[ -n "$release_green_step" ]]
[[ -n "$release_history_step" ]]
grep -Fxq '  push:' <<<"$release_trigger"
grep -Fxq '    tags:' <<<"$release_trigger"
grep -Fxq "      - 'v*.*.*'" <<<"$release_trigger"
grep -Fxq '  workflow_dispatch:' <<<"$release_trigger"
grep -Fxq '        required: true' <<<"$release_tag_input"
grep -Fxq '        type: string' <<<"$release_tag_input"
grep -Fxq '        required: true' <<<"$candidate_commit_input"
grep -Fxq '        type: string' <<<"$candidate_commit_input"
grep -Fxq "    if: github.repository == 'tompassarelli/north'" <<<"$release_preflight_job"
grep -Fxq '    needs: preflight' <<<"$release_publish_job"
grep -Fxq "    if: github.repository == 'tompassarelli/north' && github.event_name == 'push'" \
  <<<"$release_publish_job"
grep -Fxq '          fetch-depth: 0' <<<"$release_checkout_step"
# This is a literal workflow expression, not a shell expansion in this process.
# shellcheck disable=SC2016
grep -Fxq "          ref: \${{ github.event_name == 'workflow_dispatch' && inputs.candidate_commit || github.ref }}" \
  <<<"$release_checkout_step"
# shellcheck disable=SC2016
grep -Fxq "          RELEASE_TAG: \${{ github.event_name == 'workflow_dispatch' && inputs.release_tag || github.ref_name }}" \
  <<<"$release_authored_step"
grep -Fxq '        run: bash bin/north-release-preflight "$RELEASE_TAG"' \
  <<<"$release_authored_step"
# shellcheck disable=SC2016
grep -Fxq "          RELEASE_TAG: \${{ github.event_name == 'workflow_dispatch' && inputs.release_tag || github.ref_name }}" \
  <<<"$release_identity_step"
# shellcheck disable=SC2016
grep -Fxq "          CANDIDATE_COMMIT: \${{ github.event_name == 'workflow_dispatch' && inputs.candidate_commit || github.ref }}" \
  <<<"$release_identity_step"
grep -Fq 'candidate_sha="$(git rev-parse "$CANDIDATE_COMMIT^{commit}")"' \
  <<<"$release_identity_step"
grep -Fq 'test "$candidate_sha" = "$(git rev-parse HEAD^{commit})"' \
  <<<"$release_identity_step"
grep -Fq '| git mktag' <<<"$release_identity_step"
grep -Fq 'tagger North release preflight <release-preflight@north.invalid> 0 +0000' \
  <<<"$release_identity_step"
grep -Fq 'release_identity="refs/tags/$RELEASE_TAG"' <<<"$release_identity_step"
grep -Fq 'test "$(git cat-file -t "$release_identity")" = tag' <<<"$release_identity_step"
grep -Fq 'test "$(git rev-parse "$release_identity^{commit}")" = "$candidate_sha"' \
  <<<"$release_identity_step"
if grep -Eq '(^|[[:space:]])git[[:space:]]+(tag|update-ref|push)([[:space:]]|$)' \
  <<<"$release_identity_step"; then
  echo 'dispatch identity must stay an unreferenced git mktag object' >&2
  exit 1
fi
if grep -Eq '^        if:' \
  <<<"$release_identity_step$release_green_step$release_history_step"; then
  echo 'identity, exact-CI, and release-history gates must run for push and dispatch' >&2
  exit 1
fi
grep -Fq 'candidate_sha="$(git rev-parse "$RELEASE_IDENTITY^{commit}")"' \
  <<<"$release_green_step"
grep -Fq 'actions/workflows/ci.yml/runs?branch=main&event=push&status=success&head_sha=$candidate_sha&per_page=1' \
  <<<"$release_green_step"
grep -Fq 'candidate_sha="$(git rev-parse "$RELEASE_IDENTITY^{commit}")"' \
  <<<"$release_history_step"
grep -Fq 'repos/$GITHUB_REPOSITORY/releases?per_page=100' <<<"$release_history_step"
grep -Fq "done < <(git tag --list 'v*.*.*')" <<<"$release_history_step"

# The assignment form above fails closed only under errexit. Without this line
# the pin helper's non-zero exit is discarded, empty values reach
# $GITHUB_OUTPUT, and the job proceeds against an unpinned Beagle Store while staying
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
  cli/tests/agent-catalog-import-test.clj
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
grep -Fq 'BEAGLE_STORE_TEST_CHECKOUT: ${{ github.workspace }}/beagle/store' "$WORKFLOW"
grep -Fq 'NORTH_AGENT_RUNTIME_HOME=$GITHUB_WORKSPACE/north/agent-runtime/orchestration' "$WORKFLOW"
grep -Fq 'BEAGLE_STORE_HOME=$GITHUB_WORKSPACE/beagle/store' "$WORKFLOW"
grep -Fq 'BEAGLE_STORE_OUT=$GITHUB_WORKSPACE/beagle/store/out' "$WORKFLOW"
grep -Fq 'BABASHKA_CLASSPATH=$GITHUB_WORKSPACE/north/out:$GITHUB_WORKSPACE/beagle/store/out' "$WORKFLOW"
if grep -Eq '/home/tom/code/(store|beagle)' "$WORKFLOW"; then
  echo 'CI must bind bare bb to the exact-ref GitHub checkout, never a local engine path' >&2
  exit 1
fi
if grep -Fq 'ORCHESTRATION_HOME' "$WORKFLOW"; then
  echo 'CI must use the split portable-package and North-runtime roots' >&2
  exit 1
fi
# The patched executable's behavioral smoke must remain connected all the way
# from its reusable entrypoint to the x86_64 check and the release build job.
# shellcheck disable=SC2016
grep -Fq 'bash ${./bin/tests/codex-managed-hook-failure-smoke.sh}' "$ROOT/flake.nix"
grep -Fq 'codex-managed-hook-failure = codexManagedHookFailureSmoke;' "$ROOT/flake.nix"
grep -Fq "'path:.#checks.x86_64-linux.codex-managed-hook-failure'" "$WORKFLOW"
