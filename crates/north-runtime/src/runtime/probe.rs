use super::layout::linked_sha;
use super::live::{Swap, SWAPS_KEPT};
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
const RESIDENTS: [Resident; 2] = [Resident::SessionHost, Resident::SessionLink];
const WATCHDOG: &str = "fleet watchdog";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Resident {
    SessionHost,
    SessionLink,
}

impl Resident {
    fn of(argv: &[&str]) -> Option<Self> {
        match argv {
            ["session-host", ..] => Some(Self::SessionHost),
            ["session-link", ..] => Some(Self::SessionLink),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SessionHost => "session-host",
            Self::SessionLink => "session-link",
        }
    }
}

fn promoted(program: &str) -> bool {
    program.split('/').any(|part| part == "runtimes")
}

fn runs(program: &str, runtime: &str) -> bool {
    let mut parts = program.split('/');
    parts.any(|part| part == "runtimes") && parts.next() == Some(runtime)
}

#[derive(Default)]
pub(super) struct NorthProcesses {
    transient: u64,
    residents: BTreeMap<Resident, u64>,
    sessions: BTreeSet<String>,
    watchdogs: BTreeMap<String, u64>,
    stray: u64,
}

impl NorthProcesses {
    fn read(section: &str, runtime: Option<&str>) -> Self {
        let mut processes = Self::default();
        let mut watchdogs: Vec<(&str, &str)> = Vec::new();
        for line in section.lines().filter(|line| !line.is_empty()) {
            let argv: Vec<&str> = line.split('\t').collect();
            match (Resident::of(&argv[1..]).filter(|_| promoted(argv[0])), &argv[1..]) {
                (Some(resident), _) => {
                    *processes.residents.entry(resident).or_default() += 1;
                    if let (Resident::SessionHost, Some(name)) = (resident, argv.iter().position(|word| *word == "--name").and_then(|at| argv.get(at + 1))) {
                        processes.sessions.insert((*name).to_owned());
                    }
                }
                (None, ["fleet", "watchdog", session, ..]) => watchdogs.push((argv[0], session)),
                (None, _) => processes.transient += 1,
            }
        }
        for (program, session) in watchdogs {
            match processes.sessions.contains(session) && runtime.is_some_and(|runtime| runs(program, runtime)) {
                true => *processes.watchdogs.entry(session.to_owned()).or_default() += 1,
                false => processes.stray += 1,
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
        for (session, count) in self.watchdogs.iter().filter(|(_, count)| **count > 1) {
            problems.push(format!("{count} {WATCHDOG} processes of the live runtime for live session {session}"));
        }
        problems
    }

    fn record(&self) -> Value {
        let mut residents: serde_json::Map<String, Value> = RESIDENTS.iter().map(|resident| (resident.name().to_owned(), json!(self.resident(*resident)))).collect();
        residents.insert(WATCHDOG.to_owned(), json!(self.watchdogs.values().sum::<u64>()));
        json!({
            "transient": self.transient,
            "liveSessions": self.sessions.len(),
            "residents": residents,
            "strayWatchdogs": self.stray,
        })
    }
}

static DEFAULTS: LazyLock<&DocumentMut> = LazyLock::new(|| north_base::platform::harness::document(north_base::platform::harness::DEFAULTS));

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
    pub(super) swapped_at: Option<u64>,
    pub(super) swaps: Vec<Swap>,
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
        swapped_at: section("swapped").trim().parse().ok(),
        swaps: section("swaps").lines().filter_map(|line| serde_json::from_str(line).ok()).collect(),
        units: UNITS.iter().zip(section("units").lines()).map(|(unit, state)| ((*unit).to_owned(), state.trim().to_owned())).collect(),
        psi60: psi60(section("pressure")),
        load_per_core: load_per_core(section("load")),
        north: sections.get("north").map(|north| NorthProcesses::read(north, linked_sha(Path::new(section("runtime").trim())))),
        ..Facts::default()
    };
    if let (true, Some(runtime), Some(at)) = (facts.swaps.is_empty(), facts.runtime.clone(), facts.swapped_at) {
        facts.swaps.push(Swap { runtime, at: at as i64, estimated: true });
    }
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

const RESTARTING_AFTER_CLEAN_EXIT: &str = "activating auto-restart success";

fn healthy(state: &str) -> bool {
    state == "active" || state == RESTARTING_AFTER_CLEAN_EXIT
}

pub(super) fn problems(facts: &Facts, live: impl Fn(&str) -> bool) -> Vec<String> {
    if let Some(why) = &facts.unreachable {
        return vec![format!("unreachable: {why}")];
    }
    if installing(facts) {
        return if facts.unresolved.is_empty() { Vec::new() } else { std::iter::once("no live runtime".to_owned()).chain(facts.unresolved.iter().map(|unresolved| format!("unresolved hook path {unresolved}"))).collect() };
    }
    let mut problems = Vec::new();
    if let Some(runtime) = facts.runtime.as_deref().filter(|runtime| !live(runtime)) {
        problems.push(format!("{LAGS} {} predates the landing", &runtime[..8]));
    }
    for unit in UNITS {
        let state = facts.units.get(unit).map_or("unknown", String::as_str);
        if !healthy(state) {
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
        "swappedAt": facts.swapped_at,
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
         mark swapped; stat -c %Y \"$state/current\"\n\
         mark swaps; tail -n {SWAPS_KEPT} \"$state/runtime-swaps.jsonl\" 2>/dev/null || true\n\
         mark units; for unit in {}; do unit_state=$(systemctl --user is-active \"$unit\"); \
         [ \"$unit_state\" = activating ] && unit_state=\"$unit_state $(systemctl --user show -p SubState --value \"$unit\") $(systemctl --user show -p Result --value \"$unit\")\"; echo \"$unit_state\"; done\n\
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

    const OLD: &str = "1111111111111111111111111111111111111111";
    const LIVE: &str = "2222222222222222222222222222222222222222";

    fn north(runtime: &str) -> String {
        format!("/home/tom/.local/state/north/runtimes/{runtime}/tree/bin/../../north")
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
            let north = north(LIVE);
            for session in 0..live {
                let name = format!("worker-{session}");
                for _ in 0..hosts_each {
                    self.spawn("north", &[&north, "session-host", "--name", &name, "--generation", "1", "--", &north, "session-link", "--ssh", "gex45"]);
                }
                self.spawn("north", &[&north, "session-link", "--ssh", "gex45", "--unit", &name]);
                self.spawn("north", &[&north, "fleet", "watchdog", &name]);
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
            let north = NorthProcesses::read(&sections(&String::from_utf8_lossy(&output.stdout))["north"], Some(LIVE));
            (north.problems(), north.record())
        }
    }

    #[test]
    fn only_processes_named_north_count_and_a_multi_line_argument_stays_one_process() {
        let mut proc = FakeProc::new();
        proc.sessions(2, 1);
        let north = north(LIVE);
        proc.spawn("north", &[&north, "session-host", "--name", "worker-2", "--", "claude", "--append-system-prompt", "first\n\tsecond\nthird"]);
        proc.spawn("claude", &["claude", "--cwd", "/home/tom/code/north/worktrees/a", "--append-system-prompt", "north\nsession-host\nfleet watchdog"]);
        proc.spawn("bash", &["bash", "/home/tom/code/north/worktrees/a/scripts/check.sh", "fleet", "list"]);
        proc.spawn("rustc", &["rustc", "--crate-name", "north_core", "/home/tom/code/north/main/crates/north-core/src/lib.rs"]);
        let (problems, record) = proc.problems();
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(record, json!({"transient": 0, "liveSessions": 3, "residents": {"session-host": 3, "session-link": 2, "fleet watchdog": 2}, "strayWatchdogs": 0}));
    }

    #[test]
    fn a_session_host_a_land_check_test_runs_from_its_sandbox_build_is_not_a_fleet_resident() {
        let mut proc = FakeProc::new();
        proc.sessions(1, 1);
        proc.spawn("north", &["/home/tom/code/north/sandbox/target/debug/north", "session-host", "--name", "worker", "--generation", "1", "--state-dir", "/tmp/.tmpOQQkoY/.local/state/north/fleet/worker"]);
        proc.spawn("north", &["/home/tom/code/north/sandbox/target/debug/north", "session-host", "--name", "worker-0", "--generation", "1"]);
        let (problems, record) = proc.problems();
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(record["residents"]["session-host"], json!(1));
    }

    #[test]
    fn a_runtime_swap_leaves_old_and_ended_session_watchdogs_that_are_not_a_fault() {
        let mut proc = FakeProc::new();
        proc.sessions(2, 1);
        let old = north(OLD);
        proc.spawn("north", &[&old, "fleet", "watchdog", "worker-0"]);
        proc.spawn("north", &[&old, "fleet", "watchdog", "worker-1"]);
        proc.spawn("north", &[&north(LIVE), "fleet", "watchdog", "worker-ended"]);
        let (problems, record) = proc.problems();
        assert_eq!(problems, Vec::<String>::new());
        assert_eq!(record["residents"]["fleet watchdog"], json!(2));
        assert_eq!(record["strayWatchdogs"], json!(3));
    }

    #[test]
    fn two_live_runtime_watchdogs_for_one_live_session_are_a_leak() {
        let mut proc = FakeProc::new();
        proc.sessions(2, 1);
        proc.spawn("north", &[&north(LIVE), "fleet", "watchdog", "worker-1"]);
        let (problems, _) = proc.problems();
        assert_eq!(problems, vec!["2 fleet watchdog processes of the live runtime for live session worker-1".to_owned()]);
    }

    const TIMER: (&str, &str, &str, &str) = ("north-runtime-follow.timer", "active", "waiting", "success");

    fn unit_problems(watchdog: (&str, &str, &str)) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let systemctl = dir.path().join("systemctl");
        std::fs::write(&systemctl, "#!/usr/bin/env bash\nunit=\"${@: -1}\"\ncase \"$2\" in\nis-active) cat \"$FAKE_UNITS/$unit.active\"; exit 3;;\nshow) cat \"$FAKE_UNITS/$unit.$4\";;\nesac\n").unwrap();
        std::fs::set_permissions(&systemctl, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        for (unit, active, sub, result) in [("capacity-watchdog.service", watchdog.0, watchdog.1, watchdog.2), TIMER] {
            std::fs::write(dir.path().join(format!("{unit}.active")), format!("{active}\n")).unwrap();
            std::fs::write(dir.path().join(format!("{unit}.SubState")), format!("{sub}\n")).unwrap();
            std::fs::write(dir.path().join(format!("{unit}.Result")), format!("{result}\n")).unwrap();
        }
        let output = Command::new("bash")
            .arg("-c")
            .arg(facts_script())
            .env("PATH", format!("{}:{}", dir.path().display(), std::env::var("PATH").unwrap()))
            .env("FAKE_UNITS", dir.path())
            .env("NORTH_SETTLE_PROC", dir.path())
            .env("HOME", dir.path())
            .env("XDG_STATE_HOME", dir.path())
            .output()
            .unwrap();
        let units = sections(&String::from_utf8_lossy(&output.stdout)).remove("units").unwrap();
        let facts = Facts {
            host: "gex45".to_owned(),
            runtime: Some(LIVE.to_owned()),
            units: UNITS.iter().zip(units.lines()).map(|(unit, state)| ((*unit).to_owned(), state.to_owned())).collect(),
            settings: vec!["settings.json".to_owned()],
            north: Some(NorthProcesses::default()),
            ..Facts::default()
        };
        problems(&facts, |_| true)
    }

    #[test]
    fn a_watchdog_waiting_to_restart_after_the_promoted_runtime_moved_is_healthy() {
        assert_eq!(unit_problems(("activating", "auto-restart", "success")), Vec::<String>::new());
    }

    #[test]
    fn a_watchdog_waiting_to_restart_after_a_crash_or_stopped_is_a_fault() {
        assert_eq!(unit_problems(("activating", "auto-restart", "exit-code")), vec!["capacity-watchdog.service is activating auto-restart exit-code".to_owned()]);
        assert_eq!(unit_problems(("activating", "start", "success")), vec!["capacity-watchdog.service is activating start success".to_owned()]);
        assert_eq!(unit_problems(("failed", "failed", "exit-code")), vec!["capacity-watchdog.service is failed".to_owned()]);
    }
}
