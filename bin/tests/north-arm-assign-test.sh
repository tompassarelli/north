#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
ASSIGN="$ROOT/bin/north-arm-assign"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/north-arm-assign-test.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

check_arm() {
  local id="$1" expected="$2" actual
  actual="$($ASSIGN "$id")"
  [ "$actual" = "$expected" ] || {
    echo "FAIL: $id expected $expected, saw $actual" >&2
    exit 1
  }
}

check_arm MSA-001 text
check_arm MSA-002 graph
check_arm 2026-08-01-120000 text
check_arm task-alpha graph
check_arm task-beta text
[ "$($ASSIGN task-alpha)" = "$($ASSIGN task-alpha)" ]

graph=0
text=0
for i in $(seq 0 999); do
  case "$($ASSIGN "synthetic-$i")" in
    graph) graph=$((graph + 1)) ;;
    text) text=$((text + 1)) ;;
    *) echo "FAIL: invalid arm" >&2; exit 1 ;;
  esac
done
[ "$graph" -ge 450 ] && [ "$graph" -le 550 ]
[ "$text" -ge 450 ] && [ "$text" -le 550 ]

FAKE_NORTH="$SCRATCH/north"
CALLS="$SCRATCH/calls"
cat >"$FAKE_NORTH" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$NORTH_ARM_ASSIGN_TEST_CALLS"
EOF
chmod +x "$FAKE_NORTH"
export NORTH_ARM_ASSIGN_NORTH_BIN="$FAKE_NORTH"
export NORTH_ARM_ASSIGN_TEST_CALLS="$CALLS"

[ "$($ASSIGN --record --thread thread-1 MSA-001)" = text ]
grep -Fxq 'tell thread-1 run_arm text' "$CALLS"

: >"$CALLS"
[ "$($ASSIGN task-beta --force graph --why 'operator constraint' --record --thread thread-2)" = graph ]
grep -Fxq 'tell thread-2 run_arm forced-graph' "$CALLS"
grep -Fxq 'tell thread-2 run_arm_why operator constraint' "$CALLS"

if "$ASSIGN" task-alpha --force text --why unaudited >/dev/null 2>&1; then
  echo "FAIL: unaudited force was accepted" >&2
  exit 1
fi

printf 'north-arm-assign-test: PASS fixed=5 repeat=stable synthetic graph=%d text=%d record=natural+forced\n' \
  "$graph" "$text"
