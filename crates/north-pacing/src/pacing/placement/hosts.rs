use super::super::storage::load;
use super::super::{process, storage};
use super::{line_wait, local};
use crate::capacity::saturation::{self, Saturation};
use serde_json::{Value, json};
use super::gate::Machine;
use toml_edit::DocumentMut;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::os::unix::process::CommandExt;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use north_strategy_types::placement::{LineWait, SpawnCpu};

pub(in crate::pacing) fn laptop() -> String {
    if let Ok(path) = std::env::var("LAPTOP_STATE") {
        return load(Path::new(&path))["state"]
            .as_str()
            .unwrap_or("unknown")
            .into();
    }
    state_of(&crate::laptop::Ctx::from_env())
}
fn state_of(ctx: &crate::laptop::Ctx) -> String {
    ctx.main(&["status".to_owned(), "--json".to_owned()])
        .ok()
        .and_then(|out| serde_json::from_str::<Value>(&out).ok())
        .and_then(|v| v["state"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}
/// Live resident agent CLIs on host `name`, asked through ssh `destination` unless it is this host;
/// None when unreachable.
pub(in crate::pacing) fn resident(name: &str, destination: &str) -> Option<i64> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        if fixture[name].is_object() {
            return fixture[name]["online"]
                .as_bool()
                .unwrap_or(false)
                .then(|| fixture[name]["live"].as_i64().unwrap_or(0));
        }
    }
    if name == local() {
        return live_count(&crate::resident_cap::status());
    }
    let result = process::run(
        "ssh",
        &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, ".local/state/north/current/tree/bin/north", "resident-cap", "status"],
        None,
        20.0,
    );
    result.ok().filter(|r| r.code == 0).and_then(|r| live_count(&r.out))
}

const SATURATION_CACHE_SECONDS: f64 = 60.0;

/// The spawn gate's CPU reading for a launch kept here; a PLACEMENT_HOSTS fixture run reads only the
/// SPAWN_CAPACITY_STATUS fixture, never this host's live capacity.
pub(in crate::pacing) fn here_spawn_cpu() -> Option<SpawnCpu> {
    if std::env::var_os("PLACEMENT_HOSTS").is_some() && std::env::var_os("SPAWN_CAPACITY_STATUS").is_none() {
        return None;
    }
    crate::pacing::spawn_gate::launch_capacity()
}

impl super::gate::Decision {
    /// Holds a box the launching host would keep when that host is saturated, or is closed to work that crowded machines turned away.
    pub fn hold_here(&mut self) {
        let (None, super::gate::Target::Account(_)) = (&self.machine, &self.target) else { return };
        let Some(why) = self.no_room_here() else { return };
        let why = self.crowded.iter().cloned().chain([why]).collect::<Vec<_>>().join("; ");
        self.machine_lines.push(why.clone());
        self.target = super::gate::Target::Held(why);
    }

    fn no_room_here(&self) -> Option<String> {
        let closed = self.here_closed.clone().filter(|_| !self.crowded.is_empty());
        self.here_saturated.clone().or(closed)
    }

    pub fn held_for_room(&self) -> bool {
        self.machine.is_none() && matches!(self.target, super::gate::Target::Held(_)) && self.no_room_here().is_some()
    }
}

impl super::gate::Machine {
    pub(in crate::pacing) fn saturation_refusal(&self) -> Option<String> {
        self.saturation?.refusal(&self.name)
    }
}

/// The host's saturation reading, from the PLACEMENT_HOSTS fixture alone when that is set, else the cache when under a
/// minute old, else a fresh probe here or over ssh `destination`; None when unreadable.
pub(in crate::pacing) fn saturation(name: &str, destination: &str) -> Option<Saturation> {
    let bound = super::sampling::config().ok().and_then(|cfg| {
        let setting = cfg.get("hosts")?.get(name)?.get("disk_free_gib")?;
        setting.as_float().or_else(|| setting.as_integer().map(|whole| whole as f64))
    });
    read_saturation(name, destination).map(|reading| reading.bounded(bound))
}

fn read_saturation(name: &str, destination: &str) -> Option<Saturation> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        let reading = &fixture[name]["saturation"];
        return Some(Saturation { memory_total_mib: reading["memory_total_mib"].as_f64()?, memory_available_mib: reading["memory_available_mib"].as_f64()?, disk_free_gib: reading["disk_free_gib"].as_f64()?, disk_total_gib: reading["disk_total_gib"].as_f64().unwrap_or(0.0), backlog: serde_json::from_value(reading["backlog"].clone()).ok(), token_age_s: reading["token_age_s"].as_u64(), disk_bound_gib: None });
    }
    let cache = storage::directory().join("saturation").join(format!("{name}.json"));
    let now = storage::now();
    let cached = load(&cache);
    if cached["at"].as_f64().is_some_and(|at| (0.0..SATURATION_CACHE_SECONDS).contains(&(now - at))) {
        return serde_json::from_value(cached["reading"].clone()).ok();
    }
    let reading = probe(name, destination)?;
    let _ = std::fs::create_dir_all(cache.parent()?).and_then(|()| std::fs::write(&cache, json!({"at": now, "reading": reading}).to_string()));
    Some(reading)
}

fn probe(name: &str, destination: &str) -> Option<Saturation> {
    let output = if name == local() {
        process::run("sh", &["-c", saturation::PROBE], None, 20.0)
    } else {
        ssh_script(destination, saturation::PROBE)
    };
    output.ok().filter(|r| r.code == 0).and_then(|r| saturation::parse(&r.out))
}

fn ssh_script(destination: &str, script: &str) -> Result<process::Output, String> {
    if let Some((code, out)) = north_ssh_spare::run(destination, script, SPARE_WAIT) {
        return Ok(process::Output { code, out, err: String::new() });
    }
    let words = ssh_words(destination, &["sh", "-s"]);
    let refs: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    process::run(&words[0], &refs, Some(script), 20.0)
}

/// The output of `script` run on `machine` over its operator login, whatever its exit status; None when the host did not answer.
pub(super) fn ask(machine: &Machine, script: &str) -> Option<String> {
    ssh_script(&machine.ssh, script).ok().map(|output| output.out)
}

/// `north ssh-spare hold DESTINATION`, keeping spare shells open to a build host over the same ssh command the survey uses.
pub(in crate::pacing) fn hold_spares(arguments: &[String]) -> u8 {
    north_ssh_spare::hold(arguments, |destination| ssh_words(destination, &[]).into_iter().map(std::ffi::OsString::from).collect())
}

fn ssh_words(destination: &str, command: &[&str]) -> Vec<String> {
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let options = [&["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination][..], command].concat();
    let argv = crate::platform::ssh::argv(&["ssh".into()], &env, &options);
    let mut words: Vec<String> = argv.iter().map(|word| word.to_string_lossy().into_owned()).collect();
    if let Some(at) = words.iter().position(|word| word == "-F") {
        words.drain(at..at + 2);
    }
    words
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(in crate::pacing) struct Facts {
    pub live: Option<i64>,
    pub accounts: Vec<String>,
    /// The accounts signed in through claude.ai there, the only ones `claude --cloud` starts under.
    pub claude_ai: Vec<String>,
    pub saturation: Option<Saturation>,
    /// Seconds since the saturation reading was taken, zero for a fresh probe.
    pub saturation_age_s: Option<f64>,
    /// Whether the launch directory asked about exists there.
    pub has_cwd: Option<bool>,
    /// Seconds since the host last answered, set only when this reading is its last recorded one because it did not answer in time.
    #[serde(default)]
    pub stale_age_s: Option<f64>,
}

const SURVEY_DEADLINE: Duration = Duration::from_secs(2);

const SPARE_WAIT: Duration = Duration::from_secs(20);

const STALE_LIMIT_SECONDS: f64 = 600.0;

const ACCOUNTS_CACHE_SECONDS: f64 = 600.0;

const SHARED_SECONDS: f64 = 30.0;

const DIRECTORY_SECONDS: f64 = 60.0;

type Memo = Mutex<Option<HashMap<String, Facts>>>;

static PASS: Memo = Mutex::new(None);

pub struct Pass;

impl Drop for Pass {
    fn drop(&mut self) {
        if let Ok(mut memo) = PASS.lock() {
            *memo = None;
        }
    }
}

pub fn pass() -> Pass {
    if let Ok(mut memo) = PASS.lock() {
        *memo = Some(HashMap::new());
    }
    Pass
}

fn remembered(memo: &Memo, key: String, fetch: impl FnOnce() -> Facts) -> Facts {
    if let Some(found) = memo.lock().ok().and_then(|held| held.as_ref()?.get(&key).cloned()) {
        return found;
    }
    let fetched = fetch();
    if let Ok(mut held) = memo.lock()
        && let Some(map) = held.as_mut()
    {
        map.insert(key, fetched.clone());
    }
    fetched
}

pub(in crate::pacing) fn probed_machines(machines: Vec<Machine>, fleet_load: &BTreeMap<String, i64>) -> Vec<Machine> {
    let facts = surveyed(&machines, None);
    machines.into_iter().zip(facts).map(|(machine, facts)| machine.probed(facts, fleet_load)).collect()
}

/// Each machine's facts in one ssh round trip apiece, asked at once; a host silent past `SURVEY_DEADLINE` is placed from its last recorded reading.
pub(in crate::pacing) fn surveyed(machines: &[Machine], cwd: Option<&str>) -> Vec<Facts> {
    survey_via(&PASS, &records(), machines, cwd, SURVEY_DEADLINE, Arc::new(fetch), &silent_host)
}

/// What a launch asks a host: everything placement reads, or only whether its directory is there when it reads another launch's answer; the survey keeper asks everything with a saturation reading taken now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    Everything,
    Directory,
    Refresh,
}

type Asker = dyn Fn(&str, &str, Option<&str>, Asked) -> Facts + Send + Sync;

type Ask = Arc<Asker>;

fn survey_via(memo: &'static Memo, records: &Path, machines: &[Machine], cwd: Option<&str>, deadline: Duration, ask: Ask, silent: &dyn Fn(&Machine) -> Facts) -> Vec<Facts> {
    let started = Instant::now();
    let waiting: Vec<_> = machines
        .iter()
        .map(|machine| {
            let (send, receive) = mpsc::channel();
            let (machine, cwd, ask, records) = (machine.clone(), cwd.map(str::to_owned), Arc::clone(&ask), records.to_path_buf());
            std::thread::spawn(move || {
                let key = format!("{}\0{}\0{}", machine.name, machine.session_login(), cwd.clone().unwrap_or_default());
                let _ = send.send(remembered(memo, key, || answered(&records, &machine, cwd.as_deref(), &ask)));
            });
            receive
        })
        .collect();
    machines
        .iter()
        .zip(waiting)
        .map(|(machine, receive)| {
            receive.recv_timeout(deadline.saturating_sub(started.elapsed())).unwrap_or_else(|_| silent(machine))
        })
        .collect()
}

/// The host's facts from an answer another launch recorded under `SHARED_SECONDS` ago, asking the host only for this launch's
/// directory unless it was seen there under `DIRECTORY_SECONDS` ago, which the survey keeper keeps true while launches place work;
/// else from the one launch that holds the host's survey turn, which launches starting meanwhile wait for and then read.
/// A launch that waits asks its directory while it waits and gives the turn back as soon as it has read the answer, so a burst's
/// directory questions run side by side instead of one per turn (five launches on 2026-11-12 waited up to 2.2 s before registering).
fn answered(records: &Path, machine: &Machine, cwd: Option<&str>, ask: &Ask) -> Facts {
    let directory = || match cwd {
        Some(dir) if seen_there(records, &machine.name, dir, storage::now()) => Some(true),
        Some(dir) => {
            let found = ask(&machine.name, machine.session_login(), cwd, Asked::Directory).has_cwd;
            if found == Some(true) {
                mark_there(records, &machine.name, dir, true);
            }
            found
        }
        None => None,
    };
    let reused = |facts: Facts, has_cwd: Option<bool>| if cwd.is_some() { Facts { has_cwd, ..facts } } else { facts };
    let everything = || ask(&machine.name, machine.session_login(), cwd, Asked::Everything);
    if let Some(facts) = shared(records, &machine.name, storage::now()) {
        return reused(facts, directory());
    }
    let Some(turn) = turn_file(records, &machine.name) else { return everything() };
    if turn.try_lock().is_ok() {
        return match shared(records, &machine.name, storage::now()) {
            Some(facts) => {
                drop(turn);
                reused(facts, directory())
            }
            None => everything(),
        };
    }
    std::thread::scope(|scope| {
        let early = scope.spawn(directory);
        match turn.lock().ok().and_then(|()| shared(records, &machine.name, storage::now())) {
            Some(facts) => {
                drop(turn);
                reused(facts, early.join().ok().flatten())
            }
            None => everything(),
        }
    })
}

/// The answer recorded under `SHARED_SECONDS` ago, its saturation reading still under `SATURATION_CACHE_SECONDS` old; an answer
/// that held no live count or no saturation reading is asked again.
fn shared(records: &Path, name: &str, now: f64) -> Option<Facts> {
    let recorded = load(&records.join(format!("{name}.json")));
    let age = recorded["at"].as_f64().map(|at| now - at).filter(|age| (0.0..SHARED_SECONDS).contains(age))?;
    let mut facts = serde_json::from_value::<Facts>(recorded["facts"].clone()).ok()?;
    facts.saturation_age_s = Some(facts.saturation_age_s? + age);
    (facts.live.is_some() && facts.saturation.is_some() && facts.saturation_age_s.is_some_and(|aged| aged < SATURATION_CACHE_SECONDS)).then_some(facts)
}

/// Asks `machine` everything placement reads and whether each launch directory it was seen to hold is still there, recording
/// both, so launches placing work read an answer under `SHARED_SECONDS` old instead of asking it themselves.
/// Refreshes every build host the orchestration file names, side by side.
pub(super) fn refresh_every_host() {
    let Ok(text) = std::fs::read_to_string(crate::platform::orchestration::path()) else { return };
    let Ok(cfg) = text.parse::<toml_edit::DocumentMut>() else { return };
    let machines = super::gate::machines(&cfg);
    std::thread::scope(|scope| {
        for machine in &machines {
            scope.spawn(|| refresh(machine));
        }
    });
}

fn refresh(machine: &Machine) {
    let records = records();
    fetch(&machine.name, machine.session_login(), None, Asked::Refresh);
    let here = machine.name == local();
    let seen = load(&records.join(format!("{}.dirs.json", machine.name)));
    for directory in seen.as_object().into_iter().flat_map(|entries| entries.keys()) {
        if let Some(present) = directory_there(here, machine.session_login(), directory) {
            mark_there(&records, &machine.name, directory, present);
        }
    }
}

fn seen_there(records: &Path, name: &str, directory: &str, now: f64) -> bool {
    load(&records.join(format!("{name}.dirs.json")))[directory].as_f64().is_some_and(|at| (0.0..DIRECTORY_SECONDS).contains(&(now - at)))
}

/// Records whether `directory` is on the host, under the record's own lock so two launches noting at once lose neither entry.
fn mark_there(records: &Path, name: &str, directory: &str, present: bool) {
    let Some(lock) = std::fs::create_dir_all(records).and_then(|()| std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(records.join(format!("{name}.dirs.lock")))).ok() else { return };
    if lock.lock().is_err() {
        return;
    }
    let path = records.join(format!("{name}.dirs.json"));
    let mut entries = match load(&path) {
        Value::Object(entries) => entries,
        _ => serde_json::Map::new(),
    };
    let now = storage::now();
    entries.retain(|_, at| at.as_f64().is_some_and(|at| (0.0..DIRECTORY_SECONDS).contains(&(now - at))));
    if present {
        entries.insert(directory.to_owned(), json!(now));
    } else {
        entries.remove(directory);
    }
    let _ = std::fs::write(&path, Value::Object(entries).to_string());
}

fn turn_file(records: &Path, name: &str) -> Option<std::fs::File> {
    std::fs::create_dir_all(records).ok()?;
    std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(records.join(format!("{name}.lock"))).ok()
}

pub(super) fn records() -> std::path::PathBuf {
    storage::directory().join("survey")
}

const LINE_FRESH_SECONDS: f64 = 45.0;

/// Every `[hosts.NAME]` machine with `offload_builds = true`, which the survey keeper keeps a landing-line reading of.
fn every_build_host(cfg: &DocumentMut, machines: Vec<Machine>) -> Vec<Machine> {
    machines.into_iter().filter(|machine| line_wait::takes_builds(cfg, &machine.name)).collect()
}

/// The build hosts a landing from `here` can use, `here` among them when it takes builds.
pub fn build_hosts(cfg: &DocumentMut, here: &str, machines: Vec<Machine>) -> Vec<Machine> {
    let landing = line_wait::landing_hosts(cfg, here);
    every_build_host(cfg, machines).into_iter().filter(|machine| landing.contains(&machine.name)).collect()
}

/// Each build host's expected wait for a landing slot, read in parallel through `ask`; empty when the switch is off.
pub fn read_lines(cfg: &DocumentMut, here: &str, machines: Vec<Machine>, ask: &(dyn Fn(&Machine) -> Option<u64> + Sync)) -> Vec<LineWait> {
    if !line_wait::enabled() {
        return Vec::new();
    }
    let hosts = build_hosts(cfg, here, machines);
    std::thread::scope(|scope| {
        let reads: Vec<_> = hosts.iter().map(|machine| scope.spawn(move || ask(machine))).collect();
        hosts.iter().zip(reads).map(|(machine, read)| LineWait { host: machine.name.clone(), seconds: read.join().ok().flatten() }).collect()
    })
}

/// The host's expected wait from the reading recorded under `LINE_FRESH_SECONDS` ago, else asked of the host and recorded, a host that
/// did not answer recorded as unread too; the survey keeper asks with `refresh` so a launch finds a recorded reading. A PLACEMENT_HOSTS
/// fixture run reads only the fixture's `line_wait_s`.
fn line_reading(machine: &Machine, refresh: bool) -> Option<u64> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        return load(Path::new(&path))[machine.name.as_str()]["line_wait_s"].as_u64();
    }
    let record = records().join(format!("{}.line.json", machine.name));
    let now = storage::now();
    let recorded = load(&record);
    if !refresh && recorded["at"].as_f64().is_some_and(|at| (0.0..LINE_FRESH_SECONDS).contains(&(now - at))) {
        return recorded["seconds"].as_u64();
    }
    let asked = if machine.name == local() {
        process::run("sh", &["-c", &format!("cd \"$HOME\" && {}", line_wait::ASK)], None, 20.0).ok().map(|output| output.out)
    } else {
        ask(machine, line_wait::ASK)
    };
    let seconds = asked.as_deref().and_then(line_wait::parse);
    let _ = std::fs::create_dir_all(records()).and_then(|()| std::fs::write(&record, json!({"at": now, "seconds": seconds}).to_string()));
    seconds
}

/// The readings the survey keeper recorded, each asked of its host only when none is under `LINE_FRESH_SECONDS` old.
pub fn surveyed_lines(cfg: &DocumentMut, here: &str, machines: Vec<Machine>) -> Vec<LineWait> {
    read_lines(cfg, here, machines, &|machine| line_reading(machine, false))
}

/// Asks every build host its wait and records it, side by side: the survey keeper's turn.
pub(super) fn refresh_every_line() {
    if !line_wait::enabled() {
        return;
    }
    let Ok(text) = std::fs::read_to_string(crate::platform::orchestration::path()) else { return };
    let Ok(cfg) = text.parse::<DocumentMut>() else { return };
    let hosts = every_build_host(&cfg, super::gate::machines(&cfg));
    std::thread::scope(|scope| {
        for machine in &hosts {
            scope.spawn(|| line_reading(machine, true));
        }
    });
}

fn record_path(name: &str) -> std::path::PathBuf {
    records().join(format!("{name}.json"))
}

fn record(records: &Path, name: &str, facts: &Facts) {
    let _ = std::fs::create_dir_all(records).and_then(|()| std::fs::write(records.join(format!("{name}.json")), json!({"at": storage::now(), "facts": facts}).to_string()));
}

/// The host's last answered facts marked stale with their age, saying nothing of a launch directory it was not asked about;
/// nothing known when none is on record or it is older than `STALE_LIMIT_SECONDS`.
fn last_recorded(records: &Path, name: &str) -> Facts {
    let recorded = load(&records.join(format!("{name}.json")));
    let age = recorded["at"].as_f64().map(|at| storage::now() - at).filter(|age| (0.0..=STALE_LIMIT_SECONDS).contains(age));
    let Some((age, mut facts)) = age.zip(serde_json::from_value::<Facts>(recorded["facts"].clone()).ok()) else {
        return Facts::default();
    };
    facts.saturation_age_s = facts.saturation_age_s.map(|taken| taken + age);
    facts.has_cwd = None;
    facts.stale_age_s = Some(age);
    facts
}

fn silent_host(machine: &Machine) -> Facts {
    recycle_master(machine);
    last_recorded(&records(), &machine.name)
}

fn recycle_master(machine: &Machine) {
    if std::env::var_os("PLACEMENT_HOSTS").is_some() || machine.name == local() {
        return;
    }
    let Ok(program) = std::env::current_exe() else { return };
    let _ = std::process::Command::new(program)
        .args(["ssh-master", "probe", &machine.ssh])
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

impl Machine {
    /// The machine with its probed facts, its live count raised by the fleet sessions this machine runs there.
    pub(in crate::pacing) fn probed(self, facts: Facts, fleet_load: &BTreeMap<String, i64>) -> Self {
        let hosted = fleet_load.get(&self.name).copied().unwrap_or(0);
        Self { live: facts.live.map(|live| live + hosted), accounts: facts.accounts, claude_ai: facts.claude_ai, saturation: facts.saturation, stale_age_s: facts.stale_age_s, ..self }
    }
}

fn fetch(name: &str, destination: &str, cwd: Option<&str>, asked: Asked) -> Facts {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let has_cwd = cwd.map(|_| load(Path::new(&path))[name]["has_cwd"].as_bool().unwrap_or(false));
        let saturation = saturation(name, destination);
        return Facts { live: resident(name, destination), accounts: signed_in(name, destination, false), claude_ai: signed_in(name, destination, true), saturation_age_s: saturation.map(|_| 0.0), saturation, has_cwd, stale_age_s: None };
    }
    let here = name == local();
    if asked == Asked::Directory {
        return Facts { has_cwd: cwd.and_then(|directory| directory_there(here, destination, directory)), ..Facts::default() };
    }
    let cache = storage::directory().join("saturation").join(format!("{name}.json"));
    let now = storage::now();
    let cached = load(&cache);
    let fresh = asked != Asked::Refresh && cached["at"].as_f64().is_some_and(|at| (0.0..SATURATION_CACHE_SECONDS).contains(&(now - at)));
    let mut facts = Facts { saturation: fresh.then(|| serde_json::from_value(cached["reading"].clone()).ok()).flatten(), ..Facts::default() };
    facts.saturation_age_s = facts.saturation.and(cached["at"].as_f64()).map(|at| now - at);
    let want_saturation = !fresh && !here;
    let known = recorded_accounts(name, now);
    let reply = ssh_script(destination, &script(!here, want_saturation, known.is_none(), cwd));
    if let Some(failure) = probe_failure(destination, &reply).filter(|_| !here && announce_once(destination)) {
        eprintln!("north: {failure}");
    }
    let answer = reply.ok().filter(|r| r.code == 0).map(|r| r.out);
    if let Some(sections) = answer.as_deref().map(sections) {
        let section = |label: &str| sections.iter().find(|(name, _)| name == label).map(|(_, body)| body.as_str());
        facts.live = section("live").and_then(live_count);
        let (accounts, claude_ai) = known.unwrap_or_else(|| (section("accounts").map(claude_labels).unwrap_or_default(), section("claude_ai").map(claude_labels).unwrap_or_default()));
        facts.accounts = accounts;
        facts.claude_ai = claude_ai;
        facts.has_cwd = cwd.map(|_| section("cwd").is_some());
        if want_saturation {
            facts.saturation = section("saturation").and_then(saturation::parse);
        }
    }
    if here {
        facts.live = live_count(&crate::resident_cap::status());
    }
    if here && !fresh {
        facts.saturation = probe(name, destination);
    }
    if !fresh {
        facts.saturation_age_s = facts.saturation.map(|_| 0.0);
    }
    if !fresh && let (Some(reading), Some(dir)) = (facts.saturation, cache.parent()) {
        let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&cache, json!({"at": now, "reading": reading}).to_string()));
    }
    if answer.is_some() || here {
        record(&records(), name, &Facts { has_cwd: None, stale_age_s: None, ..facts.clone() });
    }
    facts
}

fn directory_there(here: bool, destination: &str, directory: &str) -> Option<bool> {
    if here {
        return Some(Path::new(directory).is_dir());
    }
    let answer = ssh_script(destination, &script(false, false, false, Some(directory))).ok().filter(|r| r.code == 0);
    answer.map(|r| sections(&r.out).iter().any(|(label, _)| label == "cwd"))
}

const RESIDENT_BINARY: &str = ".local/state/north/current/tree/bin/north";

/// The signed-in accounts the last answer recorded when it is under `ACCOUNTS_CACHE_SECONDS` old; they change when a person signs in, not per launch.
fn recorded_accounts(name: &str, now: f64) -> Option<(Vec<String>, Vec<String>)> {
    let recorded = load(&record_path(name));
    recorded["at"].as_f64().filter(|at| (0.0..ACCOUNTS_CACHE_SECONDS).contains(&(now - at)))?;
    let facts = serde_json::from_value::<Facts>(recorded["facts"].clone()).ok()?;
    (!facts.accounts.is_empty()).then_some((facts.accounts, facts.claude_ai))
}

fn script(live: bool, saturation: bool, accounts: bool, cwd: Option<&str>) -> String {
    let mut parts = Vec::new();
    if live {
        parts.push(format!("echo '@@live'; {RESIDENT_BINARY} resident-cap status; echo \"@@rc $?\""));
    }
    if accounts {
        parts.push("echo '@@accounts'; north fleet accounts 2>&1; echo \"@@rc $?\"".to_owned());
        parts.push("echo '@@claude_ai'; north fleet accounts --claude-ai 2>&1; echo \"@@rc $?\"".to_owned());
    }
    if saturation {
        parts.push(format!("echo '@@saturation'; {}; echo \"@@rc $?\"", saturation::PROBE));
    }
    if let Some(directory) = cwd {
        parts.push(format!("echo '@@cwd'; test -d '{}'; echo \"@@rc $?\"", directory.replace('\'', r"'\''")));
    }
    parts.join("\n")
}

fn blocks(output: &str) -> Vec<(String, i32, String)> {
    let mut found = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in output.split_inclusive('\n') {
        let marker = line.trim_end();
        if let Some(label) = marker.strip_prefix("@@").filter(|label| !label.starts_with("rc ")) {
            open = Some((label.to_owned(), String::new()));
        } else if let Some(code) = marker.strip_prefix("@@rc ") {
            if let Some((label, body)) = open.take() {
                found.push((label, code.parse().unwrap_or(-1), body));
            }
        } else if let Some((_, body)) = open.as_mut() {
            body.push_str(line);
        }
    }
    found
}

fn announce_once(destination: &str) -> bool {
    static ANNOUNCED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    ANNOUNCED.lock().map_or(true, |mut seen| seen.insert(destination.to_owned()))
}

fn sections(output: &str) -> Vec<(String, String)> {
    blocks(output).into_iter().filter(|(_, code, _)| *code == 0).map(|(label, _, body)| (label, body)).collect()
}

/// Why a placement probe of `destination` got no usable answer, quoting the first line the host printed so a control byte cannot reach the terminal.
fn probe_failure(destination: &str, reply: &Result<process::Output, String>) -> Option<String> {
    let first = |text: &str| text.lines().next().unwrap_or_default().trim().to_owned();
    match reply {
        Err(why) => Some(format!("probe of {destination} failed: {:?}", first(why))),
        Ok(run) if run.code != 0 => Some(format!("probe of {destination} failed: exit {}: {:?}", run.code, first(&run.err))),
        Ok(run) => blocks(&run.out).into_iter().find(|(label, code, _)| matches!(label.as_str(), "accounts" | "claude_ai") && *code != 0).map(|(_, code, body)| format!("probe of {destination} failed: exit {code}: {:?}", first(&body))),
    }
}

pub(in crate::pacing) fn signed_in(name: &str, destination: &str, claude_ai: bool) -> Vec<String> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        if fixture[name].is_object() {
            return fixture[name][if claude_ai { "claude_ai" } else { "accounts" }].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        }
    }
    let mut argv = vec!["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north", "fleet", "accounts"];
    argv.extend(claude_ai.then_some("--claude-ai"));
    let listed = process::run("ssh", &argv, None, 20.0);
    listed.ok().filter(|r| r.code == 0).map(|r| claude_labels(&r.out)).unwrap_or_default()
}

fn claude_labels(listing: &str) -> Vec<String> {
    listing.lines().filter_map(|line| line.strip_prefix("claude\t")?.split('\t').next()).map(|label| label.trim().to_owned()).collect()
}

/// The live count from `north resident-cap status`, which prints `HOST: LIVE/CAP resident agent CLIs`.
fn live_count(status: &str) -> Option<i64> {
    let (_, rest) = status.split_once(':')?;
    rest.split_whitespace().next()?.split('/').next()?.parse().ok()
}

#[cfg(test)]
#[path = "hosts_tests.rs"]
mod tests;
