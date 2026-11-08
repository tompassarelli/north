use super::{BESIDE_MISSING, LOCK_WAIT_SECONDS, RUNTIME_MISSING, OFFLOAD_ROOT, POLL_SECONDS, SCCACHE_FIRST_PORT, SCCACHE_PORTS, SLOTS, SLOTS_BUSY, TARGET_CAP_MIB, TARGET_FLOOR_MIB, TRANSPORT_FAILED, WAITED, WRONG_TREE, beside, job_mib, quote};

/// Minutes a run whose client lost its connection keeps going with no client reattached, so a caller killed on purpose does not leave the run holding its slot to its deadline; the runner reattaches within `REATTACH_WINDOW`, which is shorter.
const REATTACH_MINUTES: u32 = 3;
const DRAIN_POLL: &str = "0.1";
const BACKGROUND_WAIT_SECONDS: u32 = 3600;
const BACKGROUND_SUFFIX: &str = ".background";
const LANDER_SUFFIX: &str = ".lander";

/// The slots of a build host that jobs other than the lander's (lane pre-checks, worker builds) may hold at once, so the rest of its job cap stays free for the lander's probes and red fixes, which never wait behind them: on 2026-11-08 lane checks held at most one slot in 87% of the probes' slot readings, and a probe's 7 jobs need the other 4 of gex45's 5 and 3 of cpx62's 4.
pub const OTHER_JOBS: u32 = 1;

pub(super) const CAP: &str = r#"cores=${AGENT_CAPACITY_CORES:-$(nproc 2>/dev/null || echo 1)}
host_cap=${NORTH_OFFLOAD_MAX_JOBS:-}
if [ -z "$host_cap" ]; then
  host_cap=$cores
  [ "$per" -le 1 ] || host_cap=$((cores / per))
  memory=$(awk '/^MemTotal:/ { print int($2 / 1024) }' "${AGENT_CAPACITY_MEMINFO:-/proc/meminfo}" 2>/dev/null || echo 0)
  [ @JOB_MIB@ -eq 0 ] || [ "$memory" -lt @JOB_MIB@ ] || [ $((memory / @JOB_MIB@)) -ge "$host_cap" ] || host_cap=$((memory / @JOB_MIB@))
  [ "$host_cap" -ge 1 ] || host_cap=1
fi
cap=$host_cap
[ "$cap" -le @SLOTS@ ] || cap=@SLOTS@
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
    [ -z "${1:-}" ] || [ ! -e "${lock%.lock}@LANDER_SUFFIX@" ] || continue
    ! slot_held "$lock" || count=$((count + 1))
  done
  echo "$count"
}
other_jobs() {
  host_jobs others
}
"#;

pub(super) const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6 ref=$7 carried=${8:-} owner=${9:-} runtime=${10:-} per=${11:-0} red_fix=${12:-} ran=${13:-} pins=${14:-} lander_slot=${15:-}
root=@ROOT@
mkdir -p "$root"
if [ -n "$runtime" ] && { [ ! -x "$HOME/.local/state/north/runtimes/$runtime/north" ] || [ ! -f "$HOME/.local/state/north/runtimes/$runtime/@TREE_MARKER@" ]; }; then echo "offload: the lander runtime $runtime is not on this host yet" >&2; exit @RUNTIME_MISSING@; fi
bundle=$(mktemp "$root/$name.bundle.XXXXXX")
trap 'rm -f "$bundle"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
trap 'exit 143' HUP INT TERM
cat >"$bundle"
if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
  git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q --no-auto-maintenance "$bundle" "+$ref:$ref"
"#;

const NESTED: &str = r#"caller_slot=${@SLOT_ENV@:-} nested=
[ -n "@BACKGROUND@" ] || [ "${caller_slot%/*}" != "$root" ] || nested=1
if [ -n "$nested" ] && case ${caller_slot##*/} in "$name" | "$name"-[1-9]) true ;; *) false ;; esac && [ "$(git -C "$caller_slot" rev-parse HEAD 2>/dev/null)" = "$head" ]; then
  s=${caller_slot##*/} slot=$caller_slot
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
lander=
[ "$rank" -gt 1 ] || lander=1
ticket=$queue/$rank-$(date +%s%N)-$$@$name
[ -n "$nested" ] || { exec 8>"$ticket"; flock 8; }
trap 'rm -f "$bundle" "$ticket"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
@JOBS@@CAP@
others_full() {
  [ "$(other_jobs)" -ge @OTHER_JOBS@ ]
}
served=$root/served
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
  now=$(date +%s%N)
  for t in $(turn_order); do
    f=$queue/$t
    [ "$f" != "$ticket" ] || return 0
    stamp=${t#*-}
    if [ $((now - ${stamp%%-*})) -gt 5000000000 ] && (exec 7>>"$f"; flock -n 7) 2>/dev/null; then rm -f "$f"; continue; fi
    case $t in *@*) [ -n "@BACKGROUND@" ] || [ "${t#*@}" = "$name" ] || [ "$(slot_jobs "${t#*@}")" -lt "$cap" ] || continue ;; esac
    return 1
  done
  return 1
}
started=$(date +%s%N)
noted=0 polled=
while :; do
  if [ "$lander_slot" = 1 ]; then
    exec 9>"$root/$reserved.lock"
    if flock -n 9; then s=$reserved; break; fi
    exec 9>&-
  fi
  if [ -n "$nested" ] || { first_in_line && { [ -n "@BACKGROUND@" ] || { [ "$(slot_jobs "$name")" -lt "$cap" ] && [ "$(host_jobs)" -lt "$host_cap" ] && { [ -n "$lander" ] || ! others_full; }; }; }; }; then
    for s in $nearest; do
      exec 9>"$root/$s.lock"
      if flock -n 9; then break 2; fi
      exec 9>&-
    done
  fi
  waited=$(( ($(date +%s%N) - started) / 1000000000 ))
  [ "$waited" -lt @LOCK_WAIT@ ] || { echo "offload: queued behind other jobs for @LOCK_WAIT@ s ($name runs $cap at once in @SLOTS@ slots, the host $host_cap)" >&2; exit @SLOTS_BUSY@; }
  [ "$waited" -lt "$noted" ] || { noted=$((waited + 60)); echo "offload: waiting for a slot for ${waited}s, in order; $name runs $(slot_jobs "$name") of $cap jobs at once, the host $(host_jobs) of $host_cap; $(ls "$queue" | grep -c .) queued; held by: $(slot_holders "$name")" >&2; }
  polled=1
  sleep @POLL@
done
if [ -n "$lander" ]; then : >"$root/$s@LANDER_SUFFIX@"; else rm -f "$root/$s@LANDER_SUFFIX@"; fi
if [ -n "$polled" ]; then
  waited=$(( ($(date +%s%N) - started) / 1000000 ))
  echo "@WAITED@$((waited / 1000)).$((waited % 1000 / 100))s in line for $s" >&2
fi
exec 4>>"$root/$s.busy"
flock -s 4
date +%s%N >"$served/$name"
rm -f "$ticket"
exec 8>&-
slot=$root/$s
# sccache keys hold absolute paths, so every slot names its own directory as the base its keys are relative to, and serves it on its own port.
case $s in @SLOT_PORTS@ esac
SCCACHE_BASEDIRS=$slot; export SCCACHE_BASEDIRS SCCACHE_SERVER_PORT
printf '%s since %s\n' "$(printf '%.12s' "$head")" "$(date +%H:%M:%S)" >"$root/$s.holder"
printf '%s\n' "$owner" >"$root/$s.owner"
printf '%s\n' "$$" >"$root/$s.pgid"
cleanup() {
  [ -z "${exited:-}" ] || [ -e "$exited" ] || echo 143 >"$exited" 2>/dev/null || true
  rm -f "$bundle" "$root/$s.owner" "$root/$s.pgid" "$root/$s.holder" "$root/$s@LANDER_SUFFIX@"
  git -C "$repo" update-ref -d "$ref" 2>/dev/null || true
  if [ -e "$slot/.git" ]; then git -C "$slot" clean -ffdxq -e /target 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 143' HUP INT TERM
if [ -d "$slot/target" ]; then
  used=$(du -sm "$slot/target" | cut -f1)
  if [ -n "${AGENT_CAPACITY_DISK_FREE_GIB:-}" ]; then free=$(awk -v gib="$AGENT_CAPACITY_DISK_FREE_GIB" 'BEGIN { printf "%d", gib * 1024 }'); else free=$(df -Pm "$root" | awk 'NR == 2 { print $4 }'); fi
  if [ "$used" -gt "${NORTH_OFFLOAD_TARGET_CAP_MIB:-@TARGET_CAP@}" ] || [ "$free" -lt "${NORTH_OFFLOAD_TARGET_FLOOR_MIB:-@TARGET_FLOOR@}" ]; then
    echo "offload: removing $s/target ($used MiB, $free MiB free on the host)" >&2
    rm -rf -- "$slot/target"
  fi
fi
if [ -e "$slot/.git" ]; then git -C "$slot" checkout -q --detach -f "$head"; else git -C "$repo" worktree remove --force "$slot" 2>/dev/null || true; git -C "$repo" worktree add -q --detach "$slot" "$head"; fi
git -C "$slot" clean -ffdxq -e /target
"#;

pub(super) const VERIFY: &str = r#"landed=$(git -C "$slot" rev-parse HEAD)
[ "$landed" = "$head" ] || { echo "offload: refusing to run: $slot is on $landed, not the shipped $head" >&2; exit @WRONG_TREE@; }
echo "offload: running $head in $slot"
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
if [ -d "$HOME/.local/bin" ]; then PATH="$HOME/.local/bin:$PATH"; export PATH; fi
if [ -n "${runtime:-}" ]; then
  runtime_dir=$HOME/.local/state/north/runtimes/$runtime
  [ -d "$runtime_dir" ] && command exec 6>>"$runtime_dir/@LEASE@" && flock -s 6 || { echo "offload: the lander runtime $runtime is not on this host" >&2; exit @TRANSPORT_FAILED@; }
  [ -x "$runtime_dir/north" ] && [ -f "$runtime_dir/@TREE_MARKER@" ] || { echo "offload: the lander runtime $runtime on this host lacks north or @TREE_MARKER@" >&2; exit @TRANSPORT_FAILED@; }
  PATH="$runtime_dir:$PATH"; export PATH
fi
@SLOT_ENV@=$slot; export @SLOT_ENV@
if [ -n "${carried:-}" ]; then eval "export $carried"; fi
if [ -n "${cap:-}" ]; then
  share=$((${cores:-1} / cap)); [ "$share" -ge 1 ] || share=1
  CARGO_BUILD_JOBS=$share RUST_TEST_THREADS=$share NEXTEST_TEST_THREADS=$share; export CARGO_BUILD_JOBS RUST_TEST_THREADS NEXTEST_TEST_THREADS
  case ${SAFE_PUSH_LAND_CPUS:-} in "" | *[!0-9]*) SAFE_PUSH_LAND_CPUS=$share ;; *) [ "$SAFE_PUSH_LAND_CPUS" -le "$share" ] || SAFE_PUSH_LAND_CPUS=$share ;; esac
  export SAFE_PUSH_LAND_CPUS
fi
lease_owner=land-offload
[ -z "${SAFE_PUSH_LAND_HEAD:-}" ] || lease_owner=land-check
leased() {
  if command -v north >/dev/null 2>&1; then north capacity host run --class heavy --owner "$lease_owner" --timeout-seconds "$limit" -- "$@"; else timeout -k 5 "$limit" "$@"; fi
}
rc=0
if [ -z "${ran:-}" ]; then
  leased bash -euo pipefail -c "$command" 4>&- 6>&- 9>&- || rc=$?
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
  leased bash -euo pipefail -c "$command" || rc=$?
  echo "$rc" >"$exited.part" && mv "$exited.part" "$exited"
) >"$out" 2>"$err" 4>&- 6>&- 9>&- </dev/null &
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
printf "%s %s\n" "$ran" "$rc" || true
exit "$rc"
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
    RUN.replace("@DRAIN@", DRAIN)
        .replace("@REATTACH_MINUTES@", &REATTACH_MINUTES.to_string())
        .replace("@DRAIN_POLL@", DRAIN_POLL)
        .replace("@SLOT_ENV@", north_base::foundation::offload::SLOT_ENV)
        .replace("@LEASE@", crate::runtime::layout::LEASE)
        .replace("@TREE_MARKER@", &crate::runtime::layout::tree_marker())
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
}

pub(crate) fn jobs_part() -> String {
    JOBS.replace("@LAND_SLOTS@", &SLOTS.to_string()).replace("@BACKGROUND_SUFFIX@", BACKGROUND_SUFFIX).replace("@WARM_LOCK@", super::super::warm::LOCK).replace("@SLOT_HELD@", north_base::foundation::offload::SLOT_HELD_SH).replace("@LANDER_SUFFIX@", LANDER_SUFFIX)
}

pub(super) fn slot_part() -> String {
    format!("{REMOTE}{NESTED}{TAKE}fi\n").replace("@SLOT_ENV@", north_base::foundation::offload::SLOT_ENV).replace("@TREE_MARKER@", &crate::runtime::layout::tree_marker()).replace("@RUNTIME_MISSING@", &RUNTIME_MISSING.to_string()).replace("@LANDER_SUFFIX@", LANDER_SUFFIX).replace("@OTHER_JOBS@", &OTHER_JOBS.to_string())
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

pub(super) fn remote_script(name: &str, class: SlotClass) -> String {
    let (slots, wait, background) = match class {
        SlotClass::LandCheck => (SLOTS, LOCK_WAIT_SECONDS, ""),
        SlotClass::Background => (1, BACKGROUND_WAIT_SECONDS, "1"),
    };
    let ports: String = slot_ports(name).iter().map(|(slot, port)| format!("{}) SCCACHE_SERVER_PORT={port} ;; ", quote(slot))).collect();
    format!("{}{VERIFY}{}{}", slot_part(), beside_part(), run_part())
        .replace("@JOBS@", &jobs_part())
        .replace("@CAP@", CAP)
        .replace("@SLOT_PORTS@", &ports)
        .replace("@ROOT@", OFFLOAD_ROOT)
        .replace("@LOCK_WAIT@", &wait.to_string())
        .replace("@POLL@", POLL_SECONDS)
        .replace("@SLOTS@", &slots.to_string())
        .replace("@JOB_MIB@", &job_mib().to_string())
        .replace("@BACKGROUND_SUFFIX@", BACKGROUND_SUFFIX)
        .replace("@BACKGROUND@", background)
        .replace("@TARGET_CAP@", &TARGET_CAP_MIB.to_string())
        .replace("@TARGET_FLOOR@", &TARGET_FLOOR_MIB.to_string())
        .replace("@SLOTS_BUSY@", &SLOTS_BUSY.to_string())
        .replace("@WAITED@", WAITED)
        .replace("@WRONG_TREE@", &WRONG_TREE.to_string())
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
}
