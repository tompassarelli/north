#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
updater="$repo/bin/claude-runtime-update"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/claude-runtime-update-test.XXXXXX")"
trap 'rm -rf -- "${scratch:?}"' EXIT

stubs="$scratch/bin"
payload="$scratch/claude"
mkdir -p "$stubs"
printf '%s\n' '#!/usr/bin/env bash' "printf '%s\\n' '9.9.9 (Claude Code)'" >"$payload"

cat >"$stubs/uname" <<'STUB'
#!/usr/bin/env bash
case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac
STUB
cat >"$stubs/zstd" <<'STUB'
#!/usr/bin/env bash
cp -- "$2" "$4"
STUB
cat >"$stubs/curl" <<'STUB'
#!/usr/bin/env bash
case "$2" in
  */manifest.json)
    sum="$(sha256sum "$TEST_PAYLOAD")"
    printf '{"version":"9.9.9","platforms":{"linux-x64":{"checksum":"%s"}}}\n' "${sum%% *}" >"$4" ;;
  */claude.zst) cp -- "$TEST_PAYLOAD" "$4" ;;
  *) exit 22 ;;
esac
STUB
cat >"$stubs/north" <<'STUB'
#!/usr/bin/env bash
current=absent
[[ ! -e "$HOME/.local/lib/claude/current" ]] || current=present
printf '%s %s current=%s\n' "$1 $2" "$3" "$current" >>"$TEST_PROBE_LOG"
exit "$TEST_PROBE_EXIT"
STUB
chmod +x "$stubs"/* "$payload"

run_update() {
  local home="$1" probe_exit="$2"
  mkdir -p "$home"
  HOME="$home" PATH="$stubs:$PATH" TEST_PAYLOAD="$payload" \
    TEST_PROBE_LOG="$home/probe.log" TEST_PROBE_EXIT="$probe_exit" \
    "$updater" 9.9.9
}

fail() { printf 'claude-runtime-update test: %s\n' "$1" >&2; exit 1; }

home="$scratch/pass"
run_update "$home" 0 >/dev/null
[[ "$(readlink "$home/.local/lib/claude/current")" == versions/9.9.9 ]] || fail 'passing probe did not promote'
grep -Eq "^contract-probe --runtime $home/\.local/lib/claude/\.stage\.[A-Za-z0-9]+/bin/claude current=absent$" "$home/probe.log" \
  || fail "probe was not called on the staged binary before promotion: $(cat "$home/probe.log")"

home="$scratch/refuse"
if run_update "$home" 1 >/dev/null 2>"$home.err"; then fail 'failing probe was promoted'; fi
grep -q 'contract probe failed; refusing promotion' "$home.err" || fail 'refusal was not reported'
[[ ! -e "$home/.local/lib/claude/current" && ! -e "$home/.local/lib/claude/versions/9.9.9" ]] \
  || fail 'failing probe left a promoted runtime'

printf 'claude-runtime-update tests passed\n'
