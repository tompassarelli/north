#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UP="$ROOT/bin/north-coord-up"
FRAM_CHECKOUT="${FRAM_TEST_CHECKOUT:-$(cd "$ROOT/../fram/main" && pwd)}"

# The integration cases below inject a deterministic route reader so they can
# exercise slot/backend races. Keep the real UNIX-socket reader connected to
# this required CI bar as well.
python3 "$ROOT/cli/tests/proxy-route-test.py"

TMP_ROOT="$(mktemp -d)"
TMP="$TMP_ROOT/state with spaces"
STATE="$TMP/state"
FAKE_BIN="$TMP/fram-bin"
FRAM_ROOT="$TMP/fram checkout"
DAEMON_PID=
LISTENER_PID=
EXTRA_PIDS=()
REAL_BB="$(command -v bb)"
HOST_PATH="$PATH"

write_fake_proc_stat() {
  local path="$1" pid="$2" birth="$3"
  {
    printf '%s (north runtime test) S' "$pid"
    for _ in $(seq 4 21); do printf ' 0'; done
    printf ' %s 0\n' "$birth"
  } >"$path"
}

write_runtime_selection() {
  local identity="$1" mode="$2" source="$3" revision="$4"
  local tree="$5" origin="$6" daemon="$7"
  mkdir -p "$(dirname "$identity")"
  printf '%s\n' \
    north-fram-runtime-v1 \
    "$mode" \
    "$source" \
    "$revision" \
    "$tree" \
    "$origin" \
    "$daemon" \
    >"$identity"
}

cleanup() {
  if [[ -n "$DAEMON_PID" ]]; then
    kill "$DAEMON_PID" 2>/dev/null || true
  fi
  if [[ -n "$LISTENER_PID" ]]; then
    kill "$LISTENER_PID" 2>/dev/null || true
  fi
  if [[ "${#EXTRA_PIDS[@]}" -gt 0 ]]; then
    kill "${EXTRA_PIDS[@]}" 2>/dev/null || true
  fi
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT

mkdir -p "$FAKE_BIN" "$FRAM_ROOT" "$TMP/home/.local/state/north/threads"
: >"$TMP/home/.local/state/north/facts.log"
printf 'tracked Fram source\n' >"$FRAM_ROOT/runtime.clj"
git -C "$FRAM_ROOT" init -q
git -C "$FRAM_ROOT" add runtime.clj
git -C "$FRAM_ROOT" \
  -c user.name='North Runtime Test' \
  -c user.email='north-runtime@example.invalid' \
  commit -qm 'fixture runtime'
FRAM_REV="$(git -C "$FRAM_ROOT" rev-parse HEAD)"
FRAM_TREE="$(git -C "$FRAM_ROOT" rev-parse 'HEAD^{tree}')"

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
  "tree=$FRAM_RUNTIME_TREE" \
  "daemon=$FRAM_RUNTIME_DAEMON" \
  "owner=$FRAM_RUNTIME_OWNER_TOKEN" \
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
emit_pid() {
  local pid="$1" port="$2"
  if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
    echo "LISTEN 0 128 127.0.0.1:$port 0.0.0.0:* users:((\"runtime\",pid=$pid,fd=1))"
  fi
}
if [[ "$mode" == proxy-* ]]; then
  proxy_pid="$(cat "$FRAM_TEST_STATE/proxy-pid" 2>/dev/null || true)"
  blue_pid="$(cat "$FRAM_TEST_STATE/blue-backend-pid" 2>/dev/null || true)"
  green_pid="$(cat "$FRAM_TEST_STATE/green-backend-pid" 2>/dev/null || true)"
  case "$*" in
    *"sport = :39871"*) emit_pid "$proxy_pid" 39871 ;;
    *"sport = :41001"*) emit_pid "$blue_pid" 41001 ;;
    *"sport = :42001"*) emit_pid "$green_pid" 42001 ;;
    *)
      emit_pid "$proxy_pid" 39871
      emit_pid "$blue_pid" 41001
      emit_pid "$green_pid" 42001
      ;;
  esac
  exit 0
fi
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

cat >"$FAKE_BIN/systemctl" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == is-enabled && "${2:-}" == --quiet && "${3:-}" == north-coord.service &&
      "${NORTH_TEST_SYSTEMD_ENABLED:-}" == 1 ]]; then
  exit 0
fi
exit 1
EOF

cat >"$FAKE_BIN/bb" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == */cli/coord.clj && "${2:-}" == strict-probe ]]; then
  printf '%s\n' "$*" >>"$FRAM_TEST_STATE/strict-probes"
  if [[ "$(cat "$FRAM_TEST_STATE/mode" 2>/dev/null || true)" =~ ^(strict(-peer)?|proxy-(blue|green))$ ]]; then
    printf '{:ready true :version 1 :log "%s"}\n' "$FRAM_LOG"
    exit 0
  fi
  echo '{:ready false :reason :raw-request-not-rejected}'
  exit 1
fi
exec "$REAL_BB" "$@"
EOF

cat >"$FAKE_BIN/proxy-route-reader" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
mode="$(cat "$FRAM_TEST_STATE/mode" 2>/dev/null || true)"
case "$mode" in
  proxy-blue) echo blue ;;
  proxy-green) echo green ;;
  proxy-malformed) echo purple ;;
  proxy-missing) echo "selector missing" >&2; exit 1 ;;
  proxy-durable-disagreement)
    echo "north proxy route: durable/runtime route disagreement: durable=blue runtime=green" >&2
    exit 1
    ;;
  proxy-transaction)
    echo "north proxy route: unfinished selector transaction exists" >&2
    exit 1
    ;;
  proxy-flip)
    count="$(cat "$FRAM_TEST_STATE/route-reads" 2>/dev/null || echo 0)"
    count=$((count + 1))
    printf '%s\n' "$count" >"$FRAM_TEST_STATE/route-reads"
    if [[ "$count" -eq 1 ]]; then echo blue; else echo green; fi
    ;;
  *) echo "selector unavailable" >&2; exit 1 ;;
esac
EOF

chmod +x "$FAKE_BIN/fram" "$FAKE_BIN/fram-daemon" "$FAKE_BIN/ss" "$FAKE_BIN/systemctl" "$FAKE_BIN/bb" "$FAKE_BIN/proxy-route-reader"

common_env=(
  HOME="$TMP/home"
  XDG_STATE_HOME="$TMP/home/.local/state"
  FRAM_HOME="$FRAM_ROOT"
  FRAM_BIN="$FAKE_BIN"
  FRAM_PORT=39871
  FRAM_LOG="$TMP/home/.local/state/north/facts.log"
  FRAM_DAEMON_LOG="$TMP/daemon.log"
  FRAM_PACKAGED=1
  FRAM_JAVA=/nix/store/stale-fram-java/bin/java
  FRAM_DAEMON_CLASSPATH_FILE=/nix/store/stale-fram/daemon.classpath
  FRAM_RESOLVE=/nix/store/stale-fram/resolve.clj
  FRAM_PACKAGE_REV=stale-package-revision
  FRAM_RUNTIME_REV=poisoned-ambient-revision
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

# An enabled systemd unit owns the cold-start control plane even while its
# listener is absent; the direct launcher must not recreate the :7977 race.
if env "${common_env[@]}" NORTH_TEST_SYSTEMD_ENABLED=1 "$UP" >"$TMP/enabled-unit-cold-start.out" 2>&1; then
  echo "north-coord-up test: direct cold start raced an enabled systemd unit" >&2
  exit 1
fi
grep -q 'north-coord.service is enabled; refusing a direct cold start.*systemctl start north-coord.service' \
  "$TMP/enabled-unit-cold-start.out"
[[ ! -e "$STATE/daemon-pid" ]]

env "${common_env[@]}" "$UP" >"$TMP/start.out"
DAEMON_PID="$(cat "$STATE/daemon-pid")"
[[ "$(cat "$STATE/published.pid")" == "$DAEMON_PID" ]]
grep -q '^39871 .*/facts.log$' "$STATE/daemon-args"
grep -q '^coordinator up on :39871$' "$TMP/start.out"
grep -q '^selector=checkout$' "$STATE/runtime-identity"
grep -q "^source=$FRAM_ROOT$" "$STATE/runtime-identity"
grep -q "^rev=$FRAM_REV$" "$STATE/runtime-identity"
grep -q "^tree=$FRAM_TREE$" "$STATE/runtime-identity"
grep -q "^daemon=$FAKE_BIN/fram-daemon$" "$STATE/runtime-identity"
grep -Eq '^owner=.+$' "$STATE/runtime-identity"
[[ ! -e "$STATE/package-residue" ]]
RUNTIME_RECORD="$TMP/home/.local/state/north/fram-daemon-39871.runtime"
grep -q "^PID=$DAEMON_PID$" "$RUNTIME_RECORD"
grep -Eq '^PID_BIRTH=(proc|ps):.+$' "$RUNTIME_RECORD"
grep -Eq '^OWNER_TOKEN=.+$' "$RUNTIME_RECORD"
grep -q "^FRAM_RUNTIME_SOURCE=$FRAM_ROOT$" "$RUNTIME_RECORD"
grep -q "^FRAM_RUNTIME_REV=$FRAM_REV$" "$RUNTIME_RECORD"
grep -q "^FRAM_RUNTIME_TREE=$FRAM_TREE$" "$RUNTIME_RECORD"

env "${common_env[@]}" "$UP" --check-runtime >"$TMP/runtime-check.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/runtime-check.out"

# A tracked mutation does not change HEAD; checkout health must still fail
# closed while leaving the already-running coordinator untouched.
printf 'tracked Fram source\nmutated at unchanged HEAD\n' >"$FRAM_ROOT/runtime.clj"
if env "${common_env[@]}" "$UP" --check-runtime >"$TMP/dirty-runtime.out" 2>&1; then
  echo "north-coord-up test: tracked-dirty checkout was accepted at unchanged HEAD" >&2
  exit 1
fi
grep -q "refusing tracked-dirty Fram checkout at $FRAM_REV" "$TMP/dirty-runtime.out"
kill -0 "$DAEMON_PID"
printf 'tracked Fram source\n' >"$FRAM_ROOT/runtime.clj"
git -C "$FRAM_ROOT" diff --quiet --no-ext-diff HEAD --

env "${common_env[@]}" "$UP" >"$TMP/idempotent.out"
grep -q '^coordinator already up on :39871' "$TMP/idempotent.out"

# The explicit stop surface accepts only the exact launcher-owned, strict,
# same-log listener and leaves no stale PID authority behind.
STOPPED_DAEMON_PID="$DAEMON_PID"
env "${common_env[@]}" "$UP" --stop >"$TMP/owned-stop.out"
grep -q '^stopping coordinator on :39871$' "$TMP/owned-stop.out"
grep -q '^coordinator stopped on :39871$' "$TMP/owned-stop.out"
if kill -0 "$STOPPED_DAEMON_PID" 2>/dev/null; then
  echo "north-coord-up test: launcher-owned coordinator survived stop" >&2
  exit 1
fi
[[ ! -e "$RUNTIME_RECORD" ]]
[[ ! -e "$STATE/published.pid" ]]
DAEMON_PID=

env "${common_env[@]}" "$UP" --stop >"$TMP/already-stopped.out"
grep -q '^coordinator already stopped on :39871$' "$TMP/already-stopped.out"

env "${common_env[@]}" "$UP" >"$TMP/restart-after-stop.out"
DAEMON_PID="$(cat "$STATE/daemon-pid")"
[[ "$DAEMON_PID" != "$STOPPED_DAEMON_PID" ]]
grep -q '^coordinator up on :39871$' "$TMP/restart-after-stop.out"

# A matching owner token + PID + birth record authorizes replacement of the
# exact direct child this launcher previously published.
OLD_DAEMON_PID="$DAEMON_PID"
env "${common_env[@]}" "$UP" --restart >"$TMP/owned-restart.out"
DAEMON_PID="$(cat "$STATE/daemon-pid")"
[[ "$DAEMON_PID" != "$OLD_DAEMON_PID" ]]
if kill -0 "$OLD_DAEMON_PID" 2>/dev/null; then
  echo "north-coord-up test: launcher-owned coordinator survived replacement" >&2
  exit 1
fi
grep -q '^stopping coordinator on :39871$' "$TMP/owned-restart.out"
grep -q '^coordinator up on :39871$' "$TMP/owned-restart.out"
grep -q "^PID=$DAEMON_PID$" "$RUNTIME_RECORD"

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
grep -q '^tree=immutable:stale-package-revision$' "$STATE/runtime-identity"
if grep -q 'poisoned-ambient-revision' "$STATE/runtime-identity"; then
  echo "north-coord-up test: ambient FRAM_RUNTIME_REV overrode authoritative package revision" >&2
  exit 1
fi
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
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$FRAM_ROOT" "$FRAM_REV" "$FRAM_TREE" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
printf '0::/system.slice/north-coord.service/subgroup\n' >"$FAKE_PROC/$LISTENER_PID/cgroup"
rm -f "$STATE/daemon-pid"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --restart >"$TMP/supervised.out" 2>&1; then
  echo "north-coord-up test: direct restart accepted a supervisor-owned listener" >&2
  exit 1
fi
grep -q 'owned by systemd unit north-coord.service.*refusing a direct stop/restart' "$TMP/supervised.out"
kill -0 "$LISTENER_PID"
[[ ! -e "$STATE/daemon-pid" ]]

if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --stop >"$TMP/supervised-stop.out" 2>&1; then
  echo "north-coord-up test: direct stop accepted a supervisor-owned listener" >&2
  exit 1
fi
grep -q 'owned by systemd unit north-coord.service.*refusing a direct stop/restart' "$TMP/supervised-stop.out"
kill -0 "$LISTENER_PID"

# PID reuse cannot inherit a stale ownership record. Keep the PID field and
# owner token identical while changing only the process birth identity.
write_fake_proc_stat "$FAKE_PROC/$LISTENER_PID/stat" "$LISTENER_PID" 222
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0FRAM_RUNTIME_OWNER_TOKEN=owned-token\0' \
  "$FRAM_ROOT" "$FRAM_REV" "$FRAM_TREE" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
: >"$FAKE_PROC/$LISTENER_PID/cgroup"
printf '%s\n' \
  "PID=$LISTENER_PID" \
  'PID_BIRTH=proc:111' \
  'OWNER_TOKEN=owned-token' \
  "FRAM_RUNTIME_SOURCE=$FRAM_ROOT" \
  "FRAM_RUNTIME_REV=$FRAM_REV" \
  "FRAM_RUNTIME_TREE=$FRAM_TREE" \
  "FRAM_RUNTIME_DAEMON=$FAKE_BIN/fram-daemon" \
  >"$RUNTIME_RECORD"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --restart >"$TMP/pid-reuse.out" 2>&1; then
  echo "north-coord-up test: stale ownership record authorized a reused PID" >&2
  exit 1
fi
grep -q 'refusing to signal an unowned coordinator.*pid/birth/token' "$TMP/pid-reuse.out"
kill -0 "$LISTENER_PID"
[[ ! -e "$STATE/daemon-pid" ]]

# A launchd-style/otherwise unknown supervisor has no Linux cgroup unit to name,
# but the platform-neutral ownership record still refuses it before signal.
printf '0::/launchd/north-coordinator\n' >"$FAKE_PROC/$LISTENER_PID/cgroup"
printf '%s\n' \
  'PID=0' \
  'PID_BIRTH=ps:unknown' \
  'OWNER_TOKEN=unknown-owner' \
  >"$RUNTIME_RECORD"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --restart >"$TMP/unknown-owner.out" 2>&1; then
  echo "north-coord-up test: unknown supervisor listener was signalled" >&2
  exit 1
fi
grep -q 'refusing to signal an unowned coordinator.*pid/birth/token' "$TMP/unknown-owner.out"
kill -0 "$LISTENER_PID"
[[ ! -e "$STATE/daemon-pid" ]]

# A systemd-supervised coordinator always executes a rev-pinned deployment
# snapshot worktree, not the sibling checkout `north up` targets by path.
# Identity must accept that snapshot when git independently verifies its
# HEAD/tree against the desired rev+tree.
SNAPSHOT_ROOT="$TMP/fram-runtime/deployments/$FRAM_REV"
git clone -q "$FRAM_ROOT" "$SNAPSHOT_ROOT"
git -C "$SNAPSHOT_ROOT" checkout -q "$FRAM_REV"
: >"$FAKE_PROC/$LISTENER_PID/cgroup"

# coord-doctor's desired identity follows the durable runtime selection. An
# explicit checkout promotion outranks the launcher's Nix-store package pin;
# without one, the package pin remains authoritative.
PROMOTION_STATE="$TMP/runtime-selection"
PROMOTION_IDENTITY="$PROMOTION_STATE/active/current.identity"
write_runtime_selection "$PROMOTION_IDENTITY" checkout "$SNAPSHOT_ROOT" \
  "$FRAM_REV" "$FRAM_TREE" "$FRAM_ROOT" "$SNAPSHOT_ROOT/bin/fram-daemon"
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$SNAPSHOT_ROOT" "$FRAM_REV" "$FRAM_TREE" "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  "$UP" --check-runtime >"$TMP/promotion-match.out"
grep -q "^coordinator runtime identity OK on :39871 (identity: promoted $FRAM_REV)" \
  "$TMP/promotion-match.out"

printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$FRAM_ROOT" stale-package-revision immutable:stale-package-revision "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  "$UP" --check-runtime >"$TMP/promotion-default-running.out" 2>&1; then
  echo "north-coord-up test: package default was accepted over an explicit promotion" >&2
  exit 1
fi
grep -q 'coordinator runtime identity UNHEALTHY' "$TMP/promotion-default-running.out"
grep -q "repair.*north-coord-runtime promote.*$FRAM_REV.*restart" \
  "$TMP/promotion-default-running.out"
if grep -Eq 'repair.*restart' "$TMP/promotion-default-running.out" &&
   ! grep -Eq 'repair.*north-coord-runtime promote.*restart' \
     "$TMP/promotion-default-running.out"; then
  echo "north-coord-up test: identity repair prescribed a bare restart" >&2
  exit 1
fi

# A no-cutover system activation can stage a newer package while the durable
# selector and listener intentionally remain on the previously adopted package.
# Readiness must attest selector==listener, not new-wrapper-package==listener.
write_runtime_selection "$PROMOTION_IDENTITY" package "$SNAPSHOT_ROOT" \
  "$FRAM_REV" "immutable:$FRAM_REV" "$SNAPSHOT_ROOT" \
  "$SNAPSHOT_ROOT/bin/fram-daemon"
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$SNAPSHOT_ROOT" "$FRAM_REV" "immutable:$FRAM_REV" \
  "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  FRAM_PACKAGE_REV=newly-built-but-not-adopted \
  "$UP" --check-runtime >"$TMP/package-selection-match.out"
grep -q "^coordinator runtime identity OK on :39871 (identity: selected package $FRAM_REV)" \
  "$TMP/package-selection-match.out"

# Same revision is not enough for an adopted package selector: a different
# immutable source/executable is a real selector/listener mismatch.
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "/nix/store/different-fram/libexec/fram" "$FRAM_REV" \
  "immutable:$FRAM_REV" "/nix/store/different-fram/bin/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  FRAM_PACKAGE_REV=newly-built-but-not-adopted \
  "$UP" --check-runtime >"$TMP/package-selection-source-drift.out" 2>&1; then
  echo "north-coord-up test: package selector/source drift was accepted" >&2
  exit 1
fi
grep -q 'does not match durable package selector' \
  "$TMP/package-selection-source-drift.out"

# The same rule remains fail-closed: selector/listener drift is unhealthy even
# when the newly built package is intentionally irrelevant to this check.
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$SNAPSHOT_ROOT" stale-running-revision immutable:stale-running-revision \
  "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  FRAM_PACKAGE_REV=newly-built-but-not-adopted \
  "$UP" --check-runtime >"$TMP/package-selection-drift.out" 2>&1; then
  echo "north-coord-up test: package selector/listener drift was accepted" >&2
  exit 1
fi
grep -q 'coordinator runtime identity UNHEALTHY' \
  "$TMP/package-selection-drift.out"
grep -q 'repair.*coordinated cutover protocol' \
  "$TMP/package-selection-drift.out"

rm -f "${PROMOTION_IDENTITY:?}"
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$FRAM_ROOT" stale-package-revision immutable:stale-package-revision \
  "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  "$UP" --check-runtime >"$TMP/no-promotion-default.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/no-promotion-default.out"
if grep -q 'promoted' "$TMP/no-promotion-default.out"; then
  echo "north-coord-up test: package default was reported as promoted" >&2
  exit 1
fi

printf 'FRAM_RUNTIME_SOURCE=/nix/store/unknown-fram\0FRAM_RUNTIME_REV=unknown-revision\0FRAM_RUNTIME_TREE=immutable:unknown-revision\0FRAM_RUNTIME_DAEMON=/nix/store/unknown-fram/bin/fram-daemon\0' \
  >"$FAKE_PROC/$LISTENER_PID/environ"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" \
  NORTH_COORD_RUNTIME_STATE="$PROMOTION_STATE" NORTH_FRAM_RUNTIME=package \
  "$UP" --check-runtime >"$TMP/no-promotion-unknown.out" 2>&1; then
  echo "north-coord-up test: unknown runtime was accepted without a promotion" >&2
  exit 1
fi
grep -q 'coordinator runtime identity UNHEALTHY' "$TMP/no-promotion-unknown.out"

printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$SNAPSHOT_ROOT" "$FRAM_REV" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --check-runtime \
  >"$TMP/snapshot-match.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/snapshot-match.out"
kill -0 "$LISTENER_PID"

# A deployment-shaped source path claiming the WRONG rev must still fail
# closed. Matching the deployments/<rev> path shape is never sufficient
# identity on its own; the desired rev must still match.
FAKE_REV="deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
WRONG_SNAPSHOT_ROOT="$TMP/fram-runtime/deployments/$FAKE_REV"
cp -r "$SNAPSHOT_ROOT" "$WRONG_SNAPSHOT_ROOT"
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$WRONG_SNAPSHOT_ROOT" "$FAKE_REV" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
if env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --check-runtime \
  >"$TMP/snapshot-wrong-rev.out" 2>&1; then
  echo "north-coord-up test: deployment-shaped source at the wrong rev was accepted" >&2
  exit 1
fi
grep -q 'coordinator runtime identity UNHEALTHY' "$TMP/snapshot-wrong-rev.out"
kill -0 "$LISTENER_PID"

# A plain checkout-source match (no deployment snapshot path at all) is still
# accepted directly by path equality — the new snapshot allowance must not
# have disturbed the original ordinary-match case.
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$FRAM_ROOT" "$FRAM_REV" "$FRAM_TREE" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" "$UP" --check-runtime \
  >"$TMP/plain-checkout-match.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/plain-checkout-match.out"
kill -0 "$LISTENER_PID"

# The packaged launcher must judge code identity by revision, not by whether a
# same-revision coordinator reports a checkout/deployment source path.
printf 'FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$SNAPSHOT_ROOT" "$FRAM_REV" "$FAKE_BIN/fram-daemon" \
  >"$FAKE_PROC/$LISTENER_PID/environ"
env "${common_env[@]}" NORTH_PROC_ROOT="$FAKE_PROC" NORTH_FRAM_RUNTIME=package \
  FRAM_PACKAGE_REV="$FRAM_REV" "$UP" --check-runtime \
  >"$TMP/rev-match-mode-advisory.out"
grep -q '^coordinator runtime identity OK on :39871' "$TMP/rev-match-mode-advisory.out"
grep -q '^north coord-doctor: advisory .*matching revision' "$TMP/rev-match-mode-advisory.out"
kill -0 "$LISTENER_PID"

# The blue/green public ports belong to HAProxy, not Fram. Readiness follows
# HAProxy's exact runtime selector to one private backend for this log, loads
# that slot's durable generation identity, and attests the private JVM. The
# strict log-fence probe remains on the public port so routing itself stays in
# the safety boundary.
kill "$LISTENER_PID"
wait "$LISTENER_PID" 2>/dev/null || true
LISTENER_PID=
rm -f "$STATE/listener-pid"

sleep 60 &
PROXY_PID=$!
sleep 60 &
BLUE_BACKEND_PID=$!
sleep 60 &
GREEN_BACKEND_PID=$!
EXTRA_PIDS=("$PROXY_PID" "$BLUE_BACKEND_PID" "$GREEN_BACKEND_PID")
printf '%s\n' "$PROXY_PID" >"$STATE/proxy-pid"
printf '%s\n' "$BLUE_BACKEND_PID" >"$STATE/blue-backend-pid"
printf '%s\n' "$GREEN_BACKEND_PID" >"$STATE/green-backend-pid"

PROXY_MARKER="$TMP/proxy-bootstrap-complete"
PROXY_SOCKET="$TMP/proxy-admin.sock"
PROXY_MAP="$TMP/proxy-route.map"
PROXY_TRANSACTION="$TMP/proxy-selector.transaction"
PROXY_LOCK="$TMP/proxy-selector.lock"
PROXY_RUNTIME="$TMP/proxy-runtime"
printf 'fram-coordinator-cutover/v1 active blue\n' >"$PROXY_MARKER"
: >"$PROXY_SOCKET"
printf 'active blue\n' >"$PROXY_MAP"
: >"$PROXY_LOCK"
write_runtime_selection "$PROXY_RUNTIME-blue/active/current.identity" package \
  "$SNAPSHOT_ROOT" "$FRAM_REV" "immutable:$FRAM_REV" \
  "$SNAPSHOT_ROOT" "$SNAPSHOT_ROOT/bin/fram-daemon"
write_runtime_selection "$PROXY_RUNTIME-green/active/current.identity" package \
  "$SNAPSHOT_ROOT" "$FRAM_REV" "immutable:$FRAM_REV" \
  "$SNAPSHOT_ROOT" "$SNAPSHOT_ROOT/bin/fram-daemon"

mkdir -p "$FAKE_PROC/$PROXY_PID" "$FAKE_PROC/$BLUE_BACKEND_PID" \
  "$FAKE_PROC/$GREEN_BACKEND_PID"
printf 'NORTH_COORD_HAPROXY_CONFIG=/nix/store/test-haproxy.cfg\0NORTH_COORD_BOOTSTRAP_MARKER=%s\0' \
  "$PROXY_MARKER" >"$FAKE_PROC/$PROXY_PID/environ"
: >"$FAKE_PROC/$PROXY_PID/cgroup"
printf 'NORTH_COORD_SLOT=blue\0FRAM_LOG=%s\0FRAM_PORT=41001\0FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$TMP/home/.local/state/north/facts.log" "$SNAPSHOT_ROOT" "$FRAM_REV" \
  "immutable:$FRAM_REV" "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$BLUE_BACKEND_PID/environ"
: >"$FAKE_PROC/$BLUE_BACKEND_PID/cgroup"
printf 'NORTH_COORD_SLOT=green\0FRAM_LOG=%s\0FRAM_PORT=42001\0FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$TMP/home/.local/state/north/facts.log" "$SNAPSHOT_ROOT" "$FRAM_REV" \
  "immutable:$FRAM_REV" "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$GREEN_BACKEND_PID/environ"
: >"$FAKE_PROC/$GREEN_BACKEND_PID/cgroup"

proxy_env=(
  NORTH_PROC_ROOT="$FAKE_PROC"
  NORTH_COORD_RUNTIME_STATE="$PROXY_RUNTIME"
  NORTH_COORD_BOOTSTRAP_MARKER="$PROXY_MARKER"
  NORTH_COORD_SELECTOR_SOCKET="$PROXY_SOCKET"
  NORTH_COORD_SELECTOR_MAP="$PROXY_MAP"
  NORTH_COORD_SELECTOR_TRANSACTION="$PROXY_TRANSACTION"
  NORTH_COORD_SELECTOR_LOCK="$PROXY_LOCK"
  NORTH_PROXY_ROUTE_READER="$FAKE_BIN/proxy-route-reader"
  NORTH_FRAM_RUNTIME=package
  FRAM_PACKAGE_REV="$FRAM_REV"
)

echo proxy-blue >"$STATE/mode"
env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-blue.out"
grep -q "selected package $FRAM_REV via blue backend :41001" \
  "$TMP/proxy-blue.out"

: >"$STATE/strict-probes"
env "${common_env[@]}" "${proxy_env[@]}" "$ROOT/bin/north" coord-safety \
  >"$TMP/proxy-safety.out"
grep -q '^coordinator strict log fence OK on :39871$' "$TMP/proxy-safety.out"
grep -q "strict-probe 39871 $TMP/home/.local/state/north/facts.log" \
  "$STATE/strict-probes"
if grep -q 'strict-probe 41001' "$STATE/strict-probes"; then
  echo "north-coord-up test: proxy readiness moved the strict fence off the public port" >&2
  exit 1
fi

echo proxy-green >"$STATE/mode"
env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-green.out"
grep -q "selected package $FRAM_REV via green backend :42001" \
  "$TMP/proxy-green.out"

echo proxy-malformed >"$STATE/mode"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-malformed.out" 2>&1; then
  echo "north-coord-up test: malformed HAProxy selector was accepted" >&2
  exit 1
fi
grep -q 'HAProxy active selector is malformed' "$TMP/proxy-malformed.out"

echo proxy-missing >"$STATE/mode"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-missing.out" 2>&1; then
  echo "north-coord-up test: missing HAProxy selector was accepted" >&2
  exit 1
fi
grep -q 'HAProxy active selector is unavailable' "$TMP/proxy-missing.out"

echo proxy-durable-disagreement >"$STATE/mode"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-durable-disagreement.out" 2>&1; then
  echo "north-coord-up test: durable/runtime selector disagreement was accepted" >&2
  exit 1
fi
grep -q 'durable/runtime route disagreement' \
  "$TMP/proxy-durable-disagreement.out"

echo proxy-transaction >"$STATE/mode"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-transaction.out" 2>&1; then
  echo "north-coord-up test: unfinished selector transaction was accepted" >&2
  exit 1
fi
grep -q 'unfinished selector transaction exists' "$TMP/proxy-transaction.out"

echo proxy-blue >"$STATE/mode"
mv "$PROXY_RUNTIME-blue/active/current.identity" \
  "$PROXY_RUNTIME-blue/active/current.identity.missing"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-missing-runtime.out" 2>&1; then
  echo "north-coord-up test: missing active-slot runtime selector was accepted" >&2
  exit 1
fi
grep -q 'blue backend runtime selector is missing or malformed' \
  "$TMP/proxy-missing-runtime.out"
mv "$PROXY_RUNTIME-blue/active/current.identity.missing" \
  "$PROXY_RUNTIME-blue/active/current.identity"

printf 'NORTH_COORD_SLOT=green\0FRAM_LOG=%s\0FRAM_PORT=41001\0FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$TMP/home/.local/state/north/facts.log" "$SNAPSHOT_ROOT" "$FRAM_REV" \
  "immutable:$FRAM_REV" "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$BLUE_BACKEND_PID/environ"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-route-backend-disagreement.out" 2>&1; then
  echo "north-coord-up test: HAProxy route/backend disagreement was accepted" >&2
  exit 1
fi
grep -q 'route selects blue but exactly one private backend.*observed 0' \
  "$TMP/proxy-route-backend-disagreement.out"
printf 'NORTH_COORD_SLOT=blue\0FRAM_LOG=%s\0FRAM_PORT=41001\0FRAM_RUNTIME_SOURCE=%s\0FRAM_RUNTIME_REV=%s\0FRAM_RUNTIME_TREE=%s\0FRAM_RUNTIME_DAEMON=%s\0' \
  "$TMP/home/.local/state/north/facts.log" "$SNAPSHOT_ROOT" "$FRAM_REV" \
  "immutable:$FRAM_REV" "$SNAPSHOT_ROOT/bin/fram-daemon" \
  >"$FAKE_PROC/$BLUE_BACKEND_PID/environ"

rm -f "$STATE/route-reads"
echo proxy-flip >"$STATE/mode"
if env "${common_env[@]}" "${proxy_env[@]}" "$UP" --check-runtime \
  >"$TMP/proxy-route-race.out" 2>&1; then
  echo "north-coord-up test: selector change during attestation was accepted" >&2
  exit 1
fi
grep -q 'route changed or disagreed while attesting the blue backend' \
  "$TMP/proxy-route-race.out"

kill "${EXTRA_PIDS[@]}"
wait "${EXTRA_PIDS[@]}" 2>/dev/null || true
EXTRA_PIDS=()
rm -f "$STATE/proxy-pid" "$STATE/blue-backend-pid" \
  "$STATE/green-backend-pid" "$STATE/mode"

# A live-checkout command never silently consumes a Nix-store FRAM_HOME/BIN.
# Package mode is a deliberate selector, not residue inherited from a wrapper.
if env "${common_env[@]}" \
  FRAM_HOME=/nix/store/stale-fram FRAM_BIN=/nix/store/stale-fram/bin \
  "$UP" --check-runtime >"$TMP/store-pin.out" 2>&1; then
  echo "north-coord-up test: implicit Nix-store runtime pin was accepted" >&2
  exit 1
fi
grep -q 'refusing inherited Nix-store Fram.*NORTH_FRAM_RUNTIME=package' "$TMP/store-pin.out"

if env "${common_env[@]}" NORTH_FRAM_RUNTIME=package \
  FRAM_PACKAGE_REV= FRAM_RUNTIME_REV= \
  "$UP" --check-runtime >"$TMP/unknown-package.out" 2>&1; then
  echo "north-coord-up test: mutable package seam with unknown provenance was accepted" >&2
  exit 1
fi
grep -q 'package runtime provenance is unknown' "$TMP/unknown-package.out"

if env "${common_env[@]}" NORTH_FRAM_RUNTIME=package \
  FRAM_PACKAGE_REV= FRAM_RUNTIME_REV=forged-runtime-observation \
  "$UP" --check-runtime >"$TMP/ambient-runtime-revision.out" 2>&1; then
  echo "north-coord-up test: ambient runtime observation became package authority" >&2
  exit 1
fi
grep -q 'package runtime provenance is unknown' "$TMP/ambient-runtime-revision.out"

# A store-shaped string is not immutable provenance. Neither a nonexistent
# source nor a mutable executable may synthesize a store revision.
if env "${common_env[@]}" NORTH_FRAM_RUNTIME=package \
  FRAM_HOME=/nix/store/does-not-exist FRAM_BIN="$FRAM_CHECKOUT/bin" \
  FRAM_PACKAGE_REV= FRAM_RUNTIME_REV= FRAM_OUT= FRAM_RESOLVE= \
  FRAM_DAEMON_CLASSPATH_FILE= \
  "$UP" --check-runtime >"$TMP/nonexistent-store-package.out" 2>&1; then
  echo "north-coord-up test: nonexistent store string became package provenance" >&2
  exit 1
fi
grep -q 'package runtime provenance is unknown' "$TMP/nonexistent-store-package.out"

# Canonicalization must not turn a dangling alias or an existing store object
# paired with mutable Fram executables into one coherent package identity.
ln -s /nix/store/does-not-exist "$TMP/dangling-store-alias"
if env "${common_env[@]}" NORTH_FRAM_RUNTIME=package \
  FRAM_HOME="$TMP/dangling-store-alias" FRAM_BIN="$FRAM_CHECKOUT/bin" \
  FRAM_PACKAGE_REV= FRAM_RUNTIME_REV= FRAM_OUT= FRAM_RESOLVE= \
  FRAM_DAEMON_CLASSPATH_FILE= \
  "$UP" --check-runtime >"$TMP/dangling-store-package.out" 2>&1; then
  echo "north-coord-up test: dangling store alias became package provenance" >&2
  exit 1
fi
grep -q 'package runtime provenance is unknown' "$TMP/dangling-store-package.out"

real_bb_path="$(realpath -e "$REAL_BB" 2>/dev/null || true)"
if [[ "$real_bb_path" == /nix/store/* ]]; then
  store_relative="${real_bb_path#/nix/store/}"
  existing_store_root="/nix/store/${store_relative%%/*}"
  ln -s "$existing_store_root" "$TMP/existing-store-alias"
  if env "${common_env[@]}" NORTH_FRAM_RUNTIME=package \
    FRAM_HOME="$TMP/existing-store-alias" FRAM_BIN="$FRAM_CHECKOUT/bin" \
    FRAM_PACKAGE_REV= FRAM_RUNTIME_REV= FRAM_OUT= FRAM_RESOLVE= \
    FRAM_DAEMON_CLASSPATH_FILE= \
    "$UP" --check-runtime >"$TMP/mixed-store-package.out" 2>&1; then
    echo "north-coord-up test: canonical store source accepted mutable Fram executables" >&2
    exit 1
  fi
  grep -q 'package runtime provenance is unknown' "$TMP/mixed-store-package.out"
fi

# A same-log compatibility daemon is never "already up". Even an explicit
# restart cannot signal it without a matching launcher ownership record.
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

if env "${common_env[@]}" "$UP" --restart >"$TMP/compat-restart.out" 2>&1; then
  echo "north-coord-up test: unowned compatibility listener was replaced" >&2
  exit 1
fi
grep -q 'refusing to signal an unowned coordinator' "$TMP/compat-restart.out"
kill -0 "$LISTENER_PID"
kill "$LISTENER_PID"
wait "$LISTENER_PID" 2>/dev/null || true
LISTENER_PID=
rm -f "$STATE/listener-pid"
rm -f "$STATE/mode"

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
