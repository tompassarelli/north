//! The landing freeze: while one is placed, every hand-in on a host that holds it is refused before any check runs and before a ticket is written, naming the cause. `north land freeze` places or lifts it on this host and in each fleet host's state home for every login the lander reads there, so a build host's hand-ins are refused as the laptop's are (north#1281).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::after::route::{self, SSH_OPTIONS};
use super::lander::state_home;
use super::repositories::local_host;

pub const USAGE: &str = "usage: north land freeze --cause TEXT | north land freeze --lift";
const FILE: &str = "north/land-freeze.json";
const REMOTE_DIRECTORY: &str = "d=\"${XDG_STATE_HOME:-$HOME/.local/state}/north\"";

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Freeze {
    pub cause: String,
    pub placed_at: u64,
    pub placed_on: String,
}

pub fn path(state_home: &Path) -> PathBuf {
    state_home.join(FILE)
}

pub fn standing(state_home: &Path) -> Option<Freeze> {
    let text = fs::read_to_string(path(state_home)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The refusal a hand-in prints while this host holds a freeze, before it runs any check or writes a ticket.
pub fn unfrozen() -> Result<(), String> {
    standing(&state_home()).map_or(Ok(()), |freeze| {
        let minutes = now().saturating_sub(freeze.placed_at) / 60;
        Err(format!("the landing is frozen: {} (placed on {} {minutes} min ago; `north land freeze --lift` lifts it); NOT queued", freeze.cause, freeze.placed_on))
    })
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub fn run(arguments: &[String]) -> u8 {
    let act = match arguments {
        [flag, cause] if flag == "--cause" && !cause.trim().is_empty() => Act::Place(Freeze { cause: cause.trim().to_owned(), placed_at: now(), placed_on: local_host() }),
        [flag] if flag == "--lift" => Act::Lift,
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return 0;
        }
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    if let Err(error) = act.here(&state_home()) {
        eprintln!("north land freeze: ✗ {error}; NOT {}.", act.done());
        return 1;
    }
    println!("north land freeze: {} here", act.done());
    let failures = route::fleet_hosts().iter().filter(|host| !act.spread(host)).count();
    u8::from(failures > 0)
}

enum Act {
    Place(Freeze),
    Lift,
}

impl Act {
    fn done(&self) -> &'static str {
        match self {
            Self::Place(_) => "placed",
            Self::Lift => "lifted",
        }
    }

    fn here(&self, state_home: &Path) -> Result<(), String> {
        let target = path(state_home);
        match self {
            Self::Place(freeze) => {
                let parent = target.parent().ok_or_else(|| format!("{} has no parent", target.display()))?;
                fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
                let staged = target.with_extension("json.new");
                fs::write(&staged, serde_json::to_string(freeze).unwrap_or_default()).and_then(|()| fs::rename(&staged, &target)).map_err(|error| format!("{}: {error}", target.display()))
            }
            Self::Lift => match fs::remove_file(&target) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {error}", target.display())),
                _ => Ok(()),
            },
        }
    }

    fn script(&self) -> (String, String) {
        match self {
            Self::Place(freeze) => (format!("{REMOTE_DIRECTORY}; mkdir -p \"$d\" && cat > \"$d/land-freeze.json.new\" && mv \"$d/land-freeze.json.new\" \"$d/land-freeze.json\""), serde_json::to_string(freeze).unwrap_or_default()),
            Self::Lift => (format!("{REMOTE_DIRECTORY}; rm -f \"$d/land-freeze.json\""), String::new()),
        }
    }

    /// Places or lifts the freeze in `host`'s state home for every login the lander reads there, printing each; false when one was not reached.
    fn spread(&self, host: &str) -> bool {
        let (script, input) = self.script();
        let logins = match route::outboxes(host) {
            Ok(outboxes) => outboxes.into_iter().map(|outbox| outbox.login).collect::<Vec<_>>(),
            Err(error) => {
                eprintln!("north land freeze: ✗ {host}: {error}; NOT {} there.", self.done());
                return false;
            }
        };
        logins.iter().fold(true, |all, login| match ssh(login, &script, &input) {
            Ok(()) => {
                println!("north land freeze: {} on {host} as {login}", self.done());
                all
            }
            Err(error) => {
                eprintln!("north land freeze: ✗ {host}: {error}; NOT {} there, so rerun once it answers.", self.done());
                false
            }
        })
    }
}

fn ssh(login: &str, script: &str, input: &str) -> Result<(), String> {
    let mut child = Command::new("ssh").args(SSH_OPTIONS).args(["-T", login, "--", script]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|error| format!("ssh {login}: {error}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = child.wait_with_output().map_err(|error| format!("ssh {login}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!("ssh {login} exited {}: {}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()), String::from_utf8_lossy(&output.stderr).trim()))
}
