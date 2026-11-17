use std::path::Path;

use toml_edit::{DocumentMut, Item};

use north_lander::land::hand_in::{Lane, Road};
use north_safe_push::entry::{self, HostFacts};

const LANDING_MESSAGE: &str = "north-landing-message";

pub fn safe_push(arguments: &[String]) -> u8 {
    let base = format!("origin/{}", arguments.iter().skip_while(|word| *word != "--to").nth(1).map_or("main", String::as_str));
    if let Some(refusal) = north_worker_sandbox::worker_sandbox::lane_hand_in::safe_push_refusal(|| north_land_scans::review_accept::needed(Path::new("."), &base, "HEAD")) {
        eprintln!("{refusal}");
        return 1;
    }
    let request: Vec<String> = std::iter::once("safe-push".to_owned()).chain(arguments.iter().cloned()).collect();
    if let Some(code) = north_worker_sandbox::worker_sandbox::reenter_outside(&request) {
        return code;
    }
    entry::safe_push(arguments, &host_facts())
}

pub fn hand_in(arguments: &[String]) -> u8 {
    north_lander::land::hand_in::run(arguments, SafePush { verb: "hand-in", flags: &[] })
}

pub fn finish(arguments: &[String]) -> u8 {
    if let Some(mark) = std::env::current_dir().ok().and_then(|directory| crate::lanes::draft_mark::read(&directory)) {
        return north_lander::land::hand_in::finish(arguments, super::draft_road::Draft { mark });
    }
    north_lander::land::hand_in::finish(arguments, SafePush { verb: "finish", flags: &[] })
}

/// The verdict of a finish call's check run by the outside broker when this process runs in a worker sandbox, which keeps the finish record unwritable: the broker runs the command in the lane and writes the record from its exit code. `None` outside a sandbox.
pub(super) fn check_through_broker(root: &Path, check: &str, head: Option<&str>) -> Option<Result<(), (String, String)>> {
    if check.trim().is_empty() {
        return None;
    }
    let mut log = tempfile::tempfile().ok()?;
    let ran = north_worker_sandbox::worker_sandbox::finish_check(root, check, head.unwrap_or_default(), &log)?;
    let mut output = String::new();
    let _ = std::io::Seek::rewind(&mut log).and_then(|()| std::io::Read::read_to_string(&mut log, &mut output));
    Some(match ran {
        Ok(0) => {
            eprint!("{output}");
            Ok(())
        }
        Ok(code) => Err((format!("exited {code}"), output)),
        Err(why) => Err((format!("did not start: {why}"), output)),
    })
}

struct SafePush {
    verb: &'static str,
    flags: &'static [&'static str],
}

impl Road for SafePush {
    fn writable(&self, root: &Path) -> Result<(), String> {
        north_worker_sandbox::worker_sandbox::stranded_git_locks(root).map_or(Ok(()), Err)
    }

    fn check(&self, root: &Path, check: &str, head: Option<&str>) -> Option<Result<(), (String, String)>> {
        check_through_broker(root, check, head)
    }

    fn writes_finish_records(&self) -> bool {
        !north_worker_sandbox::worker_sandbox::confined()
    }

    fn take(self, lane: &Lane) -> u8 {
        let message = match landing_message(lane) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("{}: {error}; committed, NOT queued.", self.verb);
                return 1;
            }
        };
        match std::env::set_current_dir(&lane.root) {
            Ok(()) => safe_push(&["--to", lane.destination.as_str(), "--message", &message].into_iter().chain(self.flags.iter().copied()).map(str::to_owned).collect::<Vec<_>>()),
            Err(error) => {
                eprintln!("{}: {}: {error}", self.verb, lane.root.display());
                1
            }
        }
    }
}

fn landing_message(lane: &Lane) -> Result<String, String> {
    let output = std::process::Command::new("git").arg("-C").arg(&lane.root).args(["rev-parse", "--path-format=absolute", "--git-path", LANDING_MESSAGE]).output().map_err(|error| format!("git rev-parse: {error}"))?;
    if !output.status.success() {
        return Err(format!("git rev-parse --git-path {LANDING_MESSAGE}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    std::fs::write(&path, &lane.message).map_err(|error| format!("{path}: {error}"))?;
    Ok(path)
}

pub fn ledger_client(config: &DocumentMut, here: &str) -> bool {
    config
        .get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("ledger_client"))
        .and_then(Item::as_bool)
        .unwrap_or(false)
}

fn host_facts() -> HostFacts {
    HostFacts { ledger_client: north_capacity::capacity::host_table::config().is_ok_and(|config| ledger_client(&config, &crate::platform::host::name())), abandon }
}

fn abandon(arguments: &[String]) -> u8 {
    north_landing_gates::land_abandon::abandon(arguments, &super::abandon_witness::FleetWitness)
}
