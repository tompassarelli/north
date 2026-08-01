#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "${fixture:?}"' EXIT

home="$fixture/home"
manifest="$fixture/manifest.env"
selected="$fixture/selected-fram"
explicit="$fixture/explicit-fram"
explicit_bin="$fixture/explicit-bin"
explicit_out="$fixture/explicit-out"
compat="$home/code/fram/wt-graph-authoring-repair-codex"

make_bin() {
  local bin="$1" identity="$2"
  mkdir -p "$bin"
  cat >"$bin/fram" <<EOF
#!/usr/bin/env bash
if [ -n "\${FRAM_TEST_ENV_LOG:-}" ]; then
  printf '%s|%s|%s\n' "\${FRAM_HOME:-}" "\${FRAM_BIN:-}" "\${FRAM_OUT:-}" >"\$FRAM_TEST_ENV_LOG"
fi
printf '%s\n' '$identity'
EOF
  chmod +x "$bin/fram"
}

make_fram() {
  make_bin "$1/bin" "$2"
}

make_fram "$selected" selected
make_fram "$explicit" explicit-home
make_bin "$explicit_bin" explicit-bin

printf 'WORLD_REPO_FRAM=%q\n' "$selected" >"$manifest"

actual="$(
  env -u FRAM_HOME -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_BIN="$explicit_bin" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$ROOT/bin/north" world-manifest-probe
)"
if [ "$actual" != explicit-bin ]; then
  printf 'FAIL temporary compatibility default probe\n  expected: explicit-bin\n  actual:   %s\n' "$actual" >&2
  exit 1
fi
expected_env="$compat|$explicit_bin|$compat/out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL temporary compatibility defaults\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

actual="$(
  env -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_HOME="$explicit" \
    FRAM_BIN="$explicit_bin" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$ROOT/bin/north" world-manifest-probe
)"
expected_env="$explicit|$explicit_bin|$compat/out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual" != explicit-bin ] || [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL independent FRAM_HOME precedence\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

actual="$(
  env -u FRAM_HOME -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_BIN="$explicit_bin" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$ROOT/bin/north" world-manifest-probe
)"
expected_env="$compat|$explicit_bin|$compat/out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual" != explicit-bin ] || [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL independent FRAM_BIN precedence\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

actual="$(
  env -u FRAM_HOME \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_BIN="$explicit_bin" \
    FRAM_OUT="$explicit_out" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$ROOT/bin/north" world-manifest-probe
)"
expected_env="$compat|$explicit_bin|$explicit_out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual" != explicit-bin ] || [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL independent FRAM_OUT precedence\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

echo "north Fram compatibility defaults: PASS"
