//! `north proxy move [--to ACCOUNT] [--auto] [--dry-run]`: Tom's interactive Claude window follows the account with the most week headroom
//! before its own account runs out. The conversation is carried into the target profile, resumed in a new terminal window that takes the old
//! window's place, and the old window and its background session end only after the new one is up on the target account at the end of a turn.
mod desktop;
mod plan;
mod session;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::AccountOutlook;

use plan::Verdict;
use session::{Window, arguments_of, environment_of, name_of, parent_of};

const SETTLE: Duration = Duration::from_secs(90);
const COOLDOWN: Duration = Duration::from_secs(20 * 60);
const KEPT_ENVIRONMENT: [&str; 17] = ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LOCALE_ARCHIVE", "LOCALE_ARCHIVE_2_27", "TERMINFO_DIRS", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XDG_SESSION_TYPE", "DBUS_SESSION_BUS_ADDRESS", "SSH_AUTH_SOCK", "NIX_LD", "NIX_LD_LIBRARY_PATH", "NORTH_DELIVERY_LIVENESS_REQUIRED"];
pub(super) const USAGE: &str = "usage: north proxy move [--to ACCOUNT] [--auto] [--dry-run]";

struct Request {
    to: Option<String>,
    auto: bool,
    dry_run: bool,
}

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let mut request = Request { to: None, auto: false, dry_run: false };
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match *word {
            "--to" => request.to = Some(rest.next().ok_or_else(|| NorthError::Usage(USAGE.into()))?.to_string()),
            "--auto" => request.auto = true,
            "--dry-run" => request.dry_run = true,
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    execute(&request)
}

pub(super) fn tom_window() -> Option<super::one_proxy::Window> {
    let window = session::find_window().ok()?;
    let active = std::fs::metadata(session::activity_path(&window)).and_then(|meta| meta.modified()).ok()?;
    Some(super::one_proxy::Window { conversation: window.session.id, account: window.account, active })
}

pub(super) fn auto() {
    if let Err(error) = execute(&Request { to: None, auto: true, dry_run: false }) {
        eprintln!("north proxy move: {}", error.user_message());
    }
}

fn outlooks() -> NorthResult<Vec<AccountOutlook>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| NorthError::Usage("no home directory".into()))?;
    let db = crate::usage::open_ledger(&home, None, &crate::threads::client::refresh_mirrored_snapshot).map_err(NorthError::State)?;
    crate::usage::outlook::claude_accounts(&db, &north_core::pace::target::policy_path(), Timestamp::now()).map_err(NorthError::State)
}

fn execute(request: &Request) -> NorthResult<()> {
    let window = match session::find_window() {
        Ok(window) => window,
        Err(_) if request.auto => return Ok(()),
        Err(error) => return Err(error),
    };
    let accounts = outlooks()?;
    let (to, reason) = match plan::decide(&window.account, &accounts, request.to.as_deref()) {
        Verdict::Stay { reason } => {
            if !request.auto {
                println!("north proxy move: staying on account {}: {reason}", window.account);
            }
            return Ok(());
        }
        Verdict::Stranded { .. } if request.auto => return Ok(()),
        Verdict::Stranded { reason } => return Err(NorthError::Refused(format!("the window on account {} cannot move: {reason}", window.account))),
        Verdict::Move { to, reason } => (to, reason),
    };
    if !request.auto {
        println!("north proxy move: account {} to {to}: {reason}", window.account);
    }
    if request.dry_run {
        let ready = session::ready_for(&window, &super::router::account_home("claude", &to).unwrap_or_default());
        println!("north proxy move: dry run; window process {} would resume in a new window, turn ended: {}, target profile ready: {}", window.client, session::turn_ended(&window), ready.map_or_else(|error| error.user_message(), |()| "yes".to_owned()));
        println!("north proxy move: replacement command: {}", replacement(&window, &to, ancestor_named(window.client, "ghostty").unwrap_or(0))?.join(" "));
        return Ok(());
    }
    if !session::turn_ended(&window) {
        if !request.auto {
            println!("north proxy move: the window's turn is still running; moving at the end of it");
        }
        return Ok(());
    }
    if request.auto {
        println!("north proxy move: account {} to {to}: {reason}", window.account);
        return detach(&to);
    }
    swap(&window, &to)
}

fn detach(to: &str) -> NorthResult<()> {
    if !cooled_down() {
        return Ok(());
    }
    let executable = std::env::current_exe()?;
    let status = Command::new("systemd-run")
        .args(["--user", "--collect", "--quiet", "--unit=north-window-move", "--property=RuntimeMaxSec=600", "--setenv=PATH"])
        .arg("--")
        .arg(executable)
        .args(["proxy", "move", "--to", to])
        .status()?;
    if status.success() {
        println!("north proxy move: started the move to account {to} in its own unit");
        Ok(())
    } else {
        Err(NorthError::State(format!("could not start the window move unit: {status}")))
    }
}

fn cooled_down() -> bool {
    let Some(stamp) = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/north/window-move.stamp")) else { return false };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let last = std::fs::read_to_string(&stamp).ok().and_then(|text| text.trim().parse::<u64>().ok()).unwrap_or(0);
    if now.saturating_sub(last) < COOLDOWN.as_secs() {
        return false;
    }
    std::fs::write(&stamp, now.to_string()).is_ok()
}

fn swap(window: &Window, to: &str) -> NorthResult<()> {
    let target = super::router::account_home("claude", to).ok_or_else(|| NorthError::State(format!("no claude profile for account {to}")))?;
    session::ready_for(window, &target)?;
    let mark = session::activity(window);
    let copied = session::carry(window, &target)?;
    println!("north proxy move: carried {copied} session items into {}", target.display());
    let old_shell = parent_of(window.client).filter(|pid| name_of(*pid).ends_with("sh"));
    let terminal = ancestor_named(window.client, "ghostty").ok_or_else(|| NorthError::State("the window's terminal process is not ghostty".into()))?;
    let old = desktop::window_of_terminal(terminal)?;
    let focused = desktop::focused()?;
    let command = replacement(window, to, terminal)?;
    let opened = desktop::open(&command)?;
    let client = match wait_for_client(window, &target, opened.pid) {
        Ok(client) => client,
        Err(error) => {
            desktop::terminate(opened.pid);
            return Err(error);
        }
    };
    println!("north proxy move: replacement window {} runs process {client} on {}", opened.id, environment_of(client).get("CLAUDE_CONFIG_DIR").map_or("?", String::as_str));
    if !session::turn_ended(window) || session::activity(window) != mark {
        desktop::terminate(opened.pid);
        return Err(NorthError::Refused("the old window took a turn while the replacement started; nothing moved".into()));
    }
    stop_background_session(window);
    match old_shell {
        Some(shell) => desktop::hang_up(shell),
        None => desktop::hang_up(window.client),
    }
    wait_gone(window.client);
    desktop::settle(&opened, &old, focused)?;
    println!("north proxy move: moved the window to account {to}");
    Ok(())
}

fn ancestor_named(pid: u32, name: &str) -> Option<u32> {
    let mut at = pid;
    for _ in 0..8 {
        at = parent_of(at)?;
        if name_of(at).contains(name) {
            return Some(at);
        }
    }
    None
}

fn replacement(window: &Window, to: &str, terminal: u32) -> NorthResult<Vec<String>> {
    let launcher = format!("{}/.local/bin/claude-{to}", std::env::var("HOME").unwrap_or_default());
    let environment = environment_of(window.client);
    let mut command = vec!["ghostty".to_owned()];
    command.extend(arguments_of(terminal).into_iter().skip(1).filter(|argument| !argument.starts_with("--working-directory")));
    command.push(format!("--working-directory={}", window.cwd.display()));
    command.extend(["-e".to_owned(), "env".to_owned(), "-i".to_owned()]);
    command.extend(environment.iter().filter(|(key, _)| KEPT_ENVIRONMENT.iter().any(|kept| key == kept)).map(|(key, value)| format!("{key}={value}")));
    command.extend(["bash".to_owned(), "-c".to_owned(), "\"$@\"; exec bash --posix".to_owned(), "claude-move".to_owned(), launcher]);
    command.extend(resume_arguments(window));
    Ok(command)
}

fn resume_arguments(window: &Window) -> Vec<String> {
    let mut arguments = Vec::new();
    let mut old = window.arguments.iter().peekable();
    while let Some(argument) = old.next() {
        if matches!(argument.as_str(), "--resume" | "-r") {
            arguments.extend(["--resume".to_owned(), window.session.id.clone()]);
            if old.peek().is_some_and(|next| !next.starts_with('-')) {
                old.next();
            }
        } else {
            arguments.push(argument.clone());
        }
    }
    if !arguments.iter().any(|argument| argument == &window.session.id) {
        arguments.extend(["--resume".to_owned(), window.session.id.clone()]);
    }
    if let Some(model) = window.session.model.as_ref().filter(|_| !arguments.iter().any(|argument| argument == "--model")) {
        arguments.extend(["--model".to_owned(), model.clone()]);
    }
    arguments
}

fn wait_for_client(window: &Window, target: &Path, terminal: u32) -> NorthResult<u32> {
    let deadline = Instant::now() + SETTLE;
    while Instant::now() < deadline {
        if let Some(client) = find_replacement(&window.session.id, terminal) {
            let record = target.join("sessions").join(format!("{client}.json"));
            let up = std::fs::read(&record).ok().and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok()).is_some_and(|record| record["status"] != "busy");
            if up && environment_of(client).get("CLAUDE_CONFIG_DIR").map(Path::new) == Some(target) {
                return Ok(client);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Err(NorthError::State(format!("the replacement window did not resume session on {} within {} seconds; the old window is untouched", target.display(), SETTLE.as_secs())))
}

fn find_replacement(id: &str, terminal: u32) -> Option<u32> {
    std::fs::read_dir("/proc").ok()?.flatten().filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok()).find(|pid| {
        let arguments = arguments_of(*pid);
        arguments.first().is_some_and(|first| first.ends_with("/claude")) && arguments.iter().any(|argument| argument == id) && ancestor_named(*pid, "ghostty") == Some(terminal)
    })
}

fn stop_background_session(window: &Window) {
    let Some(job) = &window.session.job else { return };
    let Ok(runtime) = std::fs::read_link(format!("/proc/{}/exe", window.client)) else { return };
    let home = std::env::var("HOME").unwrap_or_default();
    let status = Command::new(runtime).env_clear().env("HOME", home).env("PATH", std::env::var("PATH").unwrap_or_default()).env("CLAUDE_CONFIG_DIR", &window.profile).args(["stop", job]).status();
    match status {
        Ok(status) if status.success() => println!("north proxy move: stopped background session {job} on account {}", window.account),
        other => eprintln!("north proxy move: stopping background session {job} did not succeed: {other:?}"),
    }
}

fn wait_gone(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && Path::new(&format!("/proc/{pid}")).exists() {
        std::thread::sleep(Duration::from_millis(250));
    }
    if Path::new(&format!("/proc/{pid}")).exists() {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use session::Session;

    fn window(arguments: &[&str], model: Option<&str>) -> Window {
        Window {
            client: 1,
            profile: PathBuf::from("/profiles/claude-d"),
            account: "d".into(),
            arguments: arguments.iter().map(|argument| (*argument).to_owned()).collect(),
            cwd: PathBuf::from("/work"),
            session: Session { id: "S1".into(), transcript: PathBuf::from("/profiles/claude-d/projects/p/S1.jsonl"), job: None, model: model.map(str::to_owned) },
        }
    }

    #[test]
    fn a_bare_resume_picker_becomes_a_resume_of_the_carried_session_with_the_other_flags_kept() {
        let arguments = resume_arguments(&window(&["--resume", "--dangerously-skip-permissions"], Some("claude-opus-5-5")));
        assert_eq!(arguments, ["--resume", "S1", "--dangerously-skip-permissions", "--model", "claude-opus-5-5"]);
    }

    #[test]
    fn a_resume_naming_another_session_is_pointed_at_the_carried_one_and_a_missing_resume_is_added() {
        assert_eq!(resume_arguments(&window(&["--resume", "OLD", "--verbose"], None)), ["--resume", "S1", "--verbose"]);
        assert_eq!(resume_arguments(&window(&["--verbose"], None)), ["--verbose", "--resume", "S1"]);
    }
}
