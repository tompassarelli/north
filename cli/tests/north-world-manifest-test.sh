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
fallback="$home/code/fram/main"
obsolete="$home/code/fram"

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
make_fram "$fallback" compiled-fallback
make_fram "$obsolete" obsolete-layout
make_bin "$explicit_bin" explicit-bin

printf 'WORLD_REPO_FRAM=%q\n' "$selected" >"$manifest"

run_north() {
  env -u FRAM_HOME -u FRAM_BIN -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_TEST_ENV_LOG="$fixture/resolved.env" \
    "$ROOT/bin/north" world-manifest-probe
}

actual="$(run_north)"
if [ "$actual" != selected ]; then
  printf 'FAIL manifest-selected Fram root\n  expected: selected\n  actual:   %s\n' "$actual" >&2
  exit 1
fi
expected_env="$selected|$selected/bin|$selected/out"
actual_env="$(<"$fixture/resolved.env")"
if [ "$actual_env" != "$expected_env" ]; then
  printf 'FAIL manifest-selected Fram environment export\n  expected: %s\n  actual:   %s\n' \
    "$expected_env" "$actual_env" >&2
  exit 1
fi

actual="$(
  env -u FRAM_BIN -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_HOME="$explicit" \
    "$ROOT/bin/north" world-manifest-probe
)"
if [ "$actual" != explicit-home ]; then
  printf 'FAIL explicit FRAM_HOME precedence\n  expected: explicit-home\n  actual:   %s\n' "$actual" >&2
  exit 1
fi

actual="$(
  env -u FRAM_HOME -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$manifest" \
    FRAM_BIN="$explicit_bin" \
    "$ROOT/bin/north" world-manifest-probe
)"
if [ "$actual" != explicit-bin ]; then
  printf 'FAIL explicit FRAM_BIN precedence\n  expected: explicit-bin\n  actual:   %s\n' "$actual" >&2
  exit 1
fi

actual="$(
  env -u FRAM_HOME -u FRAM_BIN -u FRAM_OUT \
    HOME="$home" \
    WORLD_MANIFEST_PATH="$fixture/missing.env" \
    "$ROOT/bin/north" world-manifest-probe
)"
if [ "$actual" != compiled-fallback ]; then
  printf 'FAIL compiled Fram fallback\n  expected: compiled-fallback\n  actual:   %s\n' "$actual" >&2
  exit 1
fi

echo "north world manifest: PASS"
