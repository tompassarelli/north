use super::{BESIDE_MISSING, LOCK_WAIT_SECONDS, RUNTIME_MISSING, OFFLOAD_ROOT, POLL_SECONDS, SCCACHE_FIRST_PORT, SCCACHE_PORTS, SLOTS, SLOTS_BUSY, TARGET_CAP_MIB, TARGET_FLOOR_MIB, TARGET_KEEP_MIB, TRANSPORT_FAILED, WAITED, WRONG_TREE, beside, quote};

/// Minutes a run whose client lost its connection keeps going with no client reattached, so a caller killed on purpose does not leave the run holding its slot to its deadline; the runner reattaches within `REATTACH_WINDOW`, which is shorter.
const REATTACH_MINUTES: u32 = 3;
const DRAIN_POLL: &str = "0.1";
/// Seconds a run whose check has recorded its exit keeps relaying to its client before it ends its own process group, so a client that stopped reading (a lander restarted under it) cannot hold the slot by a write that never completes; the exit and output stay on record for a reattaching client.
const DRAIN_GRACE_SECONDS: u32 = REATTACH_MINUTES * 60;
/// Polls of `DRAIN_POLL` a nested job whose check ended waits for its group to obey TERM before it sends KILL, and before it reports that the group survives KILL; it never frees the slot while a member is left.
const TERM_POLLS: u32 = 1200;
const KILL_POLLS: u32 = 1500;
/// Seconds past a nested job's own deadline its watcher waits for the check that holds the slot before it ends the job anyway, so a holder that never exits leaves no watcher behind.
const WATCH_MARGIN_SECONDS: u32 = 120;
/// Polls of `DRAIN_POLL` a nested job waits for its watcher to say it started before it refuses to run.
const WATCH_START_POLLS: u32 = 100;
const BACKGROUND_WAIT_SECONDS: u32 = 3600;
const BACKGROUND_SUFFIX: &str = ".background";

/// Whether the host starts this job now: a lander job (a red fix, an after-landing run, a culprit probe) at once, its floor; a land check while the host's `capacity host admit-job` admits one, since its lease is a landing lease, whose start the pressure reading counts and which opens no settle window; any other job also once the job last admitted here has settled or already ended, so the next pressure reading includes it; `NORTH_OFFLOAD_MAX_JOBS` caps the host's jobs for safety.
pub(super) const ADMIT: &str = r#"host_cap=${NORTH_OFFLOAD_MAX_JOBS:-}
admitted=$root/admitted why=
north_bin=
[ -z "${runtime:-}" ] || north_bin=$HOME/.local/state/north/runtimes/$runtime/north
[ -x "$north_bin" ] || north_bin=${AGENT_CAPACITY_NORTH:-$HOME/.local/state/north/current/north}
[ -x "$north_bin" ] || north_bin=$(command -v north 2>/dev/null) || north_bin=
admits() {
  [ -z "$host_cap" ] || [ "$(host_jobs)" -lt "$host_cap" ] || { why="the host runs at most $host_cap jobs"; return 1; }
  [ "${1:-}" != floor ] || return 0
  if [ "${1:-}" != landing ]; then
    last=0 last_slot=
    [ ! -r "$admitted" ] || read -r last <"$admitted" || last=0
    [ ! -r "$admitted.slot" ] || read -r last_slot <"$admitted.slot" || last_slot=
    since=$(($(date +%s) - last))
    settle=${NORTH_OFFLOAD_SETTLE_SECONDS:-@SETTLE@}
    [ "$since" -ge "$settle" ] || { [ -n "$last_slot" ] && ! slot_held "$root/$last_slot.lock"; } || { why="the job admitted ${since}s ago settles for $settle s"; return 1; }
  fi
  if [ -n "$north_bin" ]; then
    rc=0
    flag=
    [ "${1:-}" != landing ] || flag=--landing
    why=$("$north_bin" capacity host admit-job $flag 2>/dev/null) || rc=$?
    case $rc in 0) return 0 ;; 75) return 1 ;; esac
  fi
  [ "$(host_jobs)" -eq 0 ] || { why="this host's north answers no admit-job, so it runs one job at a time"; return 1; }
}
landing_full() {
  [ -n "$north_bin" ] || return 1
  rc=0
  "$north_bin" capacity host admit-job --landing >/dev/null 2>&1 || rc=$?
  [ "$rc" -eq 75 ]
}
"#;

pub(super) const JOBS: &str = r#"slot_lock() {
  if [ "$2" -eq 0 ]; then echo "$root/$1.lock"; else echo "$root/$1-$2.lock"; fi
}
@SLOT_HELD@slot_jobs() {
  count=0 j=0
  while [ "$j" -lt @LAND_SLOTS@ ]; do
    ! slot_held "$(slot_lock "$1" "$j")" || count=$((count + 1))
    j=$((j + 1))
  done
  echo "$count"
}
slot_holders() {
  j=0
  while [ "$j" -lt @LAND_SLOTS@ ]; do
    lock=$(slot_lock "$1" "$j")
    ! slot_held "$lock" || printf "%s %s;" "${lock##*/}" "$(cat "${lock%.lock}.holder" 2>/dev/null || echo "with no holder line")"
    j=$((j + 1))
  done
}
host_jobs() {
  count=0
  for lock in "$root"/*.lock; do
    case ${lock##*/} in *@BACKGROUND_SUFFIX@.lock | @WARM_LOCK@) continue ;; esac
    ! slot_held "$lock" || count=$((count + 1))
  done
  echo "$count"
}
"#;

pub(super) const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6 ref=$7 carried=${8:-} owner=${9:-} runtime=${10:-} red_fix=${11:-} ran=${12:-} pins=${13:-} lander_slot=${14:-} spilled=${15:-0} generated=${16:-}
root=@ROOT@
mkdir -p "$root"
if [ -n "$runtime" ] && { [ ! -x "$HOME/.local/state/north/runtimes/$runtime/north" ] || [ ! -f "$HOME/.local/state/north/runtimes/$runtime/@TREE_MARKER@" ]; }; then echo "offload: the lander runtime $runtime is not on this host yet" >&2; exit @RUNTIME_MISSING@; fi
bundle=$(mktemp "$root/$name.bundle.XXXXXX")
trap 'rm -f "$bundle" "${script:-}"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
trap 'exit 143' HUP INT TERM
[ "$spilled" -eq 0 ] || eval "$(head -c "$spilled")"
cat >"$bundle"
if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
  git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q --no-auto-maintenance "$bundle" "+$ref:$ref"
"#;

const NESTED: &str = r#"caller_slot=${@SLOT_ENV@:-} holder=${@SLOT_HOLDER_ENV@:-} nested=
[ -n "@BACKGROUND@" ] || [ "${caller_slot%/*}" != "$root" ] || [ -z "$holder" ] || ! kill -0 "$holder" 2>/dev/null || nested=1
if [ -n "$nested" ] && case ${caller_slot##*/} in "$name" | "$name"-[1-9]) true ;; *) false ;; esac && [ "$(git -C "$caller_slot" rev-parse HEAD 2>/dev/null)" = "$head" ]; then
  s=${caller_slot##*/} slot=$caller_slot
  exec 5>>"$slot.nested"
  flock -s 5
  kill -0 "$holder" 2>/dev/null || { echo "offload: refusing to run: the check that held $s ended before this nested job took its place there" >&2; exit @WRONG_TREE@; }
  echo "offload: running in $s, the slot this job's caller holds on this host" >&2
else
"#;

const TAKE: &str = r#"nearest=$(
  i=0
  while [ "$i" -lt @SLOTS@ ]; do
    s=$name
    [ "$i" -eq 0 ] || s=$name-$i
    d=999999999
    if [ -d "$root/$s/target" ] && at=$(git -C "$root/$s" rev-parse -q --verify HEAD 2>/dev/null) && files=$(git -C "$repo" diff --name-only "$at" "$head" 2>/dev/null); then
      d=$(printf '%s\n' "$files" | grep -c . || true)
    fi
    echo "$d $s"
    i=$((i + 1))
  done | sort -n -s -k1,1 | cut -d ' ' -f2
)
reserved=$name
[ @SLOTS@ -le 1 ] || reserved=$name-$((@SLOTS@ - 1))
nearest=$(printf '%s\n' $nearest | { grep -vx "$reserved" || true; }; echo "$reserved")
queue=$root/queue
mkdir -p "$queue"
rank=3
case " $carried " in *" SAFE_PUSH_LANDING='1' "*) rank=2 ;; esac
[ "$lander_slot" != 1 ] || rank=1
[ -z "$red_fix" ] || rank=0
[ -z "@BACKGROUND@" ] || rank=4
kind=
[ "$rank" -gt 1 ] || kind=floor
[ "$rank" -ne 2 ] || kind=landing
ticket=$queue/$rank-$(date +%s%N)-$$@$name
[ -n "$nested" ] || { exec 8>"$ticket"; flock 8; }
trap 'rm -f "$bundle" "$ticket"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
@JOBS@@ADMIT@served=$root/served
mkdir -p "$served"
turn_order() {
  for t in $(ls "$queue"); do
    repo=; at=0
    case $t in *@*) repo=${t#*@}; [ ! -r "$served/$repo" ] || read -r at <"$served/$repo" || at=0 ;; esac
    rest=${t#*-}
    printf '%s %s %s %s\n' "${t%%-*}" "$at" "${rest%%-*}" "$t"
  done | sort -s -k1,1n -k2,2n -k3,3 | cut -d ' ' -f4
}
first_in_line() {
  now=$(date +%s%N) passing=
  for t in $(turn_order); do
    f=$queue/$t
    [ "$f" != "$ticket" ] || return 0
    stamp=${t#*-}
    if [ $((now - ${stamp%%-*})) -gt 5000000000 ] && (exec 7>>"$f"; flock -n 7) 2>/dev/null; then rm -f "$f"; continue; fi
    case $t in *@*) [ -n "@BACKGROUND@" ] || [ "${t#*@}" = "$name" ] || [ "$(slot_jobs "${t#*@}")" -lt @SLOTS@ ] || continue ;; esac
    if [ "${t%%-*}" = 2 ] && [ "$kind" != landing ] && [ -z "@BACKGROUND@" ]; then
      [ -n "$passing" ] || { passing=no; ! landing_full || passing=yes; }
      [ "$passing" = no ] || continue
    fi
    waiter=${t#*-*-}
    why="behind the job of waiter ${waiter%@*} for ${t#*@}, in line $(((now - ${stamp%%-*}) / 1000000000))s"
    return 1
  done
  return 1
}
owned() {
  exec 9>"$root/$1.lock"
  if flock -n 9 && (exec 7>>"$root/$1.nested"; flock -n 7) 2>/dev/null; then return 0; fi
  exec 9>&-
  return 1
}
started=$(date +%s%N)
noted=0 polled=
while :; do
  if [ "$lander_slot" = 1 ]; then
    if owned "$reserved"; then s=$reserved; break; fi
  fi
  why="behind earlier jobs in line"
  ahead=1
  if [ -n "$nested" ] || first_in_line; then ahead=; fi
  if [ -n "$nested" ] || { [ -z "$ahead" ] && { [ -n "@BACKGROUND@" ] || admits $kind; }; }; then
    for s in $nearest; do
      if owned "$s"; then break 2; fi
    done
  fi
  waited=$(( ($(date +%s%N) - started) / 1000000000 ))
  [ "$waited" -lt @LOCK_WAIT@ ] || { echo "offload: queued behind other jobs for @LOCK_WAIT@ s ($why)" >&2; exit @SLOTS_BUSY@; }
  [ "$waited" -lt "$noted" ] || { noted=$((waited + 60)); held=$(slot_holders "$name"); echo "offload: waiting for a slot for ${waited}s, in order; $name runs $(slot_jobs "$name") of its @SLOTS@ slots, the host $(host_jobs) jobs; $why; $(ls "$queue" | grep -c .) queued; held by: ${held:-no slot of $name}" >&2; }
  polled=1
  if [ -n "$ahead" ]; then sleep 1; else sleep @POLL@; fi
done
if [ -n "$polled" ]; then
  waited=$(( ($(date +%s%N) - started) / 1000000 ))
  echo "@WAITED@$((waited / 1000)).$((waited % 1000 / 100))s in line for $s" >&2
fi
exec 4>>"$root/$s.busy"
flock -s 4
[ -n "$kind" ] || [ -n "@BACKGROUND@" ] || [ -n "$nested" ] || { echo "$s" >"$admitted.slot"; date +%s >"$admitted"; }
date +%s%N >"$served/$name"
rm -f "$ticket"
exec 8>&-
slot=$root/$s
# sccache keys hold absolute paths, so every slot names its own directory as the base its keys are relative to, and serves it on its own port.
case $s in @SLOT_PORTS@ esac
SCCACHE_BASEDIRS=$slot; export SCCACHE_BASEDIRS SCCACHE_SERVER_PORT
printf '%s since %s\n' "$(printf '%.12s' "$head")" "$(date +%H:%M:%S)" >"$root/$s.holder"
printf '%s\n' "$owner" >"$root/$s.owner"
placed=$HOME/@SLOT_RECORDS@
record_slot() {
  recorded_by=
  for candidate in "${north_bin:-}" "${AGENT_CAPACITY_NORTH:-}" "$HOME/.local/state/north/current/north" "$(command -v north 2>/dev/null)"; do
    [ -n "$candidate" ] && [ -x "$candidate" ] || continue
    why=$(cd "$repo" && "$candidate" capacity offload slot record "$s" 2>&1 </dev/null) && return 0
    recorded_by=$candidate
  done
  return 1
}
{ mkdir -p "$placed" && printf '%s\n' "${repo#"$HOME"/}" >"$placed/$s"; } 2>/dev/null || record_slot || echo "offload: cannot record $s as a slot of $repo, so the sandbox broker refuses a request this job makes${recorded_by:+ ($recorded_by said: $(printf '%s' "$why" | tail -n 1))}" >&2
# Buck2 keeps a project's daemon state in ~/.buck/buckd/<its real path>, and a daemon keeps the domain of the job that started it.
[ -n "${nested:-}" ] || rm -rf -- "$HOME/.buck/buckd$(cd "$root" && pwd -P)/${s:?}" || echo "offload: cannot remove the Buck2 daemon state the job before left for $s" >&2
printf '%s\n' "$$" >"$root/$s.pgid"
cleanup() {
  [ -z "${exited:-}" ] || [ -e "$exited" ] || echo 143 >"$exited" 2>/dev/null || true
  rm -f "$bundle" "$root/$s.owner" "$root/$s.pgid" "$root/$s.holder"
  rm -f "$placed/$s" 2>/dev/null || true
  rm -f "${script:-}"
  rmdir "${script_directory:-}" 2>/dev/null || true
  git -C "$repo" update-ref -d "$ref" 2>/dev/null || true
  if [ -e "$slot/.git" ]; then git -C "$slot" clean -ffdxq -e /target 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 143' HUP INT TERM
target_fits() {
  used=$(du -sm "$slot/target" | cut -f1)
  if [ -n "${AGENT_CAPACITY_DISK_FREE_GIB:-}" ]; then free=$(awk -v gib="$AGENT_CAPACITY_DISK_FREE_GIB" 'BEGIN { printf "%d", gib * 1024 }'); else free=$(df -Pm "$root" | awk 'NR == 2 { print $4 }'); fi
  [ "$used" -le "${NORTH_OFFLOAD_TARGET_CAP_MIB:-@TARGET_CAP@}" ] && [ "$free" -ge "${NORTH_OFFLOAD_TARGET_FLOOR_MIB:-@TARGET_FLOOR@}" ]
}
if [ -d "$slot/target" ] && ! target_fits; then
  trimmer=$(command -v north 2>/dev/null) || trimmer=
  [ -z "$runtime" ] || trimmer=$HOME/.local/state/north/runtimes/$runtime/north
  if [ -z "$trimmer" ] || ! "$trimmer" capacity offload trim-target "$slot/target" "${NORTH_OFFLOAD_TARGET_KEEP_MIB:-@TARGET_KEEP@}" >&2 || ! target_fits; then
    echo "offload: removing $s/target ($used MiB, $free MiB free on the host)" >&2
    rm -rf -- "$slot/target"
  fi
fi
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree remove --force "$slot" 2>/dev/null || true; git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e /target
"#;

pub(super) const VERIFY: &str = r#"landed=$(git -C "$slot" rev-parse HEAD)
[ "$landed" = "$head" ] || { echo "offload: refusing to run: $slot is on $landed, not the shipped $head" >&2; exit @WRONG_TREE@; }
proved() {
  untracked=$1
  set -- . ":(exclude,top)target"
  while IFS= read -r written; do [ -z "$written" ] || set -- "$@" ":(exclude,top,literal)$written"; done <<EOF
${generated:-}
EOF
  strays=$(git -C "$slot" status --porcelain "--untracked-files=$untracked" -- "$@" 2>&1) || strays="git status failed: $strays"
  at=$(git -C "$slot" rev-parse HEAD 2>/dev/null) || at=
  [ "$at" = "$head" ] || strays="HEAD is ${at:-unreadable}, not $head; $strays"
  why="$slot does not hold exactly the tree of $head: $(printf '%s\n' "$strays" | head -n 8 | tr '\n' ' ')"
  [ -z "$strays" ]
}
if ! proved all && [ -z "${nested:-}" ]; then
  echo "offload: $why; resetting it to $head" >&2
  git -C "$slot" reset -q --hard "$head" && git -C "$slot" clean -ffdxq -e /target || true
fi
proved all || { echo "offload: refusing to run: $why" >&2; exit @WRONG_TREE@; }
echo "offload: running $head in $slot"
proved_tree=$(git -C "$slot" rev-parse "HEAD^{tree}")
echo "@PROVED_TREE@$proved_tree of $head" >&2
"#;

const BESIDE: &str = r#"beside=$root/$s.beside
for pin in $pins; do
  project=${pin%%=*} sha=${pin#*=} source=$HOME/code/${pin%%=*}/main
  [ -e "$source/.git" ] || { echo "offload: this host keeps no $source for the check to read" >&2; exit @BESIDE_MISSING@; }
  git -C "$source" cat-file -e "$sha^{commit}" 2>/dev/null || git -C "$source" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$source" cat-file -e "$sha^{commit}" 2>/dev/null || { echo "offload: $source lacks $sha, the commit the check reads on the lander" >&2; exit @BESIDE_MISSING@; }
  if [ -e "$beside/$project/.git" ]; then
    git -C "$beside/$project" checkout -q --detach -f "$sha" || exit @BESIDE_MISSING@
  else
    mkdir -p "$beside" && git -C "$source" worktree prune && git -C "$source" worktree add -q --detach "$beside/$project" "$sha" || { echo "offload: cannot pin $beside/$project to $sha" >&2; exit @BESIDE_MISSING@; }
  fi
done
@BESIDE_ENV@=$beside; export @BESIDE_ENV@
"#;

const RUN: &str = r#"cd "$slot"
script_directory=$(mktemp -d "${root:-${TMPDIR:-/tmp}}/@SUBJECT_DIRECTORY@XXXXXX")
script=$script_directory/@SUBJECT_FILE@
printf '%s\n' "$command" >"$script"
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; export PATH; fi
if [ -n "${runtime:-}" ]; then
  runtime_dir=$HOME/.local/state/north/runtimes/$runtime
  [ -d "$runtime_dir" ] && { command exec 6>>"$runtime_dir/@LEASE@" || command exec 6<"$runtime_dir/@LEASE@"; } 2>/dev/null && flock -s 6 || { echo "offload: the lander runtime $runtime is not on this host" >&2; exit @TRANSPORT_FAILED@; }
  [ -x "$runtime_dir/north" ] && [ -f "$runtime_dir/@TREE_MARKER@" ] || { echo "offload: the lander runtime $runtime on this host lacks north or @TREE_MARKER@" >&2; exit @TRANSPORT_FAILED@; }
  PATH="$runtime_dir:$PATH"; export PATH
fi
@SLOT_ENV@=$slot @SLOT_HOLDER_ENV@=$$; export @SLOT_ENV@ @SLOT_HOLDER_ENV@
if [ -n "${carried:-}" ]; then eval "export $carried"; fi
lease_owner=land-offload
[ -z "${SAFE_PUSH_LAND_HEAD:-}" ] || lease_owner=land-check
leased() {
  if command -v north >/dev/null 2>&1; then @SUBJECT_ENV@="$script" @OWNER_ENV@="${owner:-}" north capacity host run --class heavy --owner "$lease_owner" --timeout-seconds "$limit" -- "$@"; else timeout -k 5 "$limit" "$@"; fi
}
if [ -n "${nested:-}" ]; then
  watching=$root/watch.$$.up
  rm -f "$watching"
  setsid sh -c @WATCH@ offload-watch "$holder" "$$" "$s" "$root" "$limit" </dev/null >/dev/null 2>&1 4>&- 6>&- 8>&- 9>&- &
  watcher=$! polls=0
  until [ -e "$watching" ]; do
    polls=$((polls + 1))
    if [ "$polls" -gt @WATCH_START_POLLS@ ]; then
      kill -KILL "$watcher" 2>/dev/null || true
      echo "offload: refusing to run: the watcher that ends this nested job with its check did not start in $s, so nothing would end the job if its check ended first" >&2
      exit @WRONG_TREE@
    fi
    sleep @DRAIN_POLL@
  done
fi
vouch() {
  if proved no; then
    [ -z "${proof:-}" ] || printf "%s %s\n" "$proof" "${proved_tree:-}" || true
  else
    echo "offload: the run's verdict is void, its tree changed under it: $why" >&2
    rc=@WRONG_TREE@
  fi
}
rc=0
if [ -z "${ran:-}" ]; then
  leased bash -euo pipefail "$script" </dev/null 4>&- 5>&- 6>&- 9>&- || rc=$?
  vouch
  exit "$rc"
fi
id=${ran#*:}
results=${root:-$HOME/.cache/land-offload}/results
mkdir -p "$results"
find "$results" -type f -mmin +1440 -delete 2>/dev/null || true
out=$results/$id.out err=$results/$id.err exited=$results/$id.exit lost=$results/$id.lost attached=$results/$id.attached
rm -f "$exited" "$lost" "$attached" "$results/$id.sent_out" "$results/$id.sent_err"
: >"$out"; : >"$err"
trap '' PIPE HUP
(
  rc=0
  leased bash -euo pipefail "$script" || rc=$?
  echo "$rc" >"$exited.part" && mv "$exited.part" "$exited"
  grace=${NORTH_OFFLOAD_DRAIN_GRACE_SECONDS:-@DRAIN_GRACE@} waited=0
  while [ "$waited" -lt "$grace" ] && kill -0 "$$" 2>/dev/null; do sleep 1; waited=$((waited + 1)); done
  if kill -0 "$$" 2>/dev/null; then echo "offload: ending the relay of run $id, whose check ended ${grace}s ago while its client read none of the rest" >&2; kill -TERM "-$$" 2>/dev/null || true; fi
) >"$out" 2>"$err" 4>&- 5>&- 6>&- 9>&- </dev/null &
@DRAIN@detached=
while :; do
  ended=; [ ! -f "$exited" ] || ended=1
  if [ -z "$detached" ]; then drain || { detached=1; echo >"$lost"; }; fi
  [ -z "$ended" ] || break
  if [ -n "$detached" ] && [ -n "$(find "$lost" -mmin +@REATTACH_MINUTES@ 2>/dev/null)" ] && [ -z "$(find "$attached" -mmin -1 2>/dev/null)" ]; then
    kill -TERM "-$$" 2>/dev/null || true
  fi
  sleep @DRAIN_POLL@
done
read -r rc <"$exited"
vouch
printf "%s %s\n" "$ran" "$rc" || true
exit "$rc"
"#;

/// A nested job's watcher, in a session of its own so neither a blocked write in the job's loop nor a TERM to the job's group stops it: it keeps the job's shared lock on the slot's `.nested` file, waits while the check that holds the slot lives, then ends the job's process group, TERM first and then KILL, and frees the slot only once no member of the group is left; a member that survives KILL keeps the slot taken and is reported once.
const WATCH: &str = r#"holder=$1 lead=$2 s=$3 root=$4 limit=$5
up=$root/watch.$lead.up
: >"$up"
deadline=$(($(date +%s) + limit + @WATCH_MARGIN@))
members() {
  for proc in /proc/[0-9]*; do
    read -r stat <"$proc/stat" 2>/dev/null || continue
    set -- ${stat##*) }
    [ "${3:-}" = "$lead" ] && [ "$1" != Z ] && [ "${proc#/proc/}" != "$$" ] && echo "${proc#/proc/}"
  done
  return 0
}
why="has ended, so the job ends with it"
while kill -0 "$holder" 2>/dev/null; do
  [ "$(date +%s)" -lt "$deadline" ] || { why="still runs $((limit + @WATCH_MARGIN@)) s after the job started, past its deadline, so the job ends"; break; }
  sleep @DRAIN_POLL@
done
log=$root/nested.log
printf "%s offload: the check that started the nested job %s in %s %s and reaches no verdict\n" "$(date +%FT%T)" "$lead" "$s" "$why" >>"$log"
trap '' TERM
kill -TERM "-$lead" 2>/dev/null || true
group=$root/watch.$$
waits=0 told=
while :; do
  members >"$group.left"
  [ -s "$group.left" ] || break
  waits=$((waits + 1))
  if [ "$waits" -ge @TERM_POLLS@ ]; then
    for left in $(cat "$group.left"); do kill -KILL "$left" 2>/dev/null || true; done
  fi
  if [ "$waits" -ge @KILL_POLLS@ ] && [ -z "$told" ]; then
    told=1 left=$(tr -s ' \n' ' ' <"$group.left")
    printf "%s offload: %s stays taken: processes %s of the nested job %s survive KILL\n" "$(date +%FT%T)" "$s" "$left" "$lead" >>"$log"
    north fleet send proxy "offload: slot $s on $(uname -n) stays taken: processes $left of a nested job whose check ended survive KILL" >/dev/null 2>&1 || true
  fi
  sleep @DRAIN_POLL@
done
rm -f "$group.left" "$up"
"#;

const DRAIN: &str = r#"drain() {
  for stream in out err; do
    f=$results/$id.$stream
    [ -f "$f" ] || continue
    sent=0
    [ ! -f "$results/$id.sent_$stream" ] || read -r sent <"$results/$id.sent_$stream" || sent=0
    size=$(wc -c <"$f")
    [ "$size" -gt "$sent" ] || continue
    if [ "$stream" = out ]; then
      { tail -c +$((sent + 1)) "$f" 2>/dev/null | head -c $((size - sent)); } || return 1
    else
      { tail -c +$((sent + 1)) "$f" 2>/dev/null | head -c $((size - sent)) >&2; } || return 1
    fi
    echo "$size" >"$results/$id.sent_$stream"
  done
}
"#;

pub(super) const REATTACH: &str = r#"ran=$1 limit=$2
root=@ROOT@
id=${ran#*:}
results=$root/results
exited=$results/$id.exit attached=$results/$id.attached
[ -f "$results/$id.out" ] || { echo "offload: this host holds no record of run $id" >&2; exit 1; }
@DRAIN@deadline=$(( $(date +%s) + limit + 60 ))
while :; do
  ended=; [ ! -f "$exited" ] || ended=1
  echo >"$attached"
  drain || exit 1
  if [ -n "$ended" ]; then
    read -r rc <"$exited"
    printf "%s %s\n" "$ran" "$rc"
    exit 0
  fi
  [ "$(date +%s)" -lt "$deadline" ] || { echo "offload: run $id never ended within its limit" >&2; exit 1; }
  sleep @DRAIN_POLL@
done
"#;

/// Which of a checkout's slots on the build host a job takes: a land check one of its `SLOTS`, a background job (a mutant sweep) the checkout's one background slot, queued behind every land check and never counted against their job cap, so an hours-long sweep holds no land check's slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotClass {
    LandCheck,
    Background,
}

impl SlotClass {
    pub(super) fn slot_name(self, checkout: &str) -> String {
        match self {
            Self::LandCheck => checkout.to_owned(),
            Self::Background => format!("{checkout}{BACKGROUND_SUFFIX}"),
        }
    }
}

pub(super) fn reattach_script() -> String {
    REATTACH.replace("@DRAIN@", DRAIN).replace("@DRAIN_POLL@", DRAIN_POLL).replace("@ROOT@", OFFLOAD_ROOT)
}

pub(super) fn run_part() -> String {
    RUN.replace("@WATCH@", &super::quote(WATCH))
        .replace("@DRAIN@", DRAIN)
        .replace("@REATTACH_MINUTES@", &REATTACH_MINUTES.to_string())
        .replace("@DRAIN_GRACE@", &DRAIN_GRACE_SECONDS.to_string())
        .replace("@DRAIN_POLL@", DRAIN_POLL)
        .replace("@TERM_POLLS@", &TERM_POLLS.to_string())
        .replace("@KILL_POLLS@", &KILL_POLLS.to_string())
        .replace("@WATCH_MARGIN@", &WATCH_MARGIN_SECONDS.to_string())
        .replace("@WATCH_START_POLLS@", &WATCH_START_POLLS.to_string())
        .replace("@SLOT_HOLDER_ENV@", north_base::foundation::offload::SLOT_HOLDER_ENV)
        .replace("@SUBJECT_ENV@", north_base::foundation::offload::SUBJECT_ENV)
        .replace("@SUBJECT_DIRECTORY@", north_base::foundation::offload::SUBJECT_DIRECTORY)
        .replace("@SUBJECT_FILE@", north_base::foundation::offload::SUBJECT_FILE)
        .replace("@OWNER_ENV@", north_base::foundation::offload::OWNER_ENV)
        .replace("@SLOT_ENV@", north_base::foundation::offload::SLOT_ENV)
        .replace("@LEASE@", crate::runtime::layout::LEASE)
        .replace("@TREE_MARKER@", &crate::runtime::layout::tree_marker())
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
        .replace("@WRONG_TREE@", &WRONG_TREE.to_string())
}

pub(crate) fn jobs_part() -> String {
    JOBS.replace("@LAND_SLOTS@", &SLOTS.to_string()).replace("@BACKGROUND_SUFFIX@", BACKGROUND_SUFFIX).replace("@WARM_LOCK@", super::super::warm::LOCK).replace("@SLOT_HELD@", north_base::foundation::offload::SLOT_HELD_SH)
}

pub(super) fn slot_part() -> String {
    format!("{REMOTE}{NESTED}{TAKE}fi\n").replace("@SLOT_ENV@", north_base::foundation::offload::SLOT_ENV).replace("@SLOT_RECORDS@", north_base::foundation::offload::SLOT_RECORDS).replace("@SLOT_HOLDER_ENV@", north_base::foundation::offload::SLOT_HOLDER_ENV).replace("@TREE_MARKER@", &crate::runtime::layout::tree_marker()).replace("@RUNTIME_MISSING@", &RUNTIME_MISSING.to_string())
}

pub(super) fn beside_part() -> String {
    BESIDE.replace("@BESIDE_MISSING@", &BESIDE_MISSING.to_string()).replace("@BESIDE_ENV@", beside::BESIDE_ENV)
}

/// Whether an offload slot directory is the lander's reserved one, the last of its checkout's `SLOTS`, which the lander's pre-push checks take first and every other job last.
pub fn is_lander_slot(slot: &str) -> bool {
    slot.strip_suffix(&format!("-{}", SLOTS - 1)).is_some_and(|checkout| !checkout.is_empty())
}

fn slot_name(name: &str, index: u32) -> String {
    if index == 0 { name.to_owned() } else { format!("{name}-{index}") }
}

/// One block of `SLOTS` sccache ports per checkout, picked by FNV-1a of its name, so the build host's script needs no hashing program.
pub(crate) fn slot_ports(name: &str) -> Vec<(String, u32)> {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3));
    let block = u32::try_from(hash % u64::from(SCCACHE_PORTS / SLOTS)).unwrap_or(0) * SLOTS;
    (0..SLOTS).map(|index| (slot_name(name, index), SCCACHE_FIRST_PORT + block + index)).collect()
}

fn settle_seconds() -> u64 {
    crate::capacity::pressure_target::settle_seconds().round() as u64
}

pub(super) fn remote_script(name: &str, class: SlotClass) -> String {
    let (slots, wait, background) = match class {
        SlotClass::LandCheck => (SLOTS, LOCK_WAIT_SECONDS, ""),
        SlotClass::Background => (1, BACKGROUND_WAIT_SECONDS, "1"),
    };
    let ports: String = slot_ports(name).iter().map(|(slot, port)| format!("{}) SCCACHE_SERVER_PORT={port} ;; ", quote(slot))).collect();
    format!("{}{VERIFY}{}{}", slot_part(), beside_part(), run_part())
        .replace("@JOBS@", &jobs_part())
        .replace("@ADMIT@", ADMIT)
        .replace("@SETTLE@", &settle_seconds().to_string())
        .replace("@SLOT_PORTS@", &ports)
        .replace("@ROOT@", OFFLOAD_ROOT)
        .replace("@LOCK_WAIT@", &wait.to_string())
        .replace("@POLL@", POLL_SECONDS)
        .replace("@SLOTS@", &slots.to_string())
        .replace("@BACKGROUND_SUFFIX@", BACKGROUND_SUFFIX)
        .replace("@BACKGROUND@", background)
        .replace("@TARGET_CAP@", &TARGET_CAP_MIB.to_string())
        .replace("@TARGET_FLOOR@", &TARGET_FLOOR_MIB.to_string())
        .replace("@TARGET_KEEP@", &TARGET_KEEP_MIB.to_string())
        .replace("@SLOTS_BUSY@", &SLOTS_BUSY.to_string())
        .replace("@WAITED@", WAITED)
        .replace("@WRONG_TREE@", &WRONG_TREE.to_string())
        .replace("@PROVED_TREE@", north_base::foundation::offload::PROVED_TREE)
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
}
