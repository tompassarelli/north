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
current="$home/code/fram/main"

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
make_fram "$current" current-main
make_bin "$explicit_bin" explicit-bin
mkdir -p "$home/code/north"

printf 'WORLD_REPO_FRAM=%q\n' "$selected" >"$manifest"

ln -s "$ROOT" "$home/code/north/main"
actual="$(
  env -u FRAM_HOME -u FRAM_BIN -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$ROOT/bin/north" world-manifest-probe
)"
if [ "$actual" != current-main ]; then
  printf 'FAIL current-main default probe\n  expected: current-main\n  actual:   %s\n' "$actual" >&2
  exit 1
fi
expected_env="$current|$current/bin|$current/out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL current-main defaults\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

actual="$(
  HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_HOME="$explicit" \
    FRAM_BIN="$explicit/bin" \
    FRAM_OUT="$explicit/out" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$home/code/north/main/bin/north" world-manifest-probe
)"
unlink "$home/code/north/main"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual" != current-main ] || [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL live North main accepted injected Fram paths\n  expected: %s\n  actual:   %s\n' \
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
expected_env="$explicit|$explicit_bin|$explicit/out"
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
expected_env="$current|$explicit_bin|$current/out"
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
expected_env="$current|$explicit_bin|$explicit_out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual" != explicit-bin ] || [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL independent FRAM_OUT precedence\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

echo "north Fram current-main defaults: PASS"
