use super::layout::linked_sha;
use super::remote::{limit, ssh};
use crate::platform::{hook_commands, process};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;
use toml_edit::DocumentMut;

const ASK: Duration = Duration::from_secs(60);
const MARK: &str = "==north-settle== ";
const UNITS: [&str; 2] = ["capacity-watchdog.service", "north-runtime-follow.timer"];
const RESIDENTS: [Resident; 3] = [Resident::SessionHost, Resident::SessionLink, Resident::FleetWatchdog];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Resident {
    SessionHost,
    SessionLink,
    FleetWatchdog,
}

impl Resident {
    fn of(argv: &[&str]) -> Option<Self> {
        match argv {
            ["session-host", ..] => Some(Self::SessionHost),
            ["session-link", ..] => Some(Self::SessionLink),
            ["fleet", "watchdog", ..] => Some(Self::FleetWatchdog),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SessionHost => "session-host",
            Self::SessionLink => "session-link",
            Self::FleetWatchdog => "fleet watchdog",
        }
    }
}

#[derive(Default)]
pub(super) struct NorthProcesses {
    transient: u64,
    residents: BTreeMap<Resident, u64>,
    sessions: BTreeSet<String>,
}

impl NorthProcesses {
    fn read(section: &str) -> Self {
        let mut processes = Self::default();
        for line in section.lines().filter(|line| !line.is_empty()) {
            let argv: Vec<&str> = line.split('\t').skip(1).collect();
            match Resident::of(&argv) {
                Some(resident) => {
                    *processes.residents.entry(resident).or_default() += 1;
                    if let (Resident::SessionHost, Some(name)) = (resident, argv.iter().position(|word| *word == "--name").and_then(|at| argv.get(at + 1))) {
                        processes.sessions.insert((*name).to_owned());
                    }
                }
                None => processes.transient += 1,
            }
        }
        processes
    }

    fn resident(&self, resident: Resident) -> u64 {
        self.residents.get(&resident).copied().unwrap_or_default()
    }

    fn problems(&self) -> Vec<String> {
        let most = default_number("landing_north_processes_max");
        let live = self.sessions.len() as u64;
        let mut problems = Vec::new();
        if self.transient as f64 > most {
            problems.push(format!("{} transient north processes (bound {most})", self.transient));
        }
        for resident in RESIDENTS {
            let count = self.resident(resident);
            if count > live {
                problems.push(format!("{count} {} processes for {live} live sessions", resident.name()));
            }
        }
        problems
    }

    fn record(&self) -> Value {
        json!({
            "transient": self.transient,
            "liveSessions": self.sessions.len(),
            "residents": RESIDENTS.iter().map(|resident| (resident.name().to_owned(), json!(self.resident(*resident)))).collect::<serde_json::Map<_, _>>(),
        })
    }
}

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

pub(super) fn default_number(name: &str) -> f64 {
    let value = &DEFAULTS[name]["value"];
    value.as_float().or_else(|| value.as_integer().map(|whole| whole as f64)).unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
}

pub(super) enum Shell {
    Here(String),
    Ssh(String),
}

impl Shell {
    pub(super) fn name(&self) -> &str {
        match self {
            Self::Here(name) | Self::Ssh(name) => name,
        }
    }

    fn bash(&self) -> Command {
        let mut command = match self {
            Self::Here(_) => Command::new("bash"),
            Self::Ssh(destination) => ssh(destination),
        };
        if matches!(self, Self::Ssh(_)) {
            command.arg("bash");
        }
        command.arg("-s");
        command
    }

    pub(super) fn run(&self, script: &str) -> Result<String, String> {
        let output = process::try_run_command_within(self.bash(), Some(script.as_bytes()), limit(ASK))
            .map_err(|error| format!("could not start: {error}"))?
            .ok_or_else(|| format!("no answer within {}s", limit(ASK).as_secs()))?;
        if !output.status.success() {
            return Err(format!("{}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[derive(Default)]
pub(super) struct Facts {
    pub(super) host: String,
    pub(super) unreachable: Option<String>,
    pub(super) runtime: Option<String>,
    pub(super) units: BTreeMap<String, String>,
    pub(super) psi60: Option<f64>,
    pub(super) load_per_core: Option<f64>,
    pub(super) north: Option<NorthProcesses>,
    pub(super) settings: Vec<String>,
    pub(super) unresolved: Vec<String>,
}

pub(super) fn live_runtime(shell: &Shell) -> Option<String> {
    let link = shell.run("readlink \"${XDG_STATE_HOME:-$HOME/.local/state}/north/current\"\n").ok()?;
    linked_sha(Path::new(link.trim())).map(str::to_owned)
}

pub(super) fn facts(shell: &Shell) -> Facts {
    let host = shell.name().to_owned();
    let sections = match shell.run(&facts_script()) {
        Ok(stdout) => sections(&stdout),
        Err(why) => return Facts { host, unreachable: Some(why), ..Facts::default() },
    };
    let section = |name: &str| sections.get(name).map(String::as_str).unwrap_or_default();
    let mut facts = Facts {
        host,
        runtime: linked_sha(Path::new(section("runtime").trim())).map(str::to_owned),
        units: UNITS.iter().zip(section("units").lines()).map(|(unit, state)| ((*unit).to_owned(), state.trim().to_owned())).collect(),
        psi60: psi60(section("pressure")),
        load_per_core: load_per_core(section("load")),
        north: sections.get("north").map(|section| NorthProcesses::read(section)),
        ..Facts::default()
    };
    let mut registered: Vec<(String, String)> = Vec::new();
    for (name, body) in sections.iter().filter_map(|(name, body)| Some((name.strip_prefix("settings ")?, body))) {
        facts.settings.push(name.to_owned());
        match serde_json::from_str::<Value>(body) {
            Ok(settings) => registered.extend(hook_commands::paths(&settings).into_iter().map(|path| (name.to_owned(), path))),
            Err(error) => facts.unresolved.push(format!("{name}: unparsable ({error})")),
        }
    }
    let mut paths: Vec<&str> = registered.iter().map(|(_, path)| path.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    if !paths.is_empty() {
        match shell.run(&missing_script(&paths)) {
            Ok(missing) => {
                let missing: Vec<&str> = missing.lines().collect();
                facts.unresolved.extend(registered.iter().filter(|(_, path)| missing.contains(&path.as_str())).map(|(file, path)| format!("{file}: {path}")));
            }
            Err(why) => facts.unreachable = Some(format!("hook path check failed: {why}")),
        }
    }
    facts
}

pub(super) const LAGS: &str = "live runtime";

pub(super) fn installing(facts: &Facts) -> bool {
    facts.unreachable.is_none() && facts.runtime.is_none()
}

pub(super) fn problems(facts: &Facts, live: impl Fn(&str) -> bool) -> Vec<String> {
    if let Some(why) = &facts.unreachable {
        return vec![format!("unreachable: {why}")];
    }
    if installing(facts) {
        return Vec::new();
    }
    let mut problems = Vec::new();
    if let Some(runtime) = facts.runtime.as_deref().filter(|runtime| !live(runtime)) {
        problems.push(format!("{LAGS} {} predates the landing", &runtime[..8]));
    }
    for unit in UNITS {
        let state = facts.units.get(unit).map_or("unknown", String::as_str);
        if state != "active" {
            problems.push(format!("{unit} is {state}"));
        }
    }
    if facts.settings.is_empty() {
        problems.push("no Claude settings.json found".to_owned());
    }
    problems.extend(facts.unresolved.iter().map(|unresolved| format!("unresolved hook path {unresolved}")));
    match &facts.north {
        Some(north) => problems.extend(north.problems()),
        None => problems.push("north processes unread".to_owned()),
    }
    problems
}

pub(super) fn record(facts: &Facts, problems: &[String]) -> Value {
    json!({
        "host": facts.host,
        "installing": installing(facts),
        "runtime": facts.runtime,
        "units": facts.units,
        "cpuPsi60": facts.psi60,
        "loadPerCore": facts.load_per_core,
        "northProcesses": facts.north.as_ref().map(NorthProcesses::record),
        "settings": facts.settings,
        "unresolved": facts.unresolved,
        "unreachable": facts.unreachable,
        "problems": problems,
    })
}

fn facts_script() -> String {
    format!(
        "set -u\nstate=\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north\"\nproc=\"${{NORTH_SETTLE_PROC:-/proc}}\"\n\
         mark() {{ printf '\\n{MARK}%s\\n' \"$1\"; }}\n\
         mark runtime; readlink \"$state/current\"\n\
         mark units; systemctl --user is-active {} || true\n\
         mark pressure; cat \"$proc/pressure/cpu\"\n\
         mark load; cat \"$proc/loadavg\"; nproc\n\
         mark north; for pid in \"$proc\"/[0-9]*; do read -r name <\"$pid/comm\" 2>/dev/null && [ \"$name\" = north ] || continue; \
         mapfile -d '' -t argv <\"$pid/cmdline\" 2>/dev/null || continue; [ ${{#argv[@]}} -gt 0 ] || argv=(north); argv=(\"${{argv[@]//[$'\\t\\n']/ }}\"); (IFS=$'\\t'; echo \"${{argv[*]}}\"); done\n\
         for settings in \"$HOME/.claude/settings.json\" \"$state\"/profiles/*/settings.json; do\n\
         case \"$settings\" in */profiles/stale-*) continue ;; esac\n\
         [ -f \"$settings\" ] && {{ mark \"settings $settings\"; cat \"$settings\"; }}\ndone\ntrue\n",
        UNITS.join(" ")
    )
}

fn missing_script(paths: &[&str]) -> String {
    format!("while IFS= read -r path; do [ -e \"$path\" ] || printf '%s\\n' \"$path\"; done <<'NORTH_SETTLE_PATHS'\n{}\nNORTH_SETTLE_PATHS\n", paths.join("\n"))
}

fn sections(stdout: &str) -> BTreeMap<String, String> {
    let mut sections = BTreeMap::new();
    let mut name: Option<String> = None;
    for line in stdout.lines() {
        if let Some(next) = line.strip_prefix(MARK) {
            name = Some(next.to_owned());
            sections.insert(next.to_owned(), String::new());
        } else if let Some(body) = name.as_ref().and_then(|name| sections.get_mut(name)) {
            body.push_str(line);
            body.push('\n');
        }
    }
    sections
}

fn psi60(pressure: &str) -> Option<f64> {
    let some = pressure.lines().find(|line| line.starts_with("some "))?;
    some.split_whitespace().find_map(|field| field.strip_prefix("avg60="))?.parse().ok()
}

fn load_per_core(load: &str) -> Option<f64> {
    let mut lines = load.lines();
    let one_minute: f64 = lines.next()?.split_whitespace().next()?.parse().ok()?;
    let cores: f64 = lines.next()?.trim().parse().ok()?;
    (cores > 0.0).then(|| one_minute / cores)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_and_load_parse_from_their_proc_files() {
        assert_eq!(psi60("some avg10=27.94 avg60=22.27 avg300=40.48 total=1\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n"), Some(22.27));
        assert_eq!(load_per_core("71.00 60.00 50.00 3/900 1\n20\n"), Some(3.55));
    }

    #[test]
    fn a_busy_host_running_the_landing_is_healthy_because_load_is_the_watchdogs_to_judge() {
        let units = UNITS.iter().map(|unit| ((*unit).to_owned(), "active".to_owned())).collect();
        let facts = Facts { runtime: Some("af7971a5851e603b".into()), units, psi60: Some(57.3), load_per_core: Some(2.5), north: Some(NorthProcesses::default()), settings: vec!["settings.json".into()], ..Facts::default() };
        assert_eq!(problems(&facts, |_| true), Vec::<String>::new());
    }

    struct FakeProc(tempfile::TempDir, u32);

    impl FakeProc {
        fn new() -> Self {
            let proc = Self(tempfile::tempdir().unwrap(), 100);
            std::fs::write(proc.0.path().join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
            proc
        }

        fn spawn(&mut self, comm: &str, argv: &[&str]) {
            self.1 += 1;
            let pid = self.0.path().join(self.1.to_string());
            std::fs::create_dir(&pid).unwrap();
            std::fs::write(pid.join("comm"), format!("{comm}\n")).unwrap();
            std::fs::write(pid.join("cmdline"), argv.iter().map(|word| format!("{word}\0")).collect::<String>()).unwrap();
        }

        fn sessions(&mut self, live: usize, hosts_each: usize) {
            for session in 0..live {
                let name = format!("worker-{session}");
                for _ in 0..hosts_each {
                    self.spawn("north", &["/runtimes/a/north", "session-host", "--name", &name, "--generation", "1", "--", "/runtimes/a/north", "session-link", "--ssh", "gex45"]);
                }
                self.spawn("north", &["/runtimes/a/north", "session-link", "--ssh", "gex45", "--unit", &name]);
                self.spawn("north", &["/runtimes/a/north", "fleet", "watchdog", &name]);
            }
        }

        fn transients(&mut self, count: usize) {
            for _ in 0..count {
                self.spawn("north", &["/runtimes/a/north", "fleet", "list"]);
            }
        }

        fn problems(&self) -> (Vec<String>, Value) {
            let output = Command::new("bash")
                .arg("-c")
                .arg(facts_script())
                .env("NORTH_SETTLE_PROC", self.0.path())
                .env("HOME", self.0.path())
                .env("XDG_STATE_HOME", self.0.path())
                .output()
                .unwrap();
            let north = NorthProcesses::read(&sections(&String::from_utf8_lossy(&output.stdout))["north"]);
            (north.problems(), north.record())
        }
    }

    #[test]
    fn fifty_live_sessions_with_their_three_residents_each_and_twenty_transients_are_healthy() {
        let mut proc = FakeProc::new();
        proc.sessions(50, 1);
        proc.transients(20);
        proc.spawn("bash", &["bash", "fleet", "list"]);
        let (problems, record) = proc.problems();
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(record, json!({"transient": 20, "liveSessions": 50, "residents": {"session-host": 50, "session-link": 50, "fleet watchdog": 50}}));
    }

    #[test]
    fn only_processes_named_north_count_and_a_multi_line_argument_stays_one_process() {
        let mut proc = FakeProc::new();
        proc.sessions(2, 1);
        proc.spawn("north", &["/runtimes/a/north", "session-host", "--name", "worker-2", "--", "claude", "--append-system-prompt", "first\n\tsecond\nthird"]);
        proc.spawn("claude", &["claude", "--cwd", "/home/tom/code/north/worktrees/a", "--append-system-prompt", "north\nsession-host\nfleet watchdog"]);
        proc.spawn("bash", &["bash", "/home/tom/code/north/worktrees/a/scripts/check.sh", "fleet", "list"]);
        proc.spawn("rustc", &["rustc", "--crate-name", "north_core", "/home/tom/code/north/main/crates/north-core/src/lib.rs"]);
        let (problems, record) = proc.problems();
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(record, json!({"transient": 0, "liveSessions": 3, "residents": {"session-host": 3, "session-link": 2, "fleet watchdog": 2}}));
    }

    #[test]
    fn a_storm_of_transient_north_processes_is_unhealthy_and_names_its_count() {
        let mut proc = FakeProc::new();
        proc.sessions(10, 1);
        proc.transients(120);
        assert_eq!(proc.problems().0, vec!["120 transient north processes (bound 100)".to_owned()]);
    }

    #[test]
    fn residents_beyond_the_live_sessions_are_a_leak_that_names_the_kind() {
        let mut proc = FakeProc::new();
        proc.sessions(10, 3);
        assert_eq!(proc.problems().0, vec!["30 session-host processes for 10 live sessions".to_owned()]);
    }
}
