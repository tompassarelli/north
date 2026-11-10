#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
launcher="$repo/bin/agy"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/agy-launcher-test.XXXXXX")"
trap 'rm -rf -- "${scratch:?}"' EXIT

fail() { printf 'agy-launcher test: %s\n' "$*" >&2; exit 1; }

home="$scratch/home"
bin="$home/.local/bin"
runtime="$home/.local/lib/agy/current/bin/agy"
mkdir -p "$bin" "$(dirname "$runtime")"
ln -s "$launcher" "$bin/agy"

cat >"$runtime" <<'STUB'
#!/usr/bin/env bash
printf 'version=%s args=%s auto_update=%s native=%s\n' "${AGY_TEST_VERSION:-1.5.4}" "$*" "${AGY_CLI_DISABLE_AUTO_UPDATE:-}" "${THREADS_NO_NATIVE:-}"
STUB
chmod +x "$runtime"

run() { env -i HOME="$home" PATH="$bin:/usr/bin:/bin:/run/current-system/sw/bin" "$@"; }

# A fresh shell resolves `agy` from ~/.local/bin, the location Firn fills, and the
# tracked launcher reaches the installed runtime rather than a copy in this tree.
out="$(run agy --version)"
[[ "$out" == "version=1.5.4 args=--version auto_update=1 native=1" ]] || fail "fresh-shell agy --version printed '$out'"
printf 'PASS fresh shell: %s\n' "$out"

out="$(run agy -p /usage --output-format json)"
[[ "$out" == "version=1.5.4 args=-p /usage --output-format json auto_update=1 native=1" ]] || fail "arguments were not forwarded verbatim: '$out'"
printf 'PASS arguments forwarded\n'

override="$scratch/override/bin/agy"
mkdir -p "$(dirname "$override")"
cat >"$override" <<'STUB'
#!/usr/bin/env bash
printf 'override=%s\n' "$*"
STUB
chmod +x "$override"
out="$(run env AGY_RUNTIME="$override" "$bin/agy" --version)"
[[ "$out" == "override=--version" ]] || fail "AGY_RUNTIME was not honoured: '$out'"
printf 'PASS AGY_RUNTIME override\n'

err="$(run env AGY_RUNTIME="$scratch/missing/agy" "$bin/agy" --version 2>&1 >/dev/null)" && status=0 || status=$?
[[ "$status" -eq 127 ]] || fail "missing runtime exited $status, not 127"
[[ "$err" == "agy: runtime missing at $scratch/missing/agy" ]] || fail "missing runtime printed '$err'"
case "$err" in
  *"north runtime agy"*) fail 'missing runtime points at the retired north runtime agy command' ;;
esac
printf 'PASS missing runtime: %s\n' "$err"
