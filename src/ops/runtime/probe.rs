use super::layout::linked_sha;
use super::remote::{limit, ssh};
use crate::platform::{hook_commands, process};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;
use toml_edit::DocumentMut;

const ASK: Duration = Duration::from_secs(60);
const MARK: &str = "==north-settle== ";
const UNITS: [&str; 2] = ["capacity-watchdog.service", "north-runtime-follow.timer"];

static DEFAULTS: LazyLock<DocumentMut> = LazyLock::new(|| include_str!("../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses"));

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

    fn run(&self, script: &str) -> Result<String, String> {
        let output = process::run_command_within(self.bash(), Some(script.as_bytes()), limit(ASK))
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
    pub(super) north_processes: Option<u64>,
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
        north_processes: section("north").trim().parse().ok(),
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

pub(super) fn problems(facts: &Facts, live: impl Fn(&str) -> bool) -> Vec<String> {
    if let Some(why) = &facts.unreachable {
        return vec![format!("unreachable: {why}")];
    }
    let mut problems = Vec::new();
    match facts.runtime.as_deref() {
        Some(runtime) if live(runtime) => {}
        Some(runtime) => problems.push(format!("live runtime {} predates the landing", &runtime[..8])),
        None => problems.push("no live runtime".to_owned()),
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
    let load = default_number("overload_load_per_core");
    let psi = default_number("overload_psi60");
    if let (Some(per_core), Some(psi60)) = (facts.load_per_core, facts.psi60)
        && per_core >= load
        && psi60 >= psi
    {
        problems.push(format!("overloaded: load {per_core:.1} per core (bound {load}) with CPU pressure avg60 {psi60:.1}% (bound {psi}%)"));
    }
    let most = default_number("landing_north_processes_max");
    match facts.north_processes {
        Some(count) if count as f64 > most => problems.push(format!("{count} north processes (bound {most})")),
        Some(_) => {}
        None => problems.push("north process count unread".to_owned()),
    }
    problems
}

pub(super) fn record(facts: &Facts, problems: &[String]) -> Value {
    json!({
        "host": facts.host,
        "runtime": facts.runtime,
        "units": facts.units,
        "cpuPsi60": facts.psi60,
        "loadPerCore": facts.load_per_core,
        "northProcesses": facts.north_processes,
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
         mark north; n=0; for comm in \"$proc\"/[0-9]*/comm; do read -r name <\"$comm\" 2>/dev/null && [ \"$name\" = north ] && n=$((n+1)); done; echo \"$n\"\n\
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
}
