use std::path::Path;

use toml_edit::{DocumentMut, Item};

use north_lander::land::hand_in::{Admit, Lane, Road};
use north_safe_push::entry::{self, HostFacts};

const LANDING_MESSAGE: &str = "north-landing-message";

pub fn safe_push(arguments: &[String]) -> u8 {
    if let Some(target) = entry::enqueues_to(arguments) {
        let base = format!("origin/{target}");
        if let Some(refusal) = north_worker_sandbox::worker_sandbox::lane_hand_in::safe_push_refusal(|| north_land_scans::review_accept::needed(Path::new("."), &base, "HEAD")) {
            eprintln!("{refusal}");
            return 1;
        }
    }
    let request: Vec<String> = std::iter::once("safe-push".to_owned()).chain(arguments.iter().cloned()).collect();
    if let Some(code) = north_worker_sandbox::worker_sandbox::reenter_outside(&request) {
        return code;
    }
    entry::safe_push(arguments, &host_facts())
}

pub fn hand_in(arguments: &[String]) -> u8 {
    north_lander::land::hand_in::run(arguments, SafePush { verb: "hand-in", flags: &["--require-check-only"], requires_check_only: true })
}

pub fn finish(arguments: &[String]) -> u8 {
    if let Some(mark) = std::env::current_dir().ok().and_then(|directory| crate::lanes::draft_mark::read(&directory)) {
        return north_lander::land::hand_in::finish(arguments, super::draft_road::Draft { mark });
    }
    north_lander::land::hand_in::finish(arguments, SafePush { verb: "finish", flags: &[], requires_check_only: false })
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
    requires_check_only: bool,
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

    fn requires_check_only(&self) -> bool {
        self.requires_check_only
    }

    fn admit(&self, admit: &Admit) -> Result<(), String> {
        if !self.requires_check_only {
            return Ok(());
        }
        let root = &admit.lane.root;
        let destination = &admit.lane.destination;
        if north_lander::land::enqueue::confined_worker_here() {
            refuse_worker_ring_zero_on_hand_in(root, destination, admit.relative)?;
        }
        if admit.uncommitted {
            return Err(format!("the lane has uncommitted changes and has not passed safe-push --check-only at its current head ({}); commit the changes and run safe-push --check-only first", admit.head));
        }
        if edits_head_message(root, admit.requested_message) {
            return Err(format!("--message differs from HEAD ({}); amend the commit and run safe-push --check-only first", admit.head));
        }
        let origin = git_text(root, &["remote", "get-url", "--push", "origin"]).ok_or_else(|| "origin has no push URL".to_owned())?;
        let dir = north_lander::land::check_only::store_dir(&north_landing::land::lander::state_home());
        if !north_lander::land::check_only::passed(&dir, &origin, destination, admit.head) {
            return Err(format!("the lane has not passed safe-push --check-only at its current head ({}); run safe-push --check-only first", admit.head));
        }
        Ok(())
    }

    fn take(self, lane: &Lane) -> u8 {
        let mut args = vec!["--to".to_owned(), lane.destination.clone()];
        if !self.requires_check_only {
            let message = match landing_message(lane) {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("{}: {error}; committed, NOT queued.", self.verb);
                    return 1;
                }
            };
            args.extend(["--message".to_owned(), message]);
        }
        args.extend(self.flags.iter().copied().map(str::to_owned));
        match std::env::set_current_dir(&lane.root) {
            Ok(()) => safe_push(&args),
            Err(error) => {
                eprintln!("{}: {}: {error}", self.verb, lane.root.display());
                1
            }
        }
    }
}

fn refuse_worker_ring_zero_on_hand_in(root: &Path, destination: &str, relative: &[String]) -> Result<(), String> {
    let base_ref = format!("refs/remotes/origin/{destination}");
    let mut touched = north_land_scans::review_accept::ring_zero_touched(root, &base_ref, "HEAD")?;
    let defaults = north_base::platform::harness::DEFAULTS;
    let tracked = git_text(root, &["ls-tree", "-z", "-r", "--name-only", &base_ref, "--", defaults]).ok_or_else(|| format!("git could not inspect {defaults} at {base_ref}"))?;
    if !tracked.trim_matches('\0').is_empty() {
        let text = git_text(root, &["show", &format!("{base_ref}:{defaults}")]).ok_or_else(|| format!("git could not read {defaults} at {base_ref}"))?;
        let lists = text.parse::<DocumentMut>().map_err(|error| format!("{defaults} at {base_ref} does not parse: {error}"))?;
        let named = north_base::platform::trust_boundary::ring_zero(&lists, relative).ok_or_else(|| format!("{defaults} at {base_ref} lists no trust_boundary_paths"))?;
        touched.extend(named.into_iter().map(str::to_owned));
    }
    if relative.iter().any(|file| file == ".safe-push") {
        touched.push(".safe-push".to_owned());
    }
    touched.sort();
    touched.dedup();
    if touched.is_empty() {
        return Ok(());
    }
    Err(format!("ring-0 paths are write-denied to workers ({}); make this change from a session the owner types into", touched.join(", ")))
}

fn edits_head_message(root: &Path, requested: &str) -> bool {
    let requested = requested.trim();
    if requested.is_empty() {
        return false;
    }
    let Some(full) = git_text(root, &["show", "-s", "--format=%B", "HEAD"]) else { return true };
    let subject = full.lines().next().unwrap_or_default().trim();
    requested != full.trim() && requested != subject
}

fn git_text(root: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
    north_landing_gates::land_abandon::abandon(arguments, &super::abandon_witness::PushWitness)
}
