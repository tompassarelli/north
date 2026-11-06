use super::{BESIDE_MISSING, HEADROOM, LOCK_WAIT_SECONDS, OFFLOAD_ROOT, POLL_SECONDS, SCCACHE_FIRST_PORT, SCCACHE_PORTS, SLOTS, SLOTS_BUSY, TARGET_CAP_MIB, TARGET_FLOOR_MIB, TRANSPORT_FAILED, WAITED, WRONG_TREE, beside, quote};

const BACKGROUND_WAIT_SECONDS: u32 = 3600;
const BACKGROUND_SUFFIX: &str = ".background";

pub(super) const REMOTE: &str = r#"set -eu
repo=$HOME/$1 base=$2 head=$3 limit=$4 name=$5 command=$6 ref=$7 carried=${8:-} owner=${9:-} runtime=${10:-} per=${11:-0} red_fix=${12:-} ran=${13:-} pins=${14:-} lander_slot=${15:-}
root=@ROOT@
mkdir -p "$root"
bundle=$(mktemp "$root/$name.bundle.XXXXXX")
trap 'rm -f "$bundle"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
trap 'exit 143' HUP INT TERM
cat >"$bundle"
if ! git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1; then
  git -C "$repo" fetch -q --no-auto-maintenance origin 2>/dev/null || true
  git -C "$repo" bundle verify "$bundle" >/dev/null 2>&1 || { echo "offload: $repo lacks the commits under $base" >&2; exit 97; }
fi
git -C "$repo" fetch -q --no-auto-maintenance "$bundle" "+$ref:$ref"
nearest=$(
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
rank=2
case " $carried " in *" SAFE_PUSH_LANDING='1' "*) rank=1 ;; esac
[ -z "$red_fix" ] || rank=0
[ -z "@BACKGROUND@" ] || rank=3
ticket=$queue/$rank-$(date +%s%N)-$$@$name
exec 8>"$ticket"
flock -n 8
trap 'rm -f "$bundle" "$ticket"; git -C "$repo" update-ref -d "$ref" 2>/dev/null || true' EXIT
cores=$(nproc 2>/dev/null || echo 1)
usable=$cores
cap=${NORTH_OFFLOAD_MAX_JOBS:-}
if [ -z "$cap" ]; then
  cap=@SLOTS@
  usable=$((cores * @HEADROOM_NUM@ / @HEADROOM_DEN@))
  [ "$per" -le 1 ] || { cap=$((usable / per)); [ "$cap" -ge 1 ] || cap=1; [ "$cap" -le @SLOTS@ ] || cap=@SLOTS@; }
fi
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
    if [ $((now - ${stamp%%-*})) -gt 5000000000 ] && (exec 7>>"$f"; flock -n 7) 2>/dev/null; then rm -f "$f"; else return 1; fi
  done
  return 1
}
running_jobs() {
  n=0
  for l in "$root"/*.lock; do
    [ -e "$l" ] || continue
    case $l in *@BACKGROUND_SUFFIX@.lock) continue ;; esac
    (exec 7>>"$l"; flock -n 7) 2>/dev/null || n=$((n + 1))
  done
  echo "$n"
}
started=$(date +%s)
noted=0
while :; do
  if [ "$lander_slot" = 1 ]; then
    exec 9>"$root/$reserved.lock"
    if flock -n 9; then s=$reserved; break; fi
    exec 9>&-
  fi
  if first_in_line && { [ -n "@BACKGROUND@" ] || [ "$(running_jobs)" -lt "$cap" ]; }; then
    for s in $nearest; do
      exec 9>"$root/$s.lock"
      if flock -n 9; then break 2; fi
      exec 9>&-
    done
  fi
  waited=$(( $(date +%s) - started ))
  [ "$waited" -lt @LOCK_WAIT@ ] || { echo "offload: queued behind other $name jobs for @LOCK_WAIT@ s ($cap jobs run at once, @SLOTS@ slots)" >&2; exit @SLOTS_BUSY@; }
  [ "$waited" -lt "$noted" ] || { noted=$((waited + 60)); echo "offload: waiting for a slot for ${waited}s, in order, $cap jobs run at once; $(ls "$queue" | grep -c .) queued; held by: $(cat "$root/$name".holder "$root/$name"-*.holder 2>/dev/null | tr '\n' ';')" >&2; }
  sleep @POLL@
done
waited=$(( $(date +%s) - started ))
[ "$waited" -eq 0 ] || echo "@WAITED@${waited}s in line for $s" >&2
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
  rm -f "$bundle" "$root/$s.owner" "$root/$s.pgid" "$root/$s.holder"
  git -C "$repo" update-ref -d "$ref" 2>/dev/null || true
  if [ -e "$slot/.git" ]; then git -C "$slot" clean -ffdxq -e /target 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 143' HUP INT TERM
if [ -d "$slot/target" ]; then
  used=$(du -sm "$slot/target" | cut -f1)
  free=$(df -Pm "$root" | awk 'NR == 2 { print $4 }')
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
  share=$((${usable:-${cores:-1}} / cap)); [ "$share" -ge 1 ] || share=1
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
leased bash -euo pipefail -c "$command" 6>&- 9>&- || rc=$?
[ -z "${ran:-}" ] || printf "%s %s\n" "$ran" "$rc"
exit "$rc"
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

pub(super) fn run_part() -> String {
    RUN.replace("@SLOT_ENV@", crate::capacity::land_claim::OFFLOAD_SLOT_ENV)
        .replace("@LEASE@", crate::runtime::layout::LEASE)
        .replace("@TREE_MARKER@", &crate::runtime::layout::tree_marker())
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
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
pub(super) fn slot_ports(name: &str) -> Vec<(String, u32)> {
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
    format!("{REMOTE}{VERIFY}{}{}", beside_part(), run_part())
        .replace("@SLOT_PORTS@", &ports)
        .replace("@ROOT@", OFFLOAD_ROOT)
        .replace("@LOCK_WAIT@", &wait.to_string())
        .replace("@POLL@", POLL_SECONDS)
        .replace("@SLOTS@", &slots.to_string())
        .replace("@HEADROOM_NUM@", &HEADROOM.0.to_string())
        .replace("@HEADROOM_DEN@", &HEADROOM.1.to_string())
        .replace("@BACKGROUND_SUFFIX@", BACKGROUND_SUFFIX)
        .replace("@BACKGROUND@", background)
        .replace("@TARGET_CAP@", &TARGET_CAP_MIB.to_string())
        .replace("@TARGET_FLOOR@", &TARGET_FLOOR_MIB.to_string())
        .replace("@SLOTS_BUSY@", &SLOTS_BUSY.to_string())
        .replace("@WAITED@", WAITED)
        .replace("@WRONG_TREE@", &WRONG_TREE.to_string())
        .replace("@TRANSPORT_FAILED@", &TRANSPORT_FAILED.to_string())
}
