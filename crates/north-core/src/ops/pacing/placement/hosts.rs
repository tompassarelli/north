use super::super::storage::load;
use super::super::{process, storage, target};
use super::{local, moment, operator, stamp};
use crate::capacity::saturation::{self, Saturation};
use serde_json::{Value, json};
use super::gate::Machine;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Mutex;
use toml_edit::{DocumentMut, Item};

pub(in crate::ops::pacing) fn credit(cfg: &DocumentMut, at: f64) -> (bool, String) {
    let name = cfg
        .get("placement")
        .and_then(|p| p.get("cloud_credit"))
        .and_then(Item::as_str)
        .unwrap_or_default();
    let Some(service) = credit_service(name) else {
        let detail = if name.is_empty() {
            "no [placement] cloud_credit"
        } else {
            name
        };
        return (false, format!("cloud credit: not configured ({detail})"));
    };
    let Some(expiry) = credit_expiry(&service) else {
        return (false, format!("cloud credit: not configured ({name})"));
    };
    if expiry <= at {
        return (
            false,
            format!("cloud credit: expired {}Z", &stamp(expiry)[..16]),
        );
    }
    let days = ((expiry - at) / 86400.0) as i64;
    let path = storage::directory()
        .join("costs")
        .join(format!("{name}.json"));
    let readout = load(&path);
    let spent = readout["usd_spent_period"].as_f64();
    let read = moment(&readout["read_at"]).or_else(|| modified_at(&path));
    let initial = service
        .get("initial")
        .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|n| n as f64)))
        .unwrap_or(0.0);
    credit_standing(spent, read, initial, days, at)
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
pub(in crate::ops::pacing) fn laptop() -> String {
    if let Ok(path) = std::env::var("LAPTOP_STATE") {
        return load(Path::new(&path))["state"]
            .as_str()
            .unwrap_or("unknown")
            .into();
    }
    state_of(&crate::ops::laptop::Ctx::from_env())
}
fn state_of(ctx: &crate::ops::laptop::Ctx) -> String {
    ctx.main(&["status".to_owned(), "--json".to_owned()])
        .ok()
        .and_then(|out| serde_json::from_str::<Value>(&out).ok())
        .and_then(|v| v["state"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}
/// Live resident agent CLIs on host `name`, asked through ssh `destination` unless it is this host;
/// None when unreachable.
pub(in crate::ops::pacing) fn resident(name: &str, destination: &str) -> Option<i64> {
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
        return live_count(&crate::ops::resident_cap::status());
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

pub(in crate::ops::pacing) fn here_saturation() -> Option<String> {
    let name = local();
    saturation(&name, "")?.refusal(&name)
}

/// The spawn gate's CPU refusal for a launch kept here; a PLACEMENT_HOSTS fixture run reads only the
/// SPAWN_CAPACITY_STATUS fixture, never this host's live capacity.
pub(in crate::ops::pacing) fn here_cpu_refusal(brief: &str) -> Option<String> {
    if std::env::var_os("PLACEMENT_HOSTS").is_some() && std::env::var_os("SPAWN_CAPACITY_STATUS").is_none() {
        return None;
    }
    crate::ops::pacing::spawn_gate::launch_refusal(brief)
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
    pub(in crate::ops::pacing) fn saturation_refusal(&self) -> Option<String> {
        self.saturation?.refusal(&self.name)
    }
}

/// The host's saturation reading, from the PLACEMENT_HOSTS fixture alone when that is set, else the cache when under a
/// minute old, else a fresh probe here or over ssh `destination`; None when unreadable.
pub(in crate::ops::pacing) fn saturation(name: &str, destination: &str) -> Option<Saturation> {
    if let Ok(path) = std::env::var("PLACEMENT_HOSTS") {
        let fixture = load(Path::new(&path));
        let reading = &fixture[name]["saturation"];
        return Some(Saturation { memory_total_mib: reading["memory_total_mib"].as_f64()?, memory_available_mib: reading["memory_available_mib"].as_f64()?, disk_free_gib: reading["disk_free_gib"].as_f64()?, backlog: serde_json::from_value(reading["backlog"].clone()).ok() });
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
pub(in crate::ops::pacing) struct Facts {
    pub live: Option<i64>,
    pub accounts: Vec<String>,
    pub saturation: Option<Saturation>,
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

pub(in crate::ops::pacing) fn probed_machines(machines: Vec<Machine>, fleet_load: &BTreeMap<String, i64>) -> Vec<Machine> {
    probed_via(&PASS, machines, fleet_load, &fetch)
}

fn probed_via(memo: &Memo, machines: Vec<Machine>, fleet_load: &BTreeMap<String, i64>, ask: &(dyn Fn(&str, &str) -> Facts + Sync)) -> Vec<Machine> {
    std::thread::scope(|scope| {
        let running: Vec<_> = machines
            .into_iter()
            .map(|machine| {
                scope.spawn(move || {
                    let facts = remembered(memo, format!("{}\0{}", machine.name, machine.ssh), || ask(&machine.name, &machine.ssh));
                    let hosted = fleet_load.get(&machine.name).copied().unwrap_or(0);
                    Machine { live: facts.live.map(|live| live + hosted), accounts: facts.accounts, saturation: facts.saturation, ..machine }
                })
            })
            .collect();
        running.into_iter().map(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

fn fetch(name: &str, destination: &str) -> Facts {
    if std::env::var_os("PLACEMENT_HOSTS").is_some() {
        return Facts { live: resident(name, destination), accounts: signed_in(name, destination), saturation: saturation(name, destination) };
    }
    let here = name == local();
    let cache = storage::directory().join("saturation").join(format!("{name}.json"));
    let now = storage::now();
    let cached = load(&cache);
    let fresh = cached["at"].as_f64().is_some_and(|at| (0.0..SATURATION_CACHE_SECONDS).contains(&(now - at)));
    let mut facts = Facts { saturation: fresh.then(|| serde_json::from_value(cached["reading"].clone()).ok()).flatten(), ..Facts::default() };
    let want_saturation = !fresh && !here;
    let answer = ssh_script(destination, &script(!here, want_saturation)).ok().filter(|r| r.code == 0).map(|r| r.out);
    if let Some(sections) = answer.as_deref().map(sections) {
        let section = |label: &str| sections.iter().find(|(name, _)| name == label).map(|(_, body)| body.as_str());
        facts.live = section("live").and_then(live_count);
        facts.accounts = section("accounts").map(claude_labels).unwrap_or_default();
        if want_saturation {
            facts.saturation = section("saturation").and_then(saturation::parse);
        }
    }
    if here {
        facts.live = live_count(&crate::ops::resident_cap::status());
    }
    if here && !fresh {
        facts.saturation = probe(name, destination);
    }
    if !fresh && let (Some(reading), Some(dir)) = (facts.saturation, cache.parent()) {
        let _ = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&cache, json!({"at": now, "reading": reading}).to_string()));
    }
    facts
}

const RESIDENT_BINARY: &str = ".local/state/north/current/tree/bin/north";

fn script(live: bool, saturation: bool) -> String {
    let mut parts = Vec::new();
    if live {
        parts.push(format!("echo '@@live'; {RESIDENT_BINARY} resident-cap status; echo \"@@rc $?\""));
    }
    parts.push("echo '@@accounts'; north fleet accounts; echo \"@@rc $?\"".to_owned());
    if saturation {
        parts.push(format!("echo '@@saturation'; {}; echo \"@@rc $?\"", saturation::PROBE));
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

pub(in crate::ops::pacing) fn signed_in(name: &str, destination: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_live_count_is_read_from_the_resident_cap_status_line() {
        assert_eq!(super::live_count("far: 3/24 resident agent CLIs\n"), Some(3));
        assert_eq!(super::live_count("far: no cap\n"), None);
    }

    #[test]
    fn the_laptop_state_is_read_in_process_without_a_laptop_program_on_the_path() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("placement")).unwrap();
        std::fs::write(root.path().join("placement/laptop.json"), r#"{"state":"draining","since":"2026-10-13T12:00:00Z","reason":"lid","by":"t","grace":900}"#).unwrap();
        let ctx = crate::ops::laptop::Ctx::fixture(root.path(), "2026-10-13T12:00:00Z", "false");
        assert_eq!(super::state_of(&ctx), "draining");
    }

    fn machine(name: &str) -> Machine {
        Machine { name: name.into(), ssh: format!("tom@{name}"), serves: Vec::new(), pushes: None, reaches: None, cap: 4, live: None, accounts: Vec::new(), saturation: None }
    }

    fn answer() -> Facts {
        Facts { live: Some(2), accounts: vec!["a".into()], saturation: None }
    }

    #[test]
    fn one_pass_asks_each_remote_host_for_its_facts_once_however_many_times_it_places() {
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let ask = |_: &str, _: &str| {
            asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            answer()
        };
        let memo: Memo = Mutex::new(Some(HashMap::new()));
        let load = BTreeMap::from([("far".to_owned(), 1)]);
        for _ in 0..12 {
            let machines = super::probed_via(&memo, vec![machine("far"), machine("near")], &load, &ask);
            assert_eq!(machines[0].live, Some(3));
            assert_eq!(machines[1].live, Some(2));
            assert_eq!(machines[0].accounts, ["a"]);
        }
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn outside_a_pass_every_placement_asks_afresh() {
        let asked = std::sync::atomic::AtomicUsize::new(0);
        let ask = |_: &str, _: &str| {
            asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            answer()
        };
        let memo: Memo = Mutex::new(None);
        for _ in 0..3 {
            super::probed_via(&memo, vec![machine("far")], &BTreeMap::new(), &ask);
        }
        assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[test]
    fn the_one_ssh_script_answers_live_roster_and_saturation_in_marked_sections() {
        let output = "@@live\nfar: 3/24 resident agent CLIs\n@@rc 0\n@@accounts\nclaude\ta\ncodex\tpooled\n@@rc 0\n@@saturation\n98643000\n40960000\n/dev/md127 489970656 443351304 21656744 96% /\n#backlog\n16 25.3 20\n@@rc 0\n";
        let found = super::sections(output);
        let body = |label: &str| found.iter().find(|(name, _)| name == label).map(|(_, body)| body.as_str());
        assert_eq!(body("live").and_then(super::live_count), Some(3));
        assert_eq!(body("accounts").map(super::claude_labels), Some(vec!["a".to_owned()]));
        assert_eq!(body("saturation").and_then(saturation::parse).and_then(|reading| reading.backlog).map(|backlog| backlog.queued), Some(16));
    }

    #[test]
    fn a_section_whose_command_failed_is_left_out() {
        let found = super::sections("@@live\n@@rc 127\n@@accounts\nclaude\ta\n@@rc 0\n");
        assert_eq!(found.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["accounts"]);
    }

    #[test]
    fn the_script_asks_the_host_for_only_what_it_cannot_read_itself() {
        let all = super::script(true, true);
        assert_eq!(all.matches("@@").count(), 6);
        assert_eq!(super::script(false, false).matches("echo '@@").count(), 1);
    }

    #[test]
    fn a_hosts_roster_is_read_from_its_fleet_accounts_listing() {
        assert_eq!(super::claude_labels("claude\ta\nclaude\tc\ncodex\tpooled\n"), ["a", "c"]);
    }

    const NOW: f64 = 1_791_896_400.0;
    const DAY: f64 = 86_400.0;

    #[test]
    fn the_credit_line_reads_unread_spent_or_what_is_left() {
        let unread = || (true, "cloud credit: unread, expires in 22 days".to_owned());
        assert_eq!(super::credit_standing(None, None, 250.0, 22, NOW), unread());
        assert_eq!(
            super::credit_standing(Some(250.0), Some(NOW - DAY), 250.0, 22, NOW),
            (false, "cloud credit: spent (read 2026-10-12), expires in 22 days".to_owned())
        );
        assert_eq!(
            super::credit_standing(Some(100.0), Some(NOW - DAY), 250.0, 22, NOW),
            (true, "cloud credit: $150.00 left (read 2026-10-12), expires in 22 days".to_owned())
        );
        assert_eq!(
            super::credit_standing(Some(100.0), Some(NOW - 8.0 * DAY), 250.0, 22, NOW),
            unread()
        );
    }
}
