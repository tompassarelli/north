#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
PREFLIGHT="$ROOT/bin/north-release-preflight"
SCRATCH="$(mktemp -d)"
trap 'rm -rf "${SCRATCH:?}"' EXIT

fixture_number=0
fixture_root=""
passed=0
failed=0

new_fixture() {
  fixture_number=$((fixture_number + 1))
  fixture_root="$SCRATCH/fixture-$fixture_number"
  mkdir -p \
    "$fixture_root/bin" \
    "$fixture_root/sdk" \
    "$fixture_root/.github/release-notes"
  cp "$PREFLIGHT" "$fixture_root/bin/north-release-preflight"
  cp "$ROOT/sdk/package.json" "$fixture_root/sdk/package.json"
  cp "$ROOT/flake.nix" "$fixture_root/flake.nix"
  cp "$ROOT/.github/release-notes/v0.1.0.md" \
    "$fixture_root/.github/release-notes/v0.1.0.md"
}

assert_refusal() {
  local tag="$1"
  local expected_status="$2"
  local expected_diagnostic="$3"
  local output
  local status

  set +e
  output="$(bash "$fixture_root/bin/north-release-preflight" "$tag" 2>&1)"
  status=$?
  set -e

  if [[ "$status" -ne "$expected_status" ]]; then
    printf 'expected status %s for %s, got %s\n%s\n' \
      "$expected_status" "$tag" "$status" "$output" >&2
    return 1
  fi
  if ! grep -Fq "$expected_diagnostic" <<<"$output"; then
    printf 'missing diagnostic for %s: %s\n%s\n' \
      "$tag" "$expected_diagnostic" "$output" >&2
    return 1
  fi
}

positive_case() {
  local output
  new_fixture
  output="$(bash "$fixture_root/bin/north-release-preflight" v0.1.0)"
  grep -Fxq 'north release preflight: PASS (v0.1.0)' <<<"$output"
}

semver_refusal_case() {
  new_fixture
  assert_refusal \
    v0.1 \
    2 \
    'north-release-preflight: release tag must be v-prefixed final SemVer' || return 1
  new_fixture
  assert_refusal \
    v01.2.3 \
    2 \
    'north-release-preflight: release tag must be v-prefixed final SemVer'
}

package_version_refusal_case() {
  new_fixture
  printf '%s\n' '{"version":"0.1.1"}' > "$fixture_root/sdk/package.json"
  assert_refusal \
    v0.1.0 \
    1 \
    'north-release-preflight: tag v0.1.0 disagrees with sdk/package.json 0.1.1'
}

north_version_source_refusal_case() {
  new_fixture
  sed -i \
    's|^        northVersion = (builtins.fromJSON (builtins.readFile ./sdk/package.json)).version;$|        northVersion = "0.1.0";|' \
    "$fixture_root/flake.nix"
  printf '%s\n' \
    '# northVersion = (builtins.fromJSON (builtins.readFile ./sdk/package.json)).version;' \
    >> "$fixture_root/flake.nix"
  grep -Fxq '        northVersion = "0.1.0";' "$fixture_root/flake.nix"
  assert_refusal \
    v0.1.0 \
    1 \
    'north-release-preflight: flake.nix must derive northVersion from sdk/package.json'
}

north_package_version_refusal_case() {
  new_fixture
  sed -i \
    's|^          version = northVersion;$|          version = "0.1.0";|' \
    "$fixture_root/flake.nix"
  printf '%s\n' '# version = northVersion;' >> "$fixture_root/flake.nix"
  grep -Fxq '          version = "0.1.0";' "$fixture_root/flake.nix"
  assert_refusal \
    v0.1.0 \
    1 \
    'north-release-preflight: flake.nix northPkg must use northVersion'
}

notes_missing_or_empty_refusal_case() {
  new_fixture
  rm -f "${fixture_root:?}/.github/release-notes/v0.1.0.md"
  assert_refusal \
    v0.1.0 \
    1 \
    'north-release-preflight: missing authored release notes' || return 1
  new_fixture
  : > "$fixture_root/.github/release-notes/v0.1.0.md"
  assert_refusal \
    v0.1.0 \
    1 \
    'north-release-preflight: missing authored release notes'
}

notes_title_refusal_case() {
  new_fixture
  sed -i '1c# North v0.1.1' \
    "$fixture_root/.github/release-notes/v0.1.0.md"
  assert_refusal \
    v0.1.0 \
    1 \
    'must start with # North v0.1.0'
}

notes_sections_refusal_case() {
  new_fixture
  sed -i '/^## Highlights$/d' \
    "$fixture_root/.github/release-notes/v0.1.0.md"
  assert_refusal \
    v0.1.0 \
    1 \
    'must contain a Highlights or Changes section'
}

run_case() {
  local name="$1"
  local case_function="$2"

  if "$case_function"; then
    printf 'PASS: %s\n' "$name"
    passed=$((passed + 1))
  else
    printf 'FAIL: %s\n' "$name" >&2
    failed=$((failed + 1))
  fi
}

run_case positive positive_case
run_case invalid-final-semver semver_refusal_case
run_case package-version-mismatch package_version_refusal_case
run_case flake-source-binding north_version_source_refusal_case
run_case north-package-version-binding north_package_version_refusal_case
run_case missing-or-empty-notes notes_missing_or_empty_refusal_case
run_case wrong-notes-title notes_title_refusal_case
run_case missing-notes-section notes_sections_refusal_case

if (( failed > 0 )); then
  printf 'north release preflight fixture: %s / %s PASS\n' \
    "$passed" "$((passed + failed))" >&2
  exit 1
fi
printf 'north release preflight fixture: %s / %s PASS\n' \
  "$passed" "$((passed + failed))"
