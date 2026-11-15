//! `north fleet dispatch queued --by HOST --item REPO#N [--box TEXT] -- WORDS`: what a launch dispatch placed on a queued host
//! runs when it reaches the front of that queue. It re-reads what was true when the box was queued, and drops the launch with
//! a printed reason when dispatch is paused here or on the host that queued it, the issue is closed, or the box is ticked.
use super::issues::IssueRef;
use super::pause;
use super::survey;
use north_pacing::pacing::placement::{self, gate};
use north_base::platform::process::run_within;
use std::time::Duration;

const REMOTE_WAIT: Duration = Duration::from_secs(20);

struct Live;

impl Live {
    fn pause_here(&self) -> Option<String> {
        pause::standing()
    }

    fn pause_on(&self, host: &str) -> Option<String> {
        let cfg = placement::config().ok()?;
        let ssh = gate::machines(&cfg).into_iter().find(|machine| machine.name == host)?.ssh;
        let words = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "--", ssh.as_str(), "north", "fleet", "dispatch", "paused"];
        let output = run_within("ssh", &words, REMOTE_WAIT).filter(|output| output.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn gone(&self, item: &IssueRef, box_text: Option<&str>) -> Option<String> {
        survey::github::recheck(item, box_text)
    }
}

struct Queued {
    by: String,
    item: IssueRef,
    box_text: Option<String>,
    command: Vec<String>,
}

fn parse(arguments: &[String]) -> Option<Queued> {
    let split = arguments.iter().position(|word| word == "--")?;
    let (head, command) = (&arguments[..split], arguments[split + 1..].to_vec());
    let (mut by, mut item, mut box_text) = (None, None, None);
    let mut words = head.iter();
    while let Some(flag) = words.next() {
        let value = words.next()?;
        match flag.as_str() {
            "--by" => by = Some(value.clone()),
            "--item" => item = IssueRef::parse(value),
            "--box" => box_text = Some(value.clone()),
            _ => return None,
        }
    }
    (!command.is_empty()).then_some(Queued { by: by?, item: item?, box_text, command })
}

/// The reason to drop the queued launch, or the command to run.
fn decide(queued: Queued, here: &str, probes: &Live) -> Result<Vec<String>, String> {
    if let Some(reason) = probes.pause_here() {
        return Err(format!("dispatch is paused on {here}: {reason}"));
    }
    if queued.by != here && let Some(reason) = probes.pause_on(&queued.by) {
        return Err(format!("dispatch is paused on {}, which queued it: {reason}", queued.by));
    }
    match probes.gone(&queued.item, queued.box_text.as_deref()) {
        Some(why) => Err(format!("{} {why}", queued.item)),
        None => Ok(queued.command),
    }
}

pub fn serve(arguments: &[String]) -> u8 {
    let Some(queued) = parse(arguments) else {
        eprintln!("usage: north fleet dispatch queued --by HOST --item REPO#N [--box TEXT] -- WORDS");
        return 2;
    };
    let item = queued.item.to_string();
    match decide(queued, &north_base::platform::host::name(), &Live) {
        Err(reason) => {
            println!("north fleet dispatch queued: dropped {item}: {reason}");
            0
        }
        Ok(command) => match std::env::current_exe().and_then(|north| std::process::Command::new(north).args(command).status()) {
            Ok(status) => status.code().and_then(|code| u8::try_from(code).ok()).unwrap_or(1),
            Err(error) => {
                eprintln!("north fleet dispatch queued: {error}");
                1
            }
        },
    }
}

/// `north fleet dispatch paused`: prints why dispatch is paused on this host and exits 0, or exits 1 when it is not.
pub fn paused() -> u8 {
    match pause::standing() {
        Some(reason) => {
            println!("{reason}");
            0
        }
        None => 1,
    }
}
