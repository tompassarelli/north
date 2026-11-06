use super::super::storage::load;
use super::super::{process, storage, target};
use super::{local, moment, operator, stamp};
use crate::capacity::saturation::{self, Saturation};
use serde_json::{Value, json};
use super::gate::Machine;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;
use north_strategy_types::placement::{Credit, SpawnCpu};
use toml_edit::{DocumentMut, Item};

pub(in crate::pacing) fn credit(cfg: &DocumentMut, at: f64) -> (bool, String) {
    let name = credit_name(cfg);
    credit_verdict(&credit_case(&name), &name, at)
}

pub(in crate::pacing) fn credit_name(cfg: &DocumentMut) -> String {
    cfg.get("placement").and_then(|p| p.get("cloud_credit")).and_then(Item::as_str).unwrap_or_default().to_owned()
}

/// The `costs.toml` service `name` names and its spend readout, as placement records them.
pub(in crate::pacing) fn credit_case(name: &str) -> Credit {
    let Some(service) = credit_service(name) else { return Credit::default() };
    let path = storage::directory().join("costs").join(format!("{name}.json"));
    let readout = load(&path);
    Credit {
        configured: true,
        expires: credit_expiry(&service),
        initial: service.get("initial").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64))).unwrap_or(0.0),
        spent: readout["usd_spent_period"].as_f64(),
        read_at: moment(&readout["read_at"]).or_else(|| modified_at(&path)),
    }
}

pub(in crate::pacing) fn credit_verdict(credit: &Credit, name: &str, at: f64) -> (bool, String) {
    if !credit.configured {
        let detail = if name.is_empty() { "no [placement] cloud_credit" } else { name };
        return (false, format!("cloud credit: not configured ({detail})"));
    }
    let Some(expiry) = credit.expires else {
        return (false, format!("cloud credit: not configured ({name})"));
    };
    if expiry <= at {
        return (false, format!("cloud credit: expired {}Z", &stamp(expiry)[..16]));
    }
    let days = ((expiry - at) / 86400.0) as i64;
    credit_standing(credit.spent, credit.read_at, credit.initial, days, at)
}
fn credit_service(name: &str) -> Option<toml_edit::Table> {
    let document = std::fs::read_to_string(operator().join("costs.toml"))
        .ok()
        .and_then(|s| s.parse::<DocumentMut>().ok())?;
    document
        .get("service")
        .and_then(Item::as_array_of_tables)
        .and_then(|ss| {
            ss.iter()
                .find(|s| s.get("id").and_then(Item::as_str) == Some(name))
        })
        .cloned()
}
fn credit_expiry(service: &toml_edit::Table) -> Option<f64> {
    service
        .get("expires")
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_datetime().map(ToString::to_string))
        })
        .and_then(|s| target::seconds(&s))
}
fn modified_at(path: &Path) -> Option<f64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs_f64())
}
fn credit_standing(
    spent: Option<f64>,
    read: Option<f64>,
    initial: f64,
    days: i64,
    at: f64,
) -> (bool, String) {
    let left = format!("expires in {days} days");
    let date = read.map(|t| stamp(t)[..10].to_owned()).unwrap_or_default();
    if spent.is_some_and(|n| n >= initial) {
        return (false, format!("cloud credit: spent (read {date}), {left}"));
    }
    if spent.is_none() || read.is_none_or(|t| at - t > 7.0 * 86400.0) {
        return (true, format!("cloud credit: unread, {left}"));
    }
    (
        true,
        format!(
            "cloud credit: ${:.2} left (read {date}), {left}",
            initial - spent.unwrap()
        ),
    )
}
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
        let closed = self.here_closed.clone().filter(|_| !self.crowded.is_empty());
        let Some(why) = self.here_saturated.clone().or(closed) else { return };
        let why = self.crowded.iter().cloned().chain([why]).collect::<Vec<_>>().join("; ");
        self.machine_lines.push(why.clone());
        self.target = super::gate::Target::Held(why);
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
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        let reading = &fixture[name]["saturation"];
        return Some(Saturation { memory_total_mib: reading["memory_total_mib"].as_f64()?, memory_available_mib: reading["memory_available_mib"].as_f64()?, disk_free_gib: reading["disk_free_gib"].as_f64()?, disk_total_gib: reading["disk_total_gib"].as_f64().unwrap_or(0.0), backlog: serde_json::from_value(reading["backlog"].clone()).ok() });
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
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let argv = crate::platform::ssh::argv(&["ssh".into()], &env, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "sh", "-s"]);
    let mut words: Vec<String> = argv.iter().map(|word| word.to_string_lossy().into_owned()).collect();
    if let Some(at) = words.iter().position(|word| word == "-F") {
        words.drain(at..at + 2);
    }
    let refs: Vec<&str> = words[1..].iter().map(String::as_str).collect();
    process::run(&words[0], &refs, Some(script), 20.0)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::pacing) struct Facts {
    pub live: Option<i64>,
    pub accounts: Vec<String>,
    pub saturation: Option<Saturation>,
    /// Seconds since the saturation reading was taken, zero for a fresh probe.
    pub saturation_age_s: Option<f64>,
    /// Whether the launch directory asked about exists there.
    pub has_cwd: Option<bool>,
}

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

/// Each machine's facts in one ssh round trip apiece, asked in parallel; `cwd` adds whether that directory exists there.
pub(in crate::pacing) fn surveyed(machines: &[Machine], cwd: Option<&str>) -> Vec<Facts> {
    survey_via(&PASS, machines, cwd, &fetch)
}

fn survey_via(memo: &Memo, machines: &[Machine], cwd: Option<&str>, ask: &(dyn Fn(&str, &str, Option<&str>) -> Facts + Sync)) -> Vec<Facts> {
    std::thread::scope(|scope| {
        let running: Vec<_> = machines
            .iter()
            .map(|machine| scope.spawn(move || remembered(memo, format!("{}\0{}\0{}", machine.name, machine.ssh, cwd.unwrap_or_default()), || ask(&machine.name, &machine.ssh, cwd))))
            .collect();
        running.into_iter().map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

impl Machine {
    /// The machine with its probed facts, its live count raised by the fleet sessions this machine runs there.
    pub(in crate::pacing) fn probed(self, facts: Facts, fleet_load: &BTreeMap<String, i64>) -> Self {
        let hosted = fleet_load.get(&self.name).copied().unwrap_or(0);
        Self { live: facts.live.map(|live| live + hosted), accounts: facts.accounts, saturation: facts.saturation, ..self }
    }
}

fn fetch(name: &str, destination: &str, cwd: Option<&str>) -> Facts {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let has_cwd = cwd.map(|_| load(Path::new(&path))[name]["has_cwd"].as_bool().unwrap_or(false));
        let saturation = saturation(name, destination);
        return Facts { live: resident(name, destination), accounts: signed_in(name, destination), saturation_age_s: saturation.map(|_| 0.0), saturation, has_cwd };
    }
    let here = name == local();
    let cache = storage::directory().join("saturation").join(format!("{name}.json"));
    let now = storage::now();
    let cached = load(&cache);
    let fresh = cached["at"].as_f64().is_some_and(|at| (0.0..SATURATION_CACHE_SECONDS).contains(&(now - at)));
    let mut facts = Facts { saturation: fresh.then(|| serde_json::from_value(cached["reading"].clone()).ok()).flatten(), ..Facts::default() };
    facts.saturation_age_s = facts.saturation.and(cached["at"].as_f64()).map(|at| now - at);
    facts.has_cwd = cwd.map(|_| false);
    let want_saturation = !fresh && !here;
    let answer = ssh_script(destination, &script(!here, want_saturation, cwd)).ok().filter(|r| r.code == 0).map(|r| r.out);
    if let Some(sections) = answer.as_deref().map(sections) {
        let section = |label: &str| sections.iter().find(|(name, _)| name == label).map(|(_, body)| body.as_str());
        facts.live = section("live").and_then(live_count);
        facts.accounts = section("accounts").map(claude_labels).unwrap_or_default();
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
    facts
}

const RESIDENT_BINARY: &str = ".local/state/north/current/tree/bin/north";

fn script(live: bool, saturation: bool, cwd: Option<&str>) -> String {
    let mut parts = Vec::new();
    if live {
        parts.push(format!("echo '@@live'; {RESIDENT_BINARY} resident-cap status; echo \"@@rc $?\""));
    }
    parts.push("echo '@@accounts'; north fleet accounts; echo \"@@rc $?\"".to_owned());
    if saturation {
        parts.push(format!("echo '@@saturation'; {}; echo \"@@rc $?\"", saturation::PROBE));
    }
    if let Some(directory) = cwd {
        parts.push(format!("echo '@@cwd'; test -d '{}'; echo \"@@rc $?\"", directory.replace('\'', r"'\''")));
    }
    parts.join("\n")
}

fn sections(output: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut open: Option<(String, String)> = None;
    for line in output.split_inclusive('\n') {
        let marker = line.trim_end();
        if let Some(label) = marker.strip_prefix("@@").filter(|label| !label.starts_with("rc ")) {
            open = Some((label.to_owned(), String::new()));
        } else if let Some(code) = marker.strip_prefix("@@rc ") {
            if let Some(section) = open.take().filter(|_| code == "0") {
                found.push(section);
            }
        } else if let Some((_, body)) = open.as_mut() {
            body.push_str(line);
        }
    }
    found
}

pub(in crate::pacing) fn signed_in(name: &str, destination: &str) -> Vec<String> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        if fixture[name].is_object() {
            return fixture[name]["accounts"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        }
    }
    let listed = process::run("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, "north", "fleet", "accounts"], None, 20.0);
    listed.ok().filter(|r| r.code == 0).map(|r| claude_labels(&r.out)).unwrap_or_default()
}

fn claude_labels(listing: &str) -> Vec<String> {
    listing.lines().filter_map(|line| line.strip_prefix("claude\t")).map(|label| label.trim().to_owned()).collect()
}

/// The live count from `north resident-cap status`, which prints `HOST: LIVE/CAP resident agent CLIs`.
fn live_count(status: &str) -> Option<i64> {
    let (_, rest) = status.split_once(':')?;
    rest.split_whitespace().next()?.split('/').next()?.parse().ok()
}
