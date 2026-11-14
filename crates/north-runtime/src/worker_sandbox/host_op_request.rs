//! The host operations a confined lead or the coordinator runs through the outside broker, whose domain reaches no other host over ssh and no user manager (north#1485). The request is the words `host-op state HOST UNIT`, `host-op restart HOST UNIT` or `host-op sync COPY` and nothing else of the caller's: no command, path, environment or login. UNIT is one the harness default `host_op_units` lists, COPY a name the harness default `host_copies` lists with its host, its source under the broker's home and its target under the operator's home on that host, and HOST one this host reaches over ssh as its operator, or this host itself. The broker answers only a fleet session whose own scope the caller runs in and whose record names the role `lead` or the coordinator's.

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::process::Command;

use north_base::platform::harness::{DEFAULTS, document};

use super::lane_grant::{self, Host};
use super::peer::Peer;

pub(super) const VERB: &str = "host-op";
const UNITS: &str = "host_op_units";
const COPIES: &str = "host_copies";
const ASKERS: [&str; 2] = ["lead", "proxy"];
const SHAPE: &str = "host-op state HOST UNIT | host-op restart HOST UNIT | host-op sync COPY, with a UNIT the harness default host_op_units lists and a COPY the harness default host_copies names";

/// A copy the broker refreshes: its name, the host it lives on, the directory it is copied from under the broker's home, the directory it is copied to under the operator's home there, and the entries left out.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct HostCopy {
    pub name: String,
    pub host: String,
    pub from: String,
    pub to: String,
    pub exclude: Vec<String>,
}

/// What the broker is allowed to touch: the north units and the copies the harness defaults list.
pub(super) struct Listed {
    pub units: Vec<String>,
    pub copies: Vec<HostCopy>,
}

impl Listed {
    pub(super) fn harness() -> Self {
        let defaults = document(DEFAULTS);
        let texts = |item: &toml_edit::Item| item.as_array().into_iter().flatten().filter_map(|word| word.as_str().map(str::to_owned)).collect::<Vec<_>>();
        let units = texts(&defaults[UNITS]["value"]);
        let copies = defaults[COPIES]["value"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                let table = entry.as_inline_table()?;
                let field = |key: &str| table.get(key).and_then(|value| value.as_str()).map(str::to_owned);
                let exclude = table.get("exclude").and_then(|value| value.as_array()).into_iter().flatten().filter_map(|word| word.as_str().map(str::to_owned)).collect();
                Some(HostCopy { name: field("name")?, host: field("host")?, from: field("from")?, to: field("to")?, exclude })
            })
            .collect();
        Self { units, copies }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ask {
    State { host: String, unit: String },
    Restart { host: String, unit: String },
    Sync(String),
}

fn named(word: &str) -> bool {
    !word.is_empty() && !word.starts_with('-') && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.@".contains(&byte))
}

impl Ask {
    /// The host-op request in `arguments`: `None` for any other request, and why one in other words is refused.
    pub(super) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        let ask = match rest {
            ["state", host, unit] if named(host) && named(unit) => Some(Ask::State { host: (*host).to_owned(), unit: (*unit).to_owned() }),
            ["restart", host, unit] if named(host) && named(unit) => Some(Ask::Restart { host: (*host).to_owned(), unit: (*unit).to_owned() }),
            ["sync", copy] if named(copy) => Some(Ask::Sync((*copy).to_owned())),
            _ => None,
        };
        Some(ask.ok_or_else(|| format!("a host-op request is `{SHAPE}`")))
    }
}

/// How the broker reaches a host: the operator's ssh destination of a host, `None` for this one, and a program run there or here; the tests replace it.
pub(super) trait Reach {
    fn destination(&self, host: &str) -> Result<Option<String>, String>;
    fn run(&self, destination: Option<&str>, program: &str, arguments: &[String]) -> Result<String, String>;
}

struct Ssh<'a>(&'a Host);

impl Reach for Ssh<'_> {
    fn destination(&self, host: &str) -> Result<Option<String>, String> {
        if host == north_base::platform::host::name() {
            return Ok(None);
        }
        north_offload::land_offload::operator_destination(host).map(Some)
    }

    fn run(&self, destination: Option<&str>, program: &str, arguments: &[String]) -> Result<String, String> {
        let mut command = match destination {
            Some(destination) => {
                let mut command = Command::new("ssh");
                command.args(["-o", "BatchMode=yes", destination, "--", program]);
                command
            }
            None => Command::new(program),
        };
        let output = command.args(arguments).env_clear().envs(self.0.environment.iter().cloned()).output().map_err(|error| format!("{program}: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!("{program} on {}: {}", destination.unwrap_or("this host"), String::from_utf8_lossy(&output.stderr).trim()))
        }
    }
}

/// The session the caller `peer` runs in, when its record names a lead or the coordinator.
fn asking(peer: &Peer, host: &Host) -> Result<String, String> {
    let (directory, record) = lane_grant::own_session(peer, host, "a host operation is asked for by a lead session or the coordinator, and this caller runs in no session's scope")?;
    let name = record["name"].as_str().filter(|name| !name.is_empty()).ok_or_else(|| format!("{} names no session", directory.display()))?;
    match record["role"].as_str() {
        Some(role) if ASKERS.contains(&role) => Ok(name.to_owned()),
        _ => Err(format!("{name} may not run a host operation: only a lead session or the coordinator does")),
    }
}

fn listed_unit<'a>(unit: &'a str, listed: &Listed) -> Result<&'a str, String> {
    listed.units.iter().any(|known| known == unit).then_some(unit).ok_or_else(|| format!("{unit} is no north unit the harness default {UNITS} lists"))
}

/// Does what `ask` names for the session the caller `peer` runs in on `host`, touching only what `listed` holds, through `reach`; the lines saying what was read or done.
pub(super) fn serve(ask: Ask, peer: &Peer, host: &Host, listed: &Listed, reach: &dyn Reach) -> Result<String, String> {
    asking(peer, host)?;
    let owned = |words: &[&str]| words.iter().map(|word| (*word).to_owned()).collect::<Vec<_>>();
    match ask {
        Ask::State { host: name, unit } => {
            let unit = listed_unit(&unit, listed)?;
            let destination = reach.destination(&name)?;
            let shown = reach.run(destination.as_deref(), "systemctl", &owned(&["--user", "show", "-p", "Id", "-p", "LoadState", "-p", "ActiveState", "-p", "SubState", "-p", "Result", "-p", "ActiveEnterTimestamp", unit]))?;
            let load = reach.run(destination.as_deref(), "cat", &owned(&["/proc/loadavg"]))?;
            Ok(format!("{shown}load {}", load.trim()))
        }
        Ask::Restart { host: name, unit } => {
            let unit = listed_unit(&unit, listed)?;
            let destination = reach.destination(&name)?;
            let own = destination.is_none() && unit.strip_suffix(".service") == Some(north_base::platform::session_scope::WORKER_BROKER_UNIT);
            let words: Vec<String> = ["--user", "restart"].into_iter().chain(own.then_some("--no-block")).chain([unit]).map(str::to_owned).collect();
            reach.run(destination.as_deref(), "systemctl", &words)?;
            Ok(if own { format!("{unit} restart queued on {name}; this broker answers before it restarts") } else { format!("{unit} restarted on {name}") })
        }
        Ask::Sync(name) => {
            let copy = listed.copies.iter().find(|copy| copy.name == name).ok_or_else(|| format!("{name} is no copy the harness default {COPIES} lists"))?;
            let from: PathBuf = host.home.join(&copy.from);
            if !from.symlink_metadata().is_ok_and(|entry| entry.is_dir()) {
                return Err(format!("{} is not a directory on this host, so {name} is not refreshed", from.display()));
            }
            let target = match reach.destination(&copy.host)? {
                Some(destination) => format!("{destination}:{}", copy.to),
                None => host.home.join(&copy.to).to_string_lossy().into_owned(),
            };
            let mut arguments = owned(&["-a", "--chmod=go-rwx"]);
            arguments.extend(copy.exclude.iter().map(|entry| format!("--exclude={entry}")));
            arguments.extend([format!("{}/", from.to_string_lossy().trim_end_matches('/')), format!("{}/", target.trim_end_matches('/'))]);
            reach.run(None, "rsync", &arguments)?;
            Ok(format!("{name} refreshed on {} from {}", copy.host, from.display()))
        }
    }
}

/// Answers the host-op request `asked` from the caller `peer`, telling the client on its standard output what was done; a refusal is told to the client and returned for the broker's log.
pub(super) fn answer(asked: Result<Ask, String>, streams: Vec<OwnedFd>, peer: &Peer, host: &Host) -> Result<u8, String> {
    super::hold_request::tell(asked.and_then(|ask| serve(ask, peer, host, &Listed::harness(), &Ssh(host))), streams, "a host operation")
}

#[cfg(test)]
#[path = "host_op_request_tests.rs"]
mod tests;
