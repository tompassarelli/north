#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UP="$ROOT/bin/north-coord-up"
FRAM_CHECKOUT="${FRAM_TEST_CHECKOUT:-$(cd "$ROOT/../fram" && pwd)}"
TMP_ROOT="$(mktemp -d)"
TMP="$TMP_ROOT/state with spaces"
STATE="$TMP/state"
FAKE_BIN="$TMP/fram-bin"
DAEMON_PID=
LISTENER_PID=
REAL_BB="$(command -v bb)"
HOST_PATH="$PATH"

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ -n "$LISTENER_PID" ]]; then
    kill "$LISTENER_PID" 2>/dev/null || true
  fi
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

mkdir -p "$FAKE_BIN" "$TMP/home/.local/state/north/threads"
: >"$TMP/home/.local/state/north/facts.log"

cat >"$FAKE_BIN/fram" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == doctor ]]; then
  case "$(cat "$FRAM_TEST_STATE/mode" 2>/dev/null || true)" in
    strict|strict-peer|compat)
      echo "coordinator UP on 127.0.0.1:$FRAM_PORT (v1)"
      exit 0
      ;;
    mismatch)
      # Deliberately exit zero and retain the tempting words. The launcher must
      # require the exact healthy first-line shape, not a substring.
      echo "coordinator UP on 127.0.0.1:$FRAM_PORT (v1) — WRONG LOG"
      exit 0
      ;;
    *)
      echo "coordinator DOWN on 127.0.0.1:$FRAM_PORT"
      exit 1
      ;;
  esac
fi
printf '%s\n' "$*" >"$FRAM_TEST_STATE/engine-call"
EOF

cat >"$FAKE_BIN/fram-daemon" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$$" >"$FRAM_TEST_STATE/daemon-pid"
printf '%s\n' "$*" >"$FRAM_TEST_STATE/daemon-args"
printf '%s\n' \
  "selector=$NORTH_FRAM_RUNTIME" \
  "source=$FRAM_RUNTIME_SOURCE" \
  "rev=$FRAM_RUNTIME_REV" \
  "daemon=$FRAM_RUNTIME_DAEMON" \
  >"$FRAM_TEST_STATE/runtime-identity"
for packaged_name in FRAM_PACKAGED FRAM_JAVA FRAM_DAEMON_CLASSPATH_FILE FRAM_RESOLVE FRAM_PACKAGE_REV; do
  if [[ -n "${!packaged_name:-}" ]]; then
    printf '%s\n' "$packaged_name" >>"$FRAM_TEST_STATE/package-residue"
  fi
done
echo strict >"$FRAM_TEST_STATE/mode"
trap 'rm -f "$FRAM_TEST_STATE/mode"; exit 0' TERM INT
while :; do sleep 1; done
EOF

cat >"$FAKE_BIN/ss" <<'EOF'
#!/usr/bin/env bash
mode="$(cat "$FRAM_TEST_STATE/mode" 2>/dev/null || true)"
case "$mode" in
  strict)
    pid="$(cat "$FRAM_TEST_STATE/daemon-pid" 2>/dev/null || true)"
    ;;
  strict-peer|compat|mismatch)
    pid="$(cat "$FRAM_TEST_STATE/listener-pid" 2>/dev/null || true)"
    ;;
  *)
    pid=
    ;;
esac
if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
  echo "LISTEN 0 128 127.0.0.1:39871 0.0.0.0:* users:((\"java\",pid=$pid,fd=1))"
fi
EOF

cat >"$FAKE_BIN/bb" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == */cli/coord.clj && "${2:-}" == strict-probe ]]; then
  if [[ "$(cat "$FRAM_TEST_STATE/mode" 2>/dev/null || true)" =~ ^strict(-peer)?$ ]]; then
    printf '{:ready true :version 1 :log "%s"}\n' "$FRAM_LOG"
    exit 0
  fi
  echo '{:ready false :reason :raw-request-not-rejected}'
  exit 1
fi
exec "$REAL_BB" "$@"
EOF

chmod +x "$FAKE_BIN/fram" "$FAKE_BIN/fram-daemon" "$FAKE_BIN/ss" "$FAKE_BIN/bb"

common_env=(
  HOME="$TMP/home"
  XDG_STATE_HOME="$TMP/home/.local/state"
  FRAM_HOME="$TMP/absent-runtime-root"
  FRAM_BIN="$FAKE_BIN"
  FRAM_PORT=39871
  FRAM_LOG="$TMP/home/.local/state/north/facts.log"
  FRAM_DAEMON_LOG="$TMP/daemon.log"
  FRAM_PACKAGED=1
  FRAM_JAVA=/nix/store/stale-fram-java/bin/java
  FRAM_DAEMON_CLASSPATH_FILE=/nix/store/stale-fram/daemon.classpath
  FRAM_RESOLVE=/nix/store/stale-fram/resolve.clj
  FRAM_PACKAGE_REV=stale-package-revision
  NORTH_HOME=/nix/store/stale-north
  NORTH_PACKAGE_MODE=nix-store
  NORTH_COORD_PID_FILE="$STATE/published.pid"
  FRAM_TEST_STATE="$STATE"
  REAL_BB="$REAL_BB"
  PATH="$FAKE_BIN:$HOST_PATH"
)
mkdir -p "$STATE"

for bad_port in 0 65536 nope '79|coordinator UP'; do
  if env "${common_env[@]}" FRAM_PORT="$bad_port" "$UP" >"$TMP/bad-port.out" 2>&1; then
    echo "north-coord-up test: accepted invalid port '$bad_port'" >&2
    exit 1
  fi
  grep -q 'FRAM_PORT must be an integer from 1 through 65535' "$TMP/bad-port.out"
done

env "${common_env[@]}" "$UP" >"$TMP/start.out"
DAEMON_PID="$(cat "$STATE/daemon-pid")"
[[ "$(cat "$STATE/published.pid")" == "$DAEMON_PID" ]]
grep -q '^39871 .*/facts.log$' "$STATE/daemon-args"
grep -q '^coordinator up on :39871$' "$TMP/start.out"
grep -q '^selector=checkout$' "$STATE/runtime-identity"
grep -q "^source=$TMP/absent-runtime-root$" "$STATE/runtime-identity"
grep -q '^rev=unversioned$' "$STATE/runtime-identity"
grep -q "^daemon=$FAKE_BIN/fram-daemon$" "$STATE/runtime-identity"
[[ ! -e "$STATE/package-residue" ]]
RUNTIME_RECORD="$TMP/home/.local/state/north/fram-daemon-39871.runtime"
grep -q "^PID=$DAEMON_PID$" "$RUNTIME_RECORD"
grep -q "^FRAM_RUNTIME_SOURCE=$TMP/absent-runtime-root$" "$RUNTIME_RECORD"
grep -q '^FRAM_RUNTIME_REV=unversioned$' "$RUNTIME_RECORD"

env "${common_env[@]}" "$UP" --check-runtime >"$TMP/runtime-check.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/runtime-check.out"

env "${common_env[@]}" "$UP" >"$TMP/idempotent.out"
grep -q '^coordinator already up on :39871' "$TMP/idempotent.out"

kill "$DAEMON_PID"
for _ in $(seq 1 20); do
  [[ ! -e "$STATE/mode" ]] && break
  sleep 0.1
done
DAEMON_PID=
rm -f "$STATE/daemon-pid"

# Explicit package selection preserves the package execution contract and
# publishes its package revision instead of silently converting it to checkout.
env "${common_env[@]}" NORTH_FRAM_RUNTIME=package "$UP" >"$TMP/package-start.out"
DAEMON_PID="$(cat "$STATE/daemon-pid")"
grep -q '^selector=package$' "$STATE/runtime-identity"
grep -q '^rev=stale-package-revision$' "$STATE/runtime-identity"
grep -q '^FRAM_PACKAGED$' "$STATE/package-residue"
env "${common_env[@]}" NORTH_FRAM_RUNTIME=package "$UP" --check-runtime \
  >"$TMP/package-runtime-check.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/package-runtime-check.out"
kill "$DAEMON_PID"
for _ in $(seq 1 20); do
  [[ ! -e "$STATE/mode" ]] && break
  sleep 0.1
done
DAEMON_PID=
rm -f "$STATE/daemon-pid" "$STATE/package-residue"

# A strict, same-log listener with stale source/revision identity is not healthy.
# The no-restart path must fail closed and leave that peer untouched.
sleep 60 &
LISTENER_PID=$!
printf '%s\n' "$LISTENER_PID" >"$STATE/listener-pid"
echo strict-peer >"$STATE/mode"
FAKE_PROC="$TMP/fake-proc"
mkdir -p "$FAKE_PROC/$LISTENER_PID"
printf 'FRAM_RUNTIME_SOURCE=/nix/store/stale-fram\0FRAM_RUNTIME_REV=stale-revision\0FRAM_RUNTIME_DAEMON=/nix/store/stale-fram/bin/fram-daemon\0' \
  >"$FAKE_PROC/$LISTENER_PID/environ"
: >"$FAKE_PROC/$LISTENER_PID/cgroup"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" >"$TMP/stale-runtime.out" 2>&1; then
  echo "north-coord-up test: stale runtime identity was accepted" >&2
  exit 1
fi
grep -q 'coordinator runtime identity mismatch.*stale-fram.*desired source=' "$TMP/stale-runtime.out"
kill -0 "$LISTENER_PID"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  "$ROOT/bin/north" coord-doctor >"$TMP/stale-coord-doctor.out" 2>&1; then
  echo "north-coord-up test: public coord-doctor accepted stale runtime identity" >&2
  exit 1
fi
grep -q 'coord-doctor: coordinator runtime identity UNHEALTHY.*stale-fram' \
  "$TMP/stale-coord-doctor.out"

# A supervisor-owned listener is never signalled by the direct launcher. This
# reproduces the Restart=always race that reclaimed :7977 with a stale package.
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=unversioned\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$TMP/absent-runtime-root" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
printf '0::/system.slice/north-coord.service\n' >"$FAKE_PROC/$LISTENER_PID/cgroup"
rm -f "$STATE/daemon-pid"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --restart >"$TMP/supervised.out" 2>&1; then
  echo "north-coord-up test: direct restart accepted a supervisor-owned listener" >&2
  exit 1
fi
grep -q 'owned by systemd unit north-coord.service.*refusing a direct restart' "$TMP/supervised.out"
kill -0 "$LISTENER_PID"
[[ ! -e "$STATE/daemon-pid" ]]
kill "$LISTENER_PID"
wait "$LISTENER_PID" 2>/dev/null || true
LISTENER_PID=
rm -f "$STATE/listener-pid" "$STATE/mode"

# A live-checkout command never silently consumes a Nix-store FRAM_HOME/BIN.
# Package mode is a deliberate selector, not residue inherited from a wrapper.
if env "${common_env[@]}" \
  FRAM_HOME=/nix/store/stale-fram FRAM_BIN=/nix/store/stale-fram/bin \
  "$UP" --check-runtime >"$TMP/store-pin.out" 2>&1; then
  echo "north-coord-up test: implicit Nix-store runtime pin was accepted" >&2
  exit 1
fi
grep -q 'refusing inherited Nix-store Fram.*NORTH_FRAM_RUNTIME=package' "$TMP/store-pin.out"

# A same-log compatibility daemon is never "already up". Without explicit
# restart it is refused; with restart North owns and replaces the identified
# listener, then requires the new child to pass the strict probe.
sleep 60 &
LISTENER_PID=$!
printf '%s\n' "$LISTENER_PID" >"$STATE/listener-pid"
echo compat >"$STATE/mode"
if env "${common_env[@]}" "$UP" >"$TMP/compat.out" 2>&1; then
  echo "north-coord-up test: same-log compatibility daemon was accepted" >&2
  exit 1
fi
grep -q 'does not enforce corpus fences.*north up --restart' "$TMP/compat.out"
kill -0 "$LISTENER_PID"

env "${common_env[@]}" "$UP" --restart >"$TMP/compat-restart.out"
if kill -0 "$LISTENER_PID" 2>/dev/null; then
  echo "north-coord-up test: compatibility listener survived explicit restart" >&2
  exit 1
fi
wait "$LISTENER_PID" 2>/dev/null || true
LISTENER_PID=
rm -f "$STATE/listener-pid"
DAEMON_PID="$(cat "$STATE/daemon-pid")"
grep -q '^coordinator up on :39871$' "$TMP/compat-restart.out"

kill "$DAEMON_PID"
for _ in $(seq 1 20); do
  [[ ! -e "$STATE/mode" ]] && break
  sleep 0.1
done
DAEMON_PID=
rm -f "$STATE/daemon-pid"

sleep 60 &
LISTENER_PID=$!
printf '%s\n' "$LISTENER_PID" >"$STATE/listener-pid"
echo mismatch >"$STATE/mode"
if env "${common_env[@]}" "$UP" >"$TMP/mismatch.out" 2>&1; then
  echo "north-coord-up test: wrong-log doctor output was accepted" >&2
  exit 1
fi
grep -q 'occupied, but Fram did not verify' "$TMP/mismatch.out"
[[ ! -e "$STATE/daemon-pid" ]]
kill "$LISTENER_PID"
wait "$LISTENER_PID" 2>/dev/null || true
LISTENER_PID=
rm -f "$STATE/listener-pid"
rm -f "$STATE/mode"

# FRAM_BIN and FRAM_OUT are independent package seams. Engine verbs must use
# the public executable directory even when FRAM_HOME is not a checkout.
env "${common_env[@]}" FRAM_OUT="$FRAM_CHECKOUT/out" \
  "$ROOT/bin/north" engine-probe alpha
grep -q '^engine-probe alpha$' "$STATE/engine-call"

# North's own namespace graph must load from FRAM_OUT without reaching through
# FRAM_HOME or invoking a raw libexec Fram script.
env "${common_env[@]}" FRAM_OUT="$FRAM_CHECKOUT/out" FRAM_PORT=39872 \
  "$ROOT/bin/north" validate >"$TMP/validate.out"
grep -q 'no violations' "$TMP/validate.out"

echo "north-coord-up tests: PASS"
