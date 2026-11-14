//! Spare workers on a build host: client units started before any task with the spec a worker launch there gets, each
//! waiting on its input with its hooks run and its tool server up, so a launch whose spec matches takes one in one round
//! trip instead of starting a client (north#1239). The pool is an owner-only directory on the build host with one entry
//! per spare, named by its kind (account, model, effort and directory) and its fingerprint (the spec without its
//! conversation and identity, and the runtime). A take claims an entry by renaming it, seals the session's binding in
//! the spare's held directory before the launch sends anything and gives the session the spare's spool emptied; a spare
//! nobody takes stops itself after `IDLE`, and its stop removes its entry, its held directory and its spool.
//!
//! A launch into a lane (`CODE/PROJECT/worktrees/NAME`) is served by spares of its project's main checkout: each such
//! spare runs in a lane of its own made ahead at the project's base, detached. A take checks that both lanes are clean
//! and at one commit, switches the spare's lane to the launch's branch, removes the launch's own lane, and the session
//! then runs in the spare's lane; a lane at another commit is no fit, and the launch starts its client as before.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::error::{NorthError, NorthResult};
use crate::platform::process::try_run_command_within;

use super::link::quote;
use super::model::{LaunchRequest, Machine, Role};
use super::remote::{self, Running};
use super::runtime::{ACTOR_ENV, BINDING_ENV, CAPACITY_NEED_ENV, HostSpec, SESSION_ENV, new_uuid};
use super::store::{SESSIONS_DIR, SPOOL_DIR};
use super::ssh::SSH_WAIT;

pub const PER_KIND: usize = 3;
pub const PER_HOST: usize = 6;
const IDLE: Duration = Duration::from_secs(30 * 60);
const TAKEN_DETACHED: Duration = Duration::from_secs(12 * 60 * 60);
const POOL: &str = ".local/state/north/spares";
const RETIRED_LOG: &str = ".local/state/north/spares-retired.log";
const IDENTITY: &str = "%d:%i:%W";
const MEMORY_FLOOR_KB: u64 = 4 * 1024 * 1024;
const CHANNEL_WAIT: Duration = Duration::from_secs(3);
const BOUND: [&str; 3] = [SESSION_ENV, ACTOR_ENV, CAPACITY_NEED_ENV];
const HARDENED: &str = "-c core.hooksPath=/dev/null -c core.fsmonitor=false -c core.sshCommand=false";
const PINNED_GIT: &str = r#"pinned() { [ -d "$common" ] && [ -d "$common/worktrees/${1##*/}" ] && [ "$(cat "$1/.git" 2>/dev/null)" = "gitdir: $common/worktrees/${1##*/}" ]; }
g() { (w=$1; shift; cd "$w" && GIT_DIR="$common/worktrees/${w##*/}" GIT_COMMON_DIR="$common" GIT_WORK_TREE="$w" git $hardened "$@"); }
fits() {
  [ -z "$lane" ] && return 0
  pinned "$lane" && pinned "$1" || return 1
  head=$(g "$lane" symbolic-ref -q HEAD) || return 1
  case "$head" in refs/heads/?*) branch=${head#refs/heads/};; *) return 1;; esac
  GIT_DIR="$common" git $hardened show-ref --verify -q "refs/heads/$branch" || return 1
  [ "$(g "$lane" rev-parse --verify -q HEAD)" = "$(g "$1" rev-parse --verify -q HEAD)" ] && [ -z "$(g "$lane" status --porcelain --ignore-submodules=all)" ] && [ -z "$(g "$1" status --porcelain --ignore-submodules=all)" ] && g "$1" switch -q --ignore-other-worktrees --end-of-options "$branch"
}"#;

/// A spare a launch took: the held unit its link attaches to, its conversation, and the directory its client runs in, which is the session's from then on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Taken {
    pub running: Running,
    pub conversation: String,
    pub cwd: PathBuf,
}

/// The main checkout of the project whose lane `cwd` is, `CODE/PROJECT/main` for `CODE/PROJECT/worktrees/NAME`; None for any other directory.
pub fn lane_root(cwd: &Path) -> Option<PathBuf> {
    let worktrees = cwd.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "worktrees"))?;
    Some(worktrees.parent()?.join("main"))
}

/// The lane a fill makes ahead for the spare `spare` of the project whose main checkout is `root`.
pub fn spare_lane(root: &Path, spare: &str) -> PathBuf {
    root.with_file_name("worktrees").join(spare)
}

/// The directory a spare's kind and fingerprint name for `cwd`: its project's main checkout for a lane, so every lane of one project shares spares.
fn served(cwd: &Path) -> PathBuf {
    lane_root(cwd).unwrap_or_else(|| cwd.to_path_buf())
}

/// What a fill reads of a build host: its available memory, and the spares its pool holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    pub memory_kb: u64,
    pub entries: Vec<String>,
}

/// Where a spare pool on `machine` lives, given the home layout shared with this host.
pub struct Place<'a> {
    pub ssh: &'a Path,
    pub machine: &'a Machine,
    pub home: &'a Path,
}

impl Place<'_> {
    pub fn far_home(&self) -> PathBuf {
        self.machine.session_home(self.home).unwrap_or_else(|| self.home.to_path_buf())
    }
}

/// The fingerprint a spare and a launch must share: `spec` without its conversation id, its key and the variables a binding sets, with the runtime it starts from.
pub fn fingerprint(spec: &HostSpec, runtime: Option<&Path>) -> String {
    let mut argv = spec.argv.clone();
    if let Some(value) = argv.iter().position(|word| word == "--session-id").and_then(|at| argv.get_mut(at + 1)) {
        value.clear();
    }
    let mut env: Vec<(&str, String)> = spec.env.iter().filter(|(key, _)| !BOUND.contains(&key.as_str()) && key != BINDING_ENV).map(|(key, value)| (key.as_str(), value.to_string_lossy().into_owned())).collect();
    env.sort();
    let mut removed = spec.env_remove.clone();
    removed.sort();
    digest(&serde_json::json!([argv, env, removed, served(&spec.cwd), runtime]).to_string())
}

/// The kind a spare fills for: the account, model, effort, compaction window and directory of the launches it may serve.
pub fn kind(account: &str, model: Option<&str>, effort: Option<&str>, window: Option<u64>, cwd: &Path) -> String {
    digest(&serde_json::json!([account, model, effort, window, served(cwd)]).to_string())
}

/// A fresh spare's name, the session name its environment carries until a take binds it.
pub fn spare_name() -> NorthResult<String> {
    Ok(format!("spare-{}", new_uuid()?.replace('-', "").get(..12).unwrap_or_default()))
}

/// The runtime this host runs now, which a spare's fingerprint carries so a promotion retires it.
pub fn current_runtime() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty())?;
    std::fs::canonicalize(Path::new(&home).join(".local/state/north/current")).ok()
}

/// The launch a fill starts spares like `worker` from, under the spare's `name`: `worker` without its task, box, brief, prompt file, parent, placement or capacity need, so a spare holds nothing of the worker it was modelled on and fits a fresh launch of that account, model, effort, compaction window and directory.
pub fn template(worker: LaunchRequest, name: String) -> LaunchRequest {
    LaunchRequest { name, compact_window: worker.window(), prompt: String::new(), system_prompt: None, parent: None, task_ref: None, claim_box: None, placement: None, capacity_need: None, role: Some(Role::Worker), resume: None, ..worker }
}

/// The spare's spec: `spec` staged in the place's login home, with the binding variable naming the file a take seals in its held directory.
pub fn spare_spec(spec: HostSpec, place: &Place, spare: &str) -> HostSpec {
    let mut spec = remote::staged_on(spec, place.machine, place.home);
    spec.env.push((BINDING_ENV.into(), remote::held_dir(&place.far_home(), &unit(spare)).join("bound/binding").into()));
    spec
}

/// Takes a spare of `fingerprint` for the session `name` launching in `cwd` and writes `binding` before returning it, or None when none fits or the host does not answer, and the launch starts its client as before. For a launch into a lane, the spare's lane takes the lane's branch, and [`remove_left_lane`] removes the lane once the launch's first prompt is built from it; a spare whose lane does not fit stays in the pool. Every git the take runs reads its repository from the project's main checkout, never from a lane's `.git`, which a confined worker may write. Run twice for one name, it returns the spare the first run took.
pub fn take(place: &Place, fingerprint: &str, name: &str, cwd: &Path, binding: &str) -> Option<Taken> {
    let staged = |path: &Path| place.machine.session_path(path, place.home).to_string_lossy().into_owned();
    let (lane, common) = lane_root(cwd).map_or_else(Default::default, |main| (staged(cwd), staged(&main.join(".git"))));
    let script = format!(
        r#"pool={} spools={} fp={} name={} bound={} held={} lane={} common={} hardened='{HARDENED}'
[ -d "$pool" ] && [ -O "$pool" ] && [ "$(stat -c %a "$pool")" = 700 ] || exit 1
for b in "$held"/north-spare-*.service/bound/binding; do
  [ -f "$b" ] && [ "$(cat "$b")" = "$bound" ] || continue
  d=${{b%/bound/binding}}; u=${{d##*/}}
  systemctl --user -q is-active "$u" && echo "$u $d $(cat "$d/nonce") $(cat "$d/conversation") $(cat "$d/bound/cwd")" && exit 0
done
[ ! -e "$spools/$name" ] || rmdir "$spools/$name" 2>/dev/null || exit 1
{GIT}
for e in "$pool"/*-"$fp"-*; do
  t="$pool/.${{e##*/}}.$$"
  [ -f "$e" ] && mv "$e" "$t" 2>/dev/null || continue
  read -r unit dir nonce conv spare ident cwd < "$t"
  if ! {{ systemctl --user -q is-active "$unit" && [ "$(cat "$dir/nonce" 2>/dev/null)" = "$nonce" ] && [ "$(stat -c %d:%i:%W "$cwd" 2>/dev/null)" = "$ident" ]; }}; then
    rm -f "$t"; systemctl --user stop --no-block "$unit" 2>/dev/null; continue
  fi
  fits "$cwd" || {{ mv "$t" "$e"; continue; }}
  rm -f "$t"
  if {{ [ ! -d "$spools/$spare" ] || {{ mv -T "$spools/$spare" "$spools/$name" && find "$spools/$name" -mindepth 1 -delete; }}; }} && mkdir -m 700 "$dir/bound" && printf %s "$cwd" > "$dir/bound/cwd" && printf %s "$bound" > "$dir/bound/.binding" && chmod 400 "$dir/bound/.binding" "$dir/bound/cwd" && mv "$dir/bound/.binding" "$dir/bound/binding" && chmod 500 "$dir/bound" && echo {} > "$dir/limit"; then
    echo "$unit $dir $nonce $conv $cwd"; exit 0
  fi
  systemctl --user stop --no-block "$unit" 2>/dev/null
done
exit 1"#,
        quote(&pool(&place.far_home())),
        quote(&spools(&place.far_home())),
        quote(fingerprint),
        quote(name),
        quote(binding),
        quote(&place.far_home().join(crate::platform::session_host_files::HELD).to_string_lossy()),
        quote(&lane),
        quote(&common),
        TAKEN_DETACHED.as_secs(),
        GIT = PINNED_GIT
    );
    let (code, out) = run(place, &script)?;
    let mut words = out.split_whitespace().map(str::to_owned);
    match (code, words.next(), words.next(), words.next(), words.next(), words.next()) {
        (0, Some(unit), Some(dir), Some(nonce), Some(conversation), Some(cwd)) => Some(Taken { running: Running { unit, dir, nonce }, conversation, cwd: PathBuf::from(cwd) }),
        _ => None,
    }
}

/// Removes the lane `cwd` a launch that took a spare launched into, once its first prompt is built from it, reading the repository from the project's main checkout as the take does; the session runs in the spare's lane from the take on.
pub fn remove_left_lane(place: &Place, cwd: &Path) {
    let Some(main) = lane_root(cwd) else { return };
    let staged = |path: &Path| place.machine.session_path(path, place.home).to_string_lossy().into_owned();
    let _ = run(place, &format!("GIT_DIR={} git {HARDENED} worktree remove --force -- {} 2>/dev/null", quote(&staged(&main.join(".git"))), quote(&staged(cwd))));
}

/// Stops a spare a launch took and then could not use; it never held task text.
pub fn release(place: &Place, taken: &Taken) {
    let _ = run(place, &format!("systemctl --user stop --no-block {}", quote(&taken.running.unit)));
}

/// The build host's available memory and the live spares its pool holds, removing the entries of units that are gone and retiring at once any spare whose directory is gone or is no longer the one it started in, so its slot counts free; None when it does not answer.
pub fn survey(place: &Place) -> Option<Pool> {
    let script = format!(
        r#"pool={}
awk '/^MemAvailable:/ {{print $2}}' /proc/meminfo
[ -d "$pool" ] && [ -O "$pool" ] || exit 0
{drop}
for e in "$pool"/*; do
  [ -f "$e" ] || continue
  read -r unit _ _ _ spare ident cwd < "$e"
  systemctl --user -q is-active "$unit" || {{ rm -f "$e"; continue; }}
  [ -n "$cwd" ] && [ "$(stat -c {identity} "$cwd" 2>/dev/null)" = "$ident" ] || {{ drop "$e" "$unit" "$spare" "its directory $cwd is gone or is not the one it started in"; continue; }}
  echo "${{e##*/}}"
done"#,
        quote(&pool(&place.far_home())),
        drop = drop_function(&place.far_home()),
        identity = IDENTITY
    );
    let (0, out) = run(place, &script)? else { return None };
    let mut lines = out.lines();
    let memory_kb = lines.next()?.trim().parse().ok()?;
    Some(Pool { memory_kb, entries: lines.map(str::to_owned).collect() })
}

/// The entries of `kind` a fill retires, because their fingerprint is not `fingerprint`, and how many spares it starts: up to [`PER_KIND`] of the kind and [`PER_HOST`] in all, and none while the host is under the memory floor: a spare waits idle, so a loaded host still keeps them.
pub fn wanted(pool: &Pool, kind: &str, fingerprint: &str) -> (Vec<String>, usize) {
    let of_kind = |entry: &&String| entry.starts_with(&format!("{kind}-"));
    let current = |entry: &&String| entry.starts_with(&format!("{kind}-{fingerprint}-"));
    let stale: Vec<String> = pool.entries.iter().filter(of_kind).filter(|entry| !current(entry)).cloned().collect();
    let held = pool.entries.len() - stale.len();
    let busy = pool.memory_kb < MEMORY_FLOOR_KB;
    let room = PER_KIND.saturating_sub(pool.entries.iter().filter(current).count()).min(PER_HOST.saturating_sub(held));
    (stale, if busy { 0 } else { room })
}

/// Stops the spares `entries` name; each one's stop removes its entry, held directory and spool.
pub fn retire(place: &Place, entries: &[String]) {
    if entries.is_empty() {
        return;
    }
    let pool = pool(&place.far_home());
    let names: Vec<String> = entries.iter().map(|entry| quote(&format!("{pool}/{entry}"))).collect();
    let _ = run(place, &format!("for e in {}; do read -r unit _ < \"$e\" && systemctl --user stop --no-block \"$unit\"; done", names.join(" ")));
}

/// The shell function that retires one spare: it appends a line with the spare's name, its age and the reason to the host's retirement log (and prints it when `SPARE_SAY` is set), stops its unit, which ends its whole control group, and removes its entry.
fn drop_function(home: &Path) -> String {
    let log = quote(&home.join(RETIRED_LOG).to_string_lossy());
    format!(
        r#"drop() {{ now=$(date +%s); age=$((now - $(stat -c %Y "$1" 2>/dev/null || echo "$now"))); line="$(date -u +%Y-%m-%dT%H:%M:%SZ) $3 age ${{age}}s: retired unused, $4"; printf '%s\n' "$line" >> {log}; [ -z "$SPARE_SAY" ] || printf '%s\n' "$line"; systemctl --user stop "$2" 2>/dev/null; rm -f "$1"; }}"#
    )
}

/// Retires every spare whose directory is `tree` or under it on the host, whatever its kind, and returns one line for each (its name, its age and why); a spare outside the tree stays. None when the host does not answer or `tree` is not an absolute path.
pub fn retire_under(place: &Place, tree: &Path) -> Option<Vec<String>> {
    let tree = tree.to_string_lossy();
    let tree = tree.trim_end_matches('/');
    if !tree.starts_with('/') || tree.contains('\n') {
        return None;
    }
    let script = format!(
        r#"pool={} tree={} SPARE_SAY=1
{drop}
[ -d "$pool" ] && [ -O "$pool" ] || exit 0
for e in "$pool"/*; do
  [ -f "$e" ] || continue
  read -r unit _ _ _ spare _ cwd < "$e"
  case "$cwd" in "$tree"/*|"$tree") drop "$e" "$unit" "$spare" "its directory tree $tree was retired";; esac
done"#,
        quote(&pool(&place.far_home())),
        quote(tree),
        drop = drop_function(&place.far_home())
    );
    let (0, out) = run(place, &script)? else { return None };
    Some(out.lines().map(str::to_owned).collect())
}

/// Starts the spare `spare` of `kind` and `fingerprint` from `spec` (see [`spare_spec`]) with `conversation`, and enters it in the pool once its unit runs.
pub fn start(place: &Place, spec: &HostSpec, kind: &str, fingerprint: &str, spare: &str, conversation: &str) -> NorthResult<()> {
    let held = place.far_home().join(crate::platform::session_host_files::HELD);
    let unit = unit(spare);
    let dir = remote::held_dir(&place.far_home(), &unit).to_string_lossy().into_owned();
    let pool = pool(&place.far_home());
    let entry = format!("{pool}/{kind}-{fingerprint}-{spare}");
    let spool = format!("{}/{spare}", spools(&place.far_home()));
    let cwd = quote(&spec.cwd.to_string_lossy());
    let (make_lane, remove_lane) = lane_root(&spec.cwd).map_or_else(Default::default, |root| {
        let root = quote(&root.to_string_lossy());
        let git = format!("git -C {root} {HARDENED}");
        let base = format!("$({git} rev-parse -q --verify 'refs/remotes/origin/main^{{commit}}' || {git} rev-parse -q --verify 'refs/heads/main^{{commit}}')");
        (format!("{git} worktree add -q --detach {cwd} \"{base}\" && "), format!("{git} worktree remove --force {cwd}\n"))
    });
    let retire = format!(
        "[ -e {d}/bound ] && exit 0\nrm -f {entry}\nif [ -d {spool} ]; then find {spool} -mindepth 1 -delete; rmdir {spool}; fi\ncd {d} && rm -f in out err status attach feed nonce limit conversation retire && cd / && rmdir {d}\n{remove_lane}exit 0\n",
        d = quote(&dir),
        entry = quote(&entry),
        spool = quote(&spool)
    );
    let nonce = new_uuid()?;
    let launch = remote::launch_script(spec, &held, &remote::Starting { unit: &unit, dir: &dir, nonce: &nonce, conversation: Some(conversation) }, IDLE, &[format!("ExecStopPost=/bin/sh {dir}/retire")]);
    let script = format!(
        "{drop}\n{make_lane}umask 077; ident=$(stat -c {IDENTITY} {cwd}) && mkdir -p {q_dir} {q_pool} && chmod 700 {q_pool} && printf %s {} > {q_dir}/retire && {{ {launch}; }} && printf '%s\\n' {}\" $ident \"{cwd} > {q_pool}/.new-{spare} && mv {q_pool}/.new-{spare} {entry_q} && {{ [ \"$(stat -c {IDENTITY} {cwd})\" = \"$ident\" ] || drop {entry_q} {unit_q} {spare} 'its directory was removed while it started'; }}",
        quote(&retire),
        quote(&format!("{unit} {dir} {nonce} {conversation} {spare}")),
        drop = drop_function(&place.far_home()),
        entry_q = quote(&entry),
        unit_q = quote(&unit),
        q_dir = quote(&dir),
        q_pool = quote(&pool)
    );
    match plain(place, &script) {
        Some((0, _)) => Ok(()),
        Some((code, out)) => Err(NorthError::State(format!("starting spare {spare} on {} exited {code}: {}", place.machine.name, out.trim()))),
        None => Err(NorthError::State(format!("{} did not answer the start of spare {spare}", place.machine.name))),
    }
}

fn unit(spare: &str) -> String {
    format!("north-{spare}.service")
}

fn pool(home: &Path) -> String {
    home.join(POOL).to_string_lossy().into_owned()
}

fn spools(home: &Path) -> String {
    home.join(".local/state/north/fleet").join(SPOOL_DIR).join(SESSIONS_DIR).to_string_lossy().into_owned()
}

fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

fn run(place: &Place, script: &str) -> Option<(i32, String)> {
    let wrapped = format!("sh -c {}", quote(script));
    north_ssh_spare::run(place.machine.session_login(), &wrapped, CHANNEL_WAIT).or_else(|| plain(place, script))
}

fn plain(place: &Place, script: &str) -> Option<(i32, String)> {
    let mut ssh = Command::new(place.ssh);
    ssh.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", place.machine.session_login(), "--", &format!("sh -c {}", quote(script))]);
    let output = try_run_command_within(ssh, None, SSH_WAIT).ok().flatten()?;
    Some((output.status.code().unwrap_or(255), String::from_utf8_lossy(&output.stdout).into_owned()))
}

/// The binding a take writes for the session `name` with `capacity_need`: the variables [`BOUND`] names, one per line, which the worker-sandbox prefix applies.
pub fn binding(name: &str, capacity_need: Option<&str>) -> String {
    let pairs = [(SESSION_ENV, Some(name)), (ACTOR_ENV, Some(name)), (CAPACITY_NEED_ENV, capacity_need)];
    pairs.into_iter().filter_map(|(key, value)| value.filter(|value| !value.is_empty() && !value.contains('\n')).map(|value| format!("{key}={value}\n"))).collect()
}
