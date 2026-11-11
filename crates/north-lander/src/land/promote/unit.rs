use super::{Options, Runtime};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const UNIT: &str = "north-lander-replay";
const LOG: &str = "lander-replay.log";
const PASSED: [&str; 6] = ["HOME", "PATH", "XDG_STATE_HOME", "XDG_CONFIG_HOME", "XDG_RUNTIME_DIR", "SSH_AUTH_SOCK"];

/// Stops the replay a promotion handed off before this one, so the newest runtime's replay judges the link and an older one never rolls it back.
pub(super) fn stop(runtime: &Runtime) {
    let _ = Command::new("systemctl").args(["--user", "stop", &format!("{}.service", name(runtime))]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// Starts the replay of recorded rounds against `target` in a unit of its own, which outlives the promotion that moved the link and the order that ran it; the unit judges, rolls back and pages as an inline replay would.
pub(super) fn hand_off(runtime: &Runtime, options: &Options, binary: &Path, target: &Path, held: Option<&Path>) -> Result<String, String> {
    let (log, unit_name) = (runtime.home.join(LOG), name(runtime));
    let mut unit = Command::new("systemd-run");
    unit.args(["--user", "--quiet", "--collect", "--same-dir", "--slice=agent.slice", "--unit"]).arg(&unit_name);
    unit.arg(format!("--property=StandardOutput=append:{}", log.display())).arg(format!("--property=StandardError=append:{}", log.display()));
    for (name, value) in PASSED.iter().filter_map(|name| Some((name, std::env::var_os(name)?))) {
        let mut setting = OsString::from(format!("--setenv={name}="));
        setting.push(value);
        unit.arg(setting);
    }
    unit.arg("--").arg(binary).args(judge_arguments(options, target, held));
    let started = unit.stdin(Stdio::null()).status().map_err(|error| format!("could not start {unit_name}: {error}"))?;
    match started.success() {
        true => Ok(format!("the lander link points at {}; {unit_name}.service replays the recorded rounds against it, rolls the link back on an untolerated divergence and logs its verdict to {}", target.display(), log.display())),
        false => Err(format!("systemd-run could not start {unit_name}: {started}")),
    }
}

fn judge_arguments(options: &Options, target: &Path, held: Option<&Path>) -> Vec<OsString> {
    let mut arguments: Vec<OsString> = ["land", "promote", "--judge"].map(OsString::from).to_vec();
    arguments.push(target.into());
    if let Some(held) = held {
        arguments.extend([OsString::from("--held"), held.into()]);
    }
    arguments.extend([OsString::from("--shadow-minutes"), options.shadow_minutes.to_string().into(), OsString::from("--checker"), options.checker.clone().into()]);
    for repo in &options.repos {
        arguments.extend([OsString::from("--repo"), absolute(repo).into()]);
    }
    arguments
}

fn absolute(repo: &Path) -> PathBuf {
    std::path::absolute(repo).unwrap_or_else(|_| repo.to_owned())
}

/// The unit's name: the lander's own state keeps the plain name, and a scratch state such as the doctor's drill gets a name of its own, so its promotion never stops the lander's replay.
fn name(runtime: &Runtime) -> String {
    let lander = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/north"));
    if lander.as_deref() == Some(runtime.home.as_path()) {
        return UNIT.to_owned();
    }
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(&runtime.home, &mut hasher);
    format!("{UNIT}-{:016x}", std::hash::Hasher::finish(&hasher))
}
