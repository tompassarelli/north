use super::layout::linked_sha;
use super::live::{Swap, SWAPS_KEPT};
use super::north_processes::NorthProcesses;
use super::remote::{limit, ssh_on};
use crate::platform::ssh_retry::{Link, Reply, Rerun, retried};
use crate::platform::{hook_commands, process};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::LazyLock;
use std::time::Duration;
use toml_edit::DocumentMut;

const ASK: Duration = Duration::from_secs(60);
const MARK: &str = "==north-settle== ";
const UNITS: [&str; 2] = ["capacity-watchdog.service", "north-runtime-follow.timer"];
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

    fn bash(&self, link: Link) -> Command {
        let mut command = match self {
            Self::Here(_) => Command::new("bash"),
            Self::Ssh(destination) => ssh_on(destination, link),
        };
        if matches!(self, Self::Ssh(_)) {
            command.arg("bash");
        }
        command.arg("-s");
        command
    }

    fn attempt(&self, script: &str, within: Duration, link: Link) -> Reply<std::io::Result<Option<Output>>> {
        let result = process::try_run_command_within(self.bash(link), Some(script.as_bytes()), limit(within));
        let (exit, said) = match &result {
            Ok(Some(output)) => (output.status.code(), String::from_utf8_lossy(&output.stderr).into_owned()),
            _ => (None, String::new()),
        };
        Reply { value: result, exit, said }
    }

    pub(super) fn exec(&self, script: &str, within: Duration, rerun: Rerun) -> std::io::Result<Option<Output>> {
        match self {
            Self::Here(_) => self.attempt(script, within, Link::Shared).value,
            Self::Ssh(destination) => retried(destination, rerun, |link| self.attempt(script, within, link)),
        }
    }

    pub(super) fn run(&self, script: &str) -> Result<String, String> {
        let output = self.exec(script, ASK, Rerun::Anywhere)
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
        north: sections.get("north").map(|north| NorthProcesses::read(north, linked_sha(Path::new(section("runtime").trim())), default_number("north_process_leak_window_sec") as u64)),
        ..Facts::default()
    };
    if let (true, Some(runtime), Some(at)) = (facts.swaps.is_empty(), facts.runtime.clone(), facts.swapped_at) {
        facts.swaps.push(Swap { runtime, at: at as i64, estimated: true, rollback: false });
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
    if state == "active" || state.starts_with(RESTARTING_AFTER_CLEAN_EXIT) {
        return true;
    }
    let mut words = state.split_whitespace();
    let (Some("activating"), Some("start"), Some("success"), Some(age)) = (words.next(), words.next(), words.next(), words.next()) else {
        return false;
    };
    let (Ok(age_us), Some(timeout)) = (age.parse::<u64>(), words.next()) else { return false };
    timeout_us(timeout).is_some_and(|limit| age_us < limit)
}

fn timeout_us(text: &str) -> Option<u64> {
    if text == "infinity" {
        return Some(u64::MAX);
    }
    text.replace('_', " ").split_whitespace().try_fold(0_u64, |total, part| total.checked_add(part_us(part)?))
}

/// One `AMOUNT UNIT` word of a systemd timeout: the microseconds it names, or none when the word is not one.
fn part_us(part: &str) -> Option<u64> {
    let split = part.find(|character: char| !character.is_ascii_digit())?;
    let (amount, unit) = part.split_at(split);
    let amount = amount.parse::<u64>().ok()?;
    let micros = match unit {
        "us" => 1,
        "ms" => 1_000,
        "s" => 1_000_000,
        "min" => 60_000_000,
        "h" => 3_600_000_000,
        "d" => 86_400_000_000,
        _ => return None,
    };
    amount.checked_mul(micros)
}

fn display(state: &str) -> String {
    state.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
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
            problems.push(format!("{unit} is {}", display(state)));
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
         mark units; now_us=$(( $(cut -d. -f1 \"$proc/uptime\" 2>/dev/null || echo 0) * 1000000 )); for unit in {}; do unit_state=$(systemctl --user is-active \"$unit\"); \
         if [ \"$unit_state\" = activating ]; then sub=$(systemctl --user show -p SubState --value \"$unit\"); result=$(systemctl --user show -p Result --value \"$unit\"); changed=$(systemctl --user show -p StateChangeTimestampMonotonic --value \"$unit\"); timeout=$(systemctl --user show -p TimeoutStartUSec --value \"$unit\"); [ -n \"$changed\" ] || changed=0; age_us=$(( now_us - changed )); [ \"$age_us\" -ge 0 ] || age_us=0; timeout=${{timeout// /_}}; unit_state=\"$unit_state $sub $result $age_us $timeout\"; fi; echo \"$unit_state\"; done\n\
         mark pressure; cat \"$proc/pressure/cpu\"\n\
         mark load; cat \"$proc/loadavg\"; nproc\n\
         mark north; up=$(cut -d. -f1 \"$proc/uptime\" 2>/dev/null || echo 0); hz=$(getconf CLK_TCK 2>/dev/null || echo 100); for pid in \"$proc\"/[0-9]*; do read -r name <\"$pid/comm\" 2>/dev/null && [ \"$name\" = north ] || continue; \
         read -r -a stat <\"$pid/stat\" 2>/dev/null || stat=(); parent=${{stat[3]:-0}}; age=$(( up - ${{stat[21]:-0}} / hz )); \
         mapfile -d '' -t argv <\"$pid/cmdline\" 2>/dev/null || continue; [ ${{#argv[@]}} -gt 0 ] || argv=(north); cgroup=; read -r cgroup <\"$pid/cgroup\" 2>/dev/null; unit=; [[ $cgroup =~ (north-fleet-[^/]*\\.service) ]] && unit=${{BASH_REMATCH[1]}}; argv=(\"${{pid##*/}}\" \"$parent\" \"$age\" \"$unit\" \"${{argv[@]//[$'\\t\\n']/ }}\"); (IFS=$'\\t'; echo \"${{argv[*]}}\"); done\n\
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
    (cores > 0.0).then_some(one_minute / cores)
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
