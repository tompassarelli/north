#!/usr/bin/env bash
set -euo pipefail
root=$(dirname "$(dirname "$(readlink -f "$0")")")
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
profiles=$scratch/home/.local/state/north/profiles
mkdir -p "$scratch/bin" "$profiles/claude-a" "$profiles/claude-b" "$scratch/handoffs"
stub() { printf '#!/usr/bin/env bash\n%s\n' "$2" > "$scratch/bin/$1"; chmod +x "$scratch/bin/$1"; }
stub gh 'exit 0'
stub git 'exit 0'
stub threads 'exit 0'
stub timeout 'shift; exec "$@"'
stub north 'case $2 in accounts) printf "claude\ta\nclaude\tb\nclaude\tghost\n" ;; send) echo "$4" > "$SENT"; [ "${SEND_FAILS:-0}" = 1 ] && exit 1; exit 0 ;; esac'
stub claude 'cat > /dev/null
label=${CLAUDE_CONFIG_DIR##*claude-}
mode=MODE_$label
case ${!mode:-ok} in
  limit) echo "You'"'"'ve hit your weekly limit" >&2; exit 1 ;;
  broken) echo "Not logged in" >&2; exit 1 ;;
esac
echo "answered by $label"'
run() {
  rm -f "$scratch/sent" "$scratch/handoffs/cross-project-scan.md"
  env -u NORTH_PROFILES PATH="$scratch/bin:$PATH" HOME="$scratch/home" AGENTS_HANDOFFS="$scratch/handoffs" SENT="$scratch/sent" "$@" \
    bash "$root/bin/cross-project-scan" > /dev/null 2> "$scratch/err"
}
report=$scratch/handoffs/cross-project-scan.md
run MODE_a=ok MODE_b=ok
grep -q "answered by a" "$report"
run MODE_a=limit MODE_b=ok
grep -q "answered by b" "$report"
run MODE_a=limit MODE_b=limit
[ ! -e "$report" ]
grep -q "every claude account is at its usage limit" "$scratch/err"
if run MODE_a=broken MODE_b=limit; then echo "a broken account must fail the unit" >&2; exit 1; fi
grep -q "claude account a failed: Not logged in" "$scratch/err"
[ ! -e "$report" ]
run MODE_a=ok MODE_b=ok SEND_FAILS=1
grep -q "answered by a" "$report"
grep -q "proxy not reachable" "$scratch/err"
