//! Spare workers on a build host: client units started before any task with the spec a worker launch there gets, each
//! waiting on its input with its hooks run and its tool server up, so a launch whose spec matches takes one in one round
//! trip instead of starting a client (north#1239). The pool is an owner-only directory on the build host with one entry
//! per spare, named by its kind (account, model, effort and directory) and its fingerprint (the spec without its
//! conversation and identity, and the runtime). A take claims an entry by renaming it, seals the session's binding in
//! the spare's held directory before the launch sends anything and gives the session the spare's spool emptied; a spare
//! nobody takes stops itself after `IDLE`, and its stop removes its entry, its held directory and its spool.
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

/// A spare a launch took: the held unit its link attaches to, and its conversation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Taken {
    pub running: Running,
    pub conversation: String,
}

/// What a fill reads of a build host: its load, threads and available memory, and the spares its pool holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pool {
    pub load: u64,
    pub threads: u64,
    pub memory_kb: u64,
    pub entries: Vec<String>,
}

/// Where a spare pool on `machine` lives, given the home layout shared with this host.
pub struct Place<'a> {
    pub ssh: &'a Path,
    pub machine: &'a Machine,
    pub home: &'a Path,
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
    digest(&serde_json::json!([argv, env, removed, spec.cwd, runtime]).to_string())
}

/// The kind a spare fills for: the account, model, effort and directory of the launches it may serve.
pub fn kind(account: &str, model: Option<&str>, effort: Option<&str>, cwd: &Path) -> String {
    digest(&serde_json::json!([account, model, effort, cwd]).to_string())
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

/// The launch a fill starts spares like `worker` from, under the spare's `name`: `worker` without its task, box, brief, prompt file, parent, placement, capacity need or compaction window, so a spare holds nothing of the worker it was modelled on and fits a fresh launch of that account, model, effort and directory.
pub fn template(worker: LaunchRequest, name: String) -> LaunchRequest {
    LaunchRequest { name, prompt: String::new(), system_prompt: None, parent: None, task_ref: None, claim_box: None, placement: None, capacity_need: None, role: Some(Role::Worker), resume: None, compact_window: None, ..worker }
}

/// The spare's spec: `spec` with the binding variable naming the file a take seals in its held directory.
pub fn spare_spec(mut spec: HostSpec, home: &Path, spare: &str) -> HostSpec {
    spec.env.push((BINDING_ENV.into(), remote::held_dir(home, &unit(spare)).join("bound/binding").into()));
    spec
}

/// Takes a spare of `fingerprint` for the session `name` and writes `binding` before returning it, or None when none fits or the host does not answer, and the launch starts its client as before. Run twice for one name, it returns the spare the first run took.
pub fn take(place: &Place, fingerprint: &str, name: &str, binding: &str) -> Option<Taken> {
    let script = format!(
        r#"pool={} spools={} fp={} name={} bound={} held={}
[ -d "$pool" ] && [ -O "$pool" ] && [ "$(stat -c %a "$pool")" = 700 ] || exit 1
for b in "$held"/north-spare-*.service/bound/binding; do
  [ -f "$b" ] && [ "$(cat "$b")" = "$bound" ] || continue
  d=${{b%/bound/binding}}; u=${{d##*/}}
  systemctl --user -q is-active "$u" && echo "$u $d $(cat "$d/nonce") $(cat "$d/conversation")" && exit 0
done
[ ! -e "$spools/$name" ] || rmdir "$spools/$name" 2>/dev/null || exit 1
for e in "$pool"/*-"$fp"-*; do
  t="$pool/.${{e##*/}}.$$"
  [ -f "$e" ] && mv "$e" "$t" 2>/dev/null || continue
  read -r unit dir nonce conv spare ident cwd < "$t"; rm -f "$t"
  if systemctl --user -q is-active "$unit" && [ "$(cat "$dir/nonce" 2>/dev/null)" = "$nonce" ] && [ "$(stat -c %d:%i:%W "$cwd" 2>/dev/null)" = "$ident" ] && {{ [ ! -d "$spools/$spare" ] || {{ mv -T "$spools/$spare" "$spools/$name" && find "$spools/$name" -mindepth 1 -delete; }}; }} && mkdir -m 700 "$dir/bound" && printf %s "$bound" > "$dir/bound/.binding" && chmod 400 "$dir/bound/.binding" && mv "$dir/bound/.binding" "$dir/bound/binding" && chmod 500 "$dir/bound" && echo {} > "$dir/limit"; then
    echo "$unit $dir $nonce $conv"; exit 0
  fi
  systemctl --user stop --no-block "$unit" 2>/dev/null
done
exit 1"#,
        quote(&pool(place.home)),
        quote(&spools(place.home)),
        quote(fingerprint),
        quote(name),
        quote(binding),
        quote(&place.home.join(".local/state/north/held").to_string_lossy()),
        TAKEN_DETACHED.as_secs()
    );
    let (code, out) = run(place, &script)?;
    let mut words = out.split_whitespace().map(str::to_owned);
    match (code, words.next(), words.next(), words.next(), words.next()) {
        (0, Some(unit), Some(dir), Some(nonce), Some(conversation)) => Some(Taken { running: Running { unit, dir, nonce }, conversation }),
        _ => None,
    }
}

/// Stops a spare a launch took and then could not use; it never held task text.
pub fn release(place: &Place, taken: &Taken) {
    let _ = run(place, &format!("systemctl --user stop --no-block {}", quote(&taken.running.unit)));
}

/// The build host's load and the live spares its pool holds, removing the entries of units that are gone and retiring at once any spare whose directory is gone or is no longer the one it started in, so its slot counts free; None when it does not answer.
pub fn survey(place: &Place) -> Option<Pool> {
    let script = format!(
        r#"pool={}
read -r l _ < /proc/loadavg; echo "${{l%%.*}} $(nproc) $(awk '/^MemAvailable:/ {{print $2}}' /proc/meminfo)"
[ -d "$pool" ] && [ -O "$pool" ] || exit 0
{drop}
for e in "$pool"/*; do
  [ -f "$e" ] || continue
  read -r unit _ _ _ spare ident cwd < "$e"
  systemctl --user -q is-active "$unit" || {{ rm -f "$e"; continue; }}
  [ -n "$cwd" ] && [ "$(stat -c {identity} "$cwd" 2>/dev/null)" = "$ident" ] || {{ drop "$e" "$unit" "$spare" "its directory $cwd is gone or is not the one it started in"; continue; }}
  echo "${{e##*/}}"
done"#,
        quote(&pool(place.home)),
        drop = drop_function(place.home),
        identity = IDENTITY
    );
    let (0, out) = run(place, &script)? else { return None };
    let mut lines = out.lines();
    let mut head = lines.next()?.split_whitespace().map(str::parse::<u64>);
    let (Some(Ok(load)), Some(Ok(threads)), Some(Ok(memory_kb))) = (head.next(), head.next(), head.next()) else { return None };
    Some(Pool { load, threads, memory_kb, entries: lines.map(str::to_owned).collect() })
}

/// The entries of `kind` a fill retires, because their fingerprint is not `fingerprint`, and how many spares it starts: up to [`PER_KIND`] of the kind and [`PER_HOST`] in all, and none while the host is at or over its thread count or under the memory floor.
pub fn wanted(pool: &Pool, kind: &str, fingerprint: &str) -> (Vec<String>, usize) {
    let of_kind = |entry: &&String| entry.starts_with(&format!("{kind}-"));
    let current = |entry: &&String| entry.starts_with(&format!("{kind}-{fingerprint}-"));
    let stale: Vec<String> = pool.entries.iter().filter(of_kind).filter(|entry| !current(entry)).cloned().collect();
    let held = pool.entries.len() - stale.len();
    let busy = pool.load >= pool.threads || pool.memory_kb < MEMORY_FLOOR_KB;
    let room = PER_KIND.saturating_sub(pool.entries.iter().filter(current).count()).min(PER_HOST.saturating_sub(held));
    (stale, if busy { 0 } else { room })
}

/// Stops the spares `entries` name; each one's stop removes its entry, held directory and spool.
pub fn retire(place: &Place, entries: &[String]) {
    if entries.is_empty() {
        return;
    }
    let pool = pool(place.home);
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
        quote(&pool(place.home)),
        quote(tree),
        drop = drop_function(place.home)
    );
    let (0, out) = run(place, &script)? else { return None };
    Some(out.lines().map(str::to_owned).collect())
}

/// Starts the spare `spare` of `kind` and `fingerprint` from `spec` (see [`spare_spec`]) with `conversation`, and enters it in the pool once its unit runs.
pub fn start(place: &Place, spec: &HostSpec, kind: &str, fingerprint: &str, spare: &str, conversation: &str) -> NorthResult<()> {
    let held = place.home.join(".local/state/north/held");
    let unit = unit(spare);
    let dir = remote::held_dir(place.home, &unit).to_string_lossy().into_owned();
    let pool = pool(place.home);
    let entry = format!("{pool}/{kind}-{fingerprint}-{spare}");
    let spool = format!("{}/{spare}", spools(place.home));
    let retire = format!(
        "[ -e {d}/bound ] && exit 0\nrm -f {entry}\nif [ -d {spool} ]; then find {spool} -mindepth 1 -delete; rmdir {spool}; fi\ncd {d} && rm -f in out err status attach feed nonce limit conversation retire && cd / && rmdir {d}\nexit 0\n",
        d = quote(&dir),
        entry = quote(&entry),
        spool = quote(&spool)
    );
    let nonce = new_uuid()?;
    let launch = remote::launch_script(spec, &held, &unit, &dir, &nonce, IDLE, &[format!("ExecStopPost=/bin/sh {dir}/retire")]);
    let cwd = quote(&spec.cwd.to_string_lossy());
    let script = format!(
        "{drop}\numask 077; ident=$(stat -c {IDENTITY} {cwd}) && mkdir -p {q_dir} {q_pool} && chmod 700 {q_pool} && printf %s {} > {q_dir}/retire && printf %s {} > {q_dir}/conversation && {{ {launch}; }} && printf '%s\\n' {}\" $ident \"{cwd} > {q_pool}/.new-{spare} && mv {q_pool}/.new-{spare} {entry_q} && {{ [ \"$(stat -c {IDENTITY} {cwd})\" = \"$ident\" ] || drop {entry_q} {unit_q} {spare} 'its directory was removed while it started'; }}",
        quote(&retire),
        quote(conversation),
        quote(&format!("{unit} {dir} {nonce} {conversation} {spare}")),
        drop = drop_function(place.home),
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
