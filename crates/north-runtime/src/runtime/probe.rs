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
const LANDER: &str = "land serve";

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

fn lander(argv: &[&str]) -> Option<String> {
    match argv {
        ["land", "serve", "--all"] => Some("all repositories".to_owned()),
        ["land", "serve", "--repo", repo] => Some((*repo).to_owned()),
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct NorthProcesses {
    transient: u64,
    residents: BTreeMap<Resident, u64>,
    sessions: BTreeSet<String>,
    watchdogs: BTreeMap<String, u64>,
    landers: BTreeMap<String, u64>,
    stray: u64,
}

impl NorthProcesses {
    fn read(section: &str, runtime: Option<&str>) -> Self {
        let mut processes = Self::default();
        let mut watchdogs: Vec<(&str, &str)> = Vec::new();
        let mut transient: Vec<(&str, &str)> = Vec::new();
        for line in section.lines().filter(|line| !line.is_empty()) {
            let mut fields = line.splitn(3, '\t');
            let (Some(pid), Some(parent), Some(command)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let argv: Vec<&str> = command.split('\t').collect();
            match (Resident::of(&argv[1..]).filter(|_| promoted(argv[0])), &argv[1..]) {
                (Some(resident), _) => {
                    *processes.residents.entry(resident).or_default() += 1;
                    if let (Resident::SessionHost, Some(name)) = (resident, argv.iter().position(|word| *word == "--name").and_then(|at| argv.get(at + 1))) {
                        processes.sessions.insert((*name).to_owned());
                    }
                }
                (None, ["fleet", "watchdog", session, ..]) => watchdogs.push((argv[0], session)),
                (None, rest) => match lander(rest).filter(|_| promoted(argv[0])) {
                    Some(repo) => *processes.landers.entry(repo).or_default() += 1,
                    None => transient.push((pid, parent)),
                },
            }
        }
        let pids: BTreeSet<&str> = transient.iter().map(|(pid, _)| *pid).collect();
        processes.transient = transient.iter().filter(|(_, parent)| !pids.contains(parent)).count() as u64;
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

    pub(super) fn transient(&self) -> u64 {
        self.transient
    }

    fn problems(&self) -> Vec<String> {
        let live = self.sessions.len() as u64;
        let mut problems = Vec::new();
        for resident in RESIDENTS {
            let count = self.resident(resident);
            if count > live {
                problems.push(format!("{count} {} processes for {live} live sessions", resident.name()));
            }
        }
        for (session, count) in self.watchdogs.iter().filter(|(_, count)| **count > 1) {
            problems.push(format!("{count} {WATCHDOG} processes of the live runtime for live session {session}"));
        }
        for (repo, count) in self.landers.iter().filter(|(_, count)| **count > 1) {
            problems.push(format!("{count} {LANDER} processes for {repo}"));
        }
        problems
    }

    fn record(&self) -> Value {
        let mut residents: serde_json::Map<String, Value> = RESIDENTS.iter().map(|resident| (resident.name().to_owned(), json!(self.resident(*resident)))).collect();
        residents.insert(WATCHDOG.to_owned(), json!(self.watchdogs.values().sum::<u64>()));
        residents.insert(LANDER.to_owned(), json!(self.landers.values().sum::<u64>()));
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
         read -r _ _ _ parent _ <\"$pid/stat\" 2>/dev/null || parent=0; \
         mapfile -d '' -t argv <\"$pid/cmdline\" 2>/dev/null || continue; [ ${{#argv[@]}} -gt 0 ] || argv=(north); argv=(\"${{pid##*/}}\" \"$parent\" \"${{argv[@]//[$'\\t\\n']/ }}\"); (IFS=$'\\t'; echo \"${{argv[*]}}\"); done\n\
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
#[path = "probe_tests.rs"]
mod tests;
