//! An evaluation run started and stopped through the outside broker for a lead whose domain reaches neither the user manager's bus nor the evaluation data directory (north#1414). The request is the words `eval-unit start finish-proof LABEL LEVEL`, `eval-unit state LABEL` or `eval-unit stop LABEL` and nothing else of the caller's: no command, path, environment or host. The broker answers only a fleet session whose own scope the caller runs in, whose record names the role `lead` and whose name the harness default `eval_unit_askers` lists. The unit's name is the eval prefix and the label, the commands are the closed list of `Form`, every path lies under the evaluation data directory, and the answer holds only the unit's name, state and result.

use std::ops::RangeInclusive;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::platform::harness::{DEFAULTS, document};
use sha2::{Digest, Sha256};

use super::lane_grant::{self, Host};
use super::request_checkout;

pub(super) const VERB: &str = "eval-unit";
const ASKERS: &str = "eval_unit_askers";
const LEAD: &str = "lead";
const FINISH_PROOF: &str = "finish-proof";
const UNIT_PREFIX: &str = "eval-b-north-";
const MARKER: &str = "north eval-unit";
const BUILD_HOST: &str = "cpx62b";
const ACCOUNT: &str = "a";
const DATA: &str = "eval-data/eval-b";
const NIX_PROFILE_BIN: &str = ".nix-profile/bin";
const SYSTEM_BIN: &str = "/run/current-system/sw/bin";
const SSH_AGENT: &str = "SSH_AUTH_SOCK";
const LABEL_LIMIT: usize = 40;
const LEVELS: RangeInclusive<u32> = 1..=60;
const SHAPE: &str = "eval-unit start finish-proof LABEL LEVEL | eval-unit state LABEL | eval-unit stop LABEL, with a LABEL of at most 40 lower-case letters, digits and hyphens that starts and ends with a letter or digit, and a LEVEL from 1 to 60";

/// A runner build the closed list runs: the full id of the landed commit whose `evals/b` it was built from, the name its file carries under `W/bin` and the sha256 of that file, so a new build is a new entry through review.
pub(super) struct Pin<'a> {
    pub commit: &'a str,
    pub built: &'a str,
    pub digest: &'a str,
}

pub(super) const FINISH_PROOF_RUNNER: Pin<'static> = Pin { commit: "e5ddfd73a5303060e5305e138c343fd15135822c", built: "34ac25ed50", digest: "b9fb1c86f7fd872949f572d5727d8f1bd759661aec5a8cb7a5ac913ae26ea312" };

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Label(String);

impl Label {
    fn parse(word: &str) -> Option<Self> {
        let edge = |at: Option<char>| at.is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit());
        let fits = word.len() <= LABEL_LIMIT && edge(word.chars().next()) && edge(word.chars().last()) && word.chars().all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-');
        fits.then(|| Self(word.to_owned()))
    }

    fn unit(&self) -> String {
        format!("{UNIT_PREFIX}{}", self.0)
    }

    fn of_unit(name: &str) -> Option<Self> {
        name.strip_prefix(UNIT_PREFIX).and_then(|rest| rest.strip_suffix(".service")).and_then(Self::parse)
    }
}

/// The closed list of what the broker starts: each form is a variant with its typed arguments.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Form {
    FinishProof { label: Label, level: u32 },
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ask {
    Start(Form),
    State(Label),
    Stop(Label),
}

impl Ask {
    /// The eval-unit request in `arguments`: `None` for any other request, and why one in other words is refused.
    pub(super) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        let label = |word: &str| Label::parse(word);
        let level = |word: &str| word.parse::<u32>().ok().filter(|level| LEVELS.contains(level) && level.to_string() == word);
        let ask = match rest {
            ["start", FINISH_PROOF, name, at] => label(name).zip(level(at)).map(|(label, level)| Ask::Start(Form::FinishProof { label, level })),
            ["state", name] => label(name).map(Ask::State),
            ["stop", name] => label(name).map(Ask::Stop),
            _ => None,
        };
        Some(ask.ok_or_else(|| format!("an eval-unit request is `{SHAPE}`")))
    }
}

/// A transient unit to start: its name, the marker that says this request started it, its environment, its main command and the command that runs after it ends, whatever its result.
pub(super) struct Unit {
    pub name: String,
    pub description: String,
    pub environment: Vec<(String, String)>,
    pub log: PathBuf,
    pub command: Vec<String>,
    pub after: Vec<String>,
}

pub(super) struct Shown {
    pub id: String,
    pub description: String,
    pub state: String,
    pub result: String,
}

/// The user manager as the broker reaches it; the tests replace it.
pub(super) trait Units {
    fn start(&self, unit: &Unit) -> Result<(), String>;
    fn show(&self, name: &str) -> Result<Shown, String>;
    fn stop(&self, name: &str) -> Result<(), String>;
}

pub(super) struct UserManager;

fn run(program: &str, arguments: &[String]) -> Result<String, String> {
    let output = Command::new(program).args(arguments).output().map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(format!("{program} {}: {}", arguments.first().map_or("", String::as_str), String::from_utf8_lossy(&output.stderr).trim()))
    }
}

impl Units for UserManager {
    fn start(&self, unit: &Unit) -> Result<(), String> {
        let log = format!("append:{}", unit.log.display());
        let mut arguments: Vec<String> = ["--user", "--quiet", "--unit", &unit.name, "--description", &unit.description].map(str::to_owned).to_vec();
        arguments.extend(unit.environment.iter().map(|(name, value)| format!("--setenv={name}={value}")));
        arguments.extend([format!("--property=StandardOutput={log}"), format!("--property=StandardError={log}"), format!("--property=ExecStopPost={}", unit.after.join(" ")), "--".to_owned()]);
        arguments.extend(unit.command.iter().cloned());
        run("systemd-run", &arguments).map(|_| ())
    }

    fn show(&self, name: &str) -> Result<Shown, String> {
        let said = run("systemctl", &["--user", "show", "-p", "Id", "-p", "Description", "-p", "ActiveState", "-p", "Result", name].map(str::to_owned))?;
        let field = |key: &str| said.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix('=')).unwrap_or_default().to_owned();
        Ok(Shown { id: field("Id"), description: field("Description"), state: field("ActiveState"), result: field("Result") })
    }

    fn stop(&self, name: &str) -> Result<(), String> {
        run("systemctl", &["--user", "stop", name].map(str::to_owned))?;
        let _ = run("systemctl", &["--user", "reset-failed", name].map(str::to_owned));
        Ok(())
    }
}

fn askers() -> Vec<String> {
    document(DEFAULTS)[ASKERS]["value"].as_array().into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect()
}

fn named(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// The name of the lead session on the harness list whose own scope `cgroup` names.
fn asking(cgroup: &str, host: &Host) -> Result<String, String> {
    let (directory, record) = lane_grant::own_session(cgroup, host, "an eval unit is asked for by a lead session, and this caller runs in no session's scope")?;
    let name = record["name"].as_str().filter(|name| named(name)).ok_or_else(|| format!("{} names no session", directory.display()))?;
    if record["role"].as_str() == Some(LEAD) && askers().iter().any(|asker| asker == name) {
        return Ok(name.to_owned());
    }
    Err(format!("{name} may not ask for an eval unit: only a lead session the harness default {ASKERS} lists does"))
}

fn plain(path: &Path) -> Result<String, String> {
    let text = path.to_string_lossy().into_owned();
    if text.is_empty() || text.chars().any(|character| character.is_whitespace() || "\"'\\%$;".contains(character)) {
        return Err(format!("{text} has a character a unit's command line cannot carry"));
    }
    Ok(text)
}

fn git(main: &request_checkout::Checkout, host: &Host, arguments: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git").arg("-C").arg(&main.top).args(arguments).env_clear().envs(host.environment.iter().cloned()).envs(main.pinned()).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

/// The runner the form runs: `W/bin/eval-b.SHA` for the short head of this host's north main checkout, the broker's copy of the runner built from `pin.commit`, which stands for this head only while `evals/b` has not changed since. The built file must have the digest of `pin` before anything else is read or copied; the copy is made when it is missing and refused when it differs.
fn runner(host: &Host, data: &Path, pin: &Pin) -> Result<PathBuf, String> {
    let built = data.join("bin").join(format!("eval-b.{}", pin.built));
    let bytes = built.symlink_metadata().is_ok_and(|file| file.is_file()).then(|| std::fs::read(&built).ok()).flatten().ok_or_else(|| format!("{} is not a built runner", built.display()))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != pin.digest {
        return Err(format!("{} has the sha256 {digest}, not the {} the closed list pins; a runner build is a separate matter", built.display(), pin.digest));
    }
    let main = host.code.iter().find_map(|root| request_checkout::registered(&host.code, &root.join("north/main")).ok()).ok_or_else(|| "this host keeps no north main checkout".to_owned())?;
    let same = git(&main, host, &["diff", "--quiet", pin.commit, "HEAD", "--", "evals/b"])?;
    match same.status.code() {
        Some(0) => {}
        Some(1) => return Err(format!("evals/b changed since {}, so the runner there no longer stands for this head; a runner build is a separate matter", pin.commit)),
        _ => return Err(format!("git diff {} HEAD -- evals/b: {}", pin.commit, String::from_utf8_lossy(&same.stderr).trim())),
    }
    let head = git(&main, host, &["rev-parse", "--short=10", "HEAD"])?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_owned();
    if head.is_empty() || !head.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("the north main checkout has no head".to_owned());
    }
    let copy = data.join("bin").join(format!("eval-b.{head}"));
    if copy.symlink_metadata().is_err() {
        std::fs::copy(&built, &copy).map_err(|error| format!("{}: {error}", copy.display()))?;
    }
    match copy.symlink_metadata().is_ok_and(|file| file.is_file()).then(|| std::fs::read(&copy).ok()).flatten() {
        Some(read) if read == bytes => Ok(copy),
        _ => Err(format!("{} is not a copy of {}", copy.display(), built.display())),
    }
}

impl Form {
    fn label(&self) -> &Label {
        match self {
            Form::FinishProof { label, .. } => label,
        }
    }

    /// The unit this form starts for the session `parent`: the commands are the runner's ladder, then its queue release, with the runner `runner` and the evaluation data directory `data`.
    fn unit(&self, parent: &str, runner: &Path, data: &Path, host: &Host) -> Result<Unit, String> {
        let Form::FinishProof { label, level } = self;
        let agent = host.environment.iter().find(|(name, value)| name == SSH_AGENT && Path::new(value).is_absolute()).map(|(_, value)| value.clone()).ok_or_else(|| "the broker holds no ssh agent socket to hand the unit".to_owned())?;
        let path = [host.home.join(".local/bin"), host.home.join(NIX_PROFILE_BIN), PathBuf::from(SYSTEM_BIN)].iter().map(|entry| plain(entry)).collect::<Result<Vec<_>, _>>()?.join(":");
        let (runner, data_text) = (plain(runner)?, plain(data)?);
        let at = |name: &str| format!("{data_text}/{name}");
        let command = [&runner, "ladder", "--workload", &at("workload"), "--level", &level.to_string(), "--count", "1", "--account", ACCOUNT, "--parent", parent, "--hosts", BUILD_HOST, "--work", &at("work"), "--results", &at(&format!("results-north-{}.jsonl", label.0))].map(str::to_owned).to_vec();
        let after = [&runner, "queue", "--archive", &at("queue"), "--hosts", BUILD_HOST].map(str::to_owned).to_vec();
        Ok(Unit { name: label.unit(), description: format!("{MARKER} {FINISH_PROOF} for {parent}"), environment: vec![("PATH".to_owned(), path), (SSH_AGENT.to_owned(), agent)], log: data.join("runs.log"), command, after })
    }
}

/// The unit `label` names, read when this request started it.
fn started(units: &dyn Units, label: &Label) -> Result<(String, Shown), String> {
    let name = label.unit();
    let shown = units.show(&name)?;
    let form = shown.description.strip_prefix(MARKER).and_then(|rest| rest.split_whitespace().next());
    if form == Some(FINISH_PROOF) && Label::of_unit(&shown.id).as_ref() == Some(label) {
        return Ok((name, shown));
    }
    Err(format!("{name} is no unit an eval-unit request started"))
}

/// Does what `ask` names for the lead whose own scope `cgroup` names on `host`, with the user manager `units` and the runner `pin`; the line saying what was done.
pub(super) fn serve(ask: Ask, cgroup: &str, host: &Host, units: &dyn Units, pin: &Pin) -> Result<String, String> {
    let parent = asking(cgroup, host)?;
    match ask {
        Ask::Start(form) => {
            let data = host.home.join(DATA);
            let unit = form.unit(&parent, &runner(host, &data, pin)?, &data, host)?;
            units.start(&unit)?;
            Ok(format!("{} started; its rows go to {}", unit.name, data.join(format!("results-north-{}.jsonl", form.label().0)).display()))
        }
        Ask::State(label) => started(units, &label).map(|(name, shown)| format!("{name}: {}, result {}", shown.state, shown.result)),
        Ask::Stop(label) => {
            let (name, _) = started(units, &label)?;
            units.stop(&name)?;
            Ok(format!("{name}: stopped"))
        }
    }
}

/// Answers the eval-unit request `asked` from a caller whose `/proc/PID/cgroup` reads `cgroup`, telling the client on its standard output what was done; a refusal is told to the client and returned for the broker's log.
pub(super) fn answer(asked: Result<Ask, String>, streams: Vec<OwnedFd>, cgroup: Option<&str>, host: &Host) -> Result<u8, String> {
    super::hold_request::tell(asked.and_then(|ask| serve(ask, cgroup.unwrap_or_default(), host, &UserManager, &FINISH_PROOF_RUNNER)), streams, "an eval unit request")
}

#[cfg(test)]
#[path = "eval_unit_request_tests.rs"]
pub(super) mod tests;
