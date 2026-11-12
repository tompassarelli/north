//! `north proxy move [--to ACCOUNT] [--auto] [--dry-run]`: the coordinator's window follows the account with the most week headroom before
//! its own account runs out. The window is the one holding the conversation the fleet record names for the coordinator. Everything from
//! reading the windows on runs in a user unit, so a sandboxed shell, the coordinator's own included, can start it: the caller waits for the
//! plan unit and prints its result, and the plan unit starts the swap unit, which waits for the old window's turn to end and runs the steps.
mod on_desktop;
mod plan;
mod session;
mod steps;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::AccountOutlook;

use super::router;

use on_desktop::OnDesktop;
use plan::{STEPS, Verdict};
use session::{Window, arguments_of, environment_of, name_of, parent_of};

const COOLDOWN: Duration = Duration::from_secs(20 * 60);
const STAGE: &str = "NORTH_WINDOW_MOVE";
const PLAN_UNIT: (&str, Duration) = ("north-window-move", Duration::from_secs(120));
const SWAP_UNIT: (&str, Duration) = ("north-window-move-swap", Duration::from_secs(10 * 60));
const KEPT_ENVIRONMENT: [&str; 17] = ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LOCALE_ARCHIVE", "LOCALE_ARCHIVE_2_27", "TERMINFO_DIRS", "WAYLAND_DISPLAY", "XDG_RUNTIME_DIR", "XDG_SESSION_TYPE", "DBUS_SESSION_BUS_ADDRESS", "SSH_AUTH_SOCK", "NIX_LD", "NIX_LD_LIBRARY_PATH", "NORTH_DELIVERY_LIVENESS_REQUIRED"];
pub(super) const USAGE: &str = "usage: north proxy move [--to ACCOUNT] [--auto] [--dry-run]";

struct Request {
    to: Option<String>,
    dry_run: bool,
}

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let mut request = Request { to: None, dry_run: false };
    let mut auto = false;
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match *word {
            "--to" => request.to = Some(rest.next().ok_or_else(|| NorthError::Usage(USAGE.into()))?.to_string()),
            "--auto" => auto = true,
            "--dry-run" => request.dry_run = true,
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    if auto {
        return start_when_due(&router::open_fleet()?);
    }
    match std::env::var(STAGE).as_deref() {
        Ok("plan") => plan(&request),
        Ok("swap") => swap(&request),
        _ => in_unit(&request),
    }
}

pub(super) fn tom_windows() -> Vec<super::one_proxy::Window> {
    let windows = session::live_windows().into_iter().flatten().flatten().filter(|window| window.terminal);
    let active = windows.filter_map(|window| std::fs::metadata(session::activity_path(&window)).and_then(|meta| meta.modified()).ok().map(|active| (window, active)));
    active.map(|(window, active)| super::one_proxy::Window { conversation: window.session.id, account: window.account, active, pid: window.client }).collect()
}

pub(super) fn own_ancestors() -> Vec<u32> {
    std::iter::successors(Some(std::process::id()), |pid| parent_of(*pid).filter(|parent| *parent > 1)).collect()
}

pub(super) fn auto(fleet: &Fleet) {
    if let Err(error) = start_when_due(fleet) {
        eprintln!("north proxy move: {}", error.user_message());
    }
}

fn start_when_due(fleet: &Fleet) -> NorthResult<()> {
    let Some(record) = fleet.fleet_proxy()? else { return Ok(()) };
    let Verdict::Move { to, reason } = plan::decide(&record.account, &outlooks()?, None) else { return Ok(()) };
    if !cooled_down() {
        return Ok(());
    }
    println!("north proxy move: account {} to {to}: {reason}", record.account);
    start(SWAP_UNIT, "swap", &["--to", &to], false)
}

fn in_unit(request: &Request) -> NorthResult<()> {
    let mut words = Vec::new();
    if let Some(to) = &request.to {
        words.extend(["--to", to.as_str()]);
    }
    if request.dry_run {
        words.push("--dry-run");
    }
    start(PLAN_UNIT, "plan", &words, true)
}

fn start((unit, limit): (&str, Duration), stage: &str, words: &[&str], wait: bool) -> NorthResult<()> {
    let mut command = Command::new("systemd-run");
    command.args(["--user", "--collect", "--quiet", "--setenv=PATH"]).arg(format!("--unit={unit}")).arg(format!("--property=RuntimeMaxSec={}", limit.as_secs())).arg(format!("--setenv={STAGE}={stage}"));
    if wait {
        command.args(["--wait", "--pipe"]);
    }
    let status = command.arg("--").arg(std::env::current_exe()?).args(["proxy", "move"]).args(words).status()?;
    match (status.success(), wait) {
        (true, true) => Ok(()),
        (true, false) => {
            println!("north proxy move: started the move in unit {unit}; the new window opens now and the old one ends once its turn ends, within {} seconds", plan::END_WAIT.as_secs());
            Ok(())
        }
        (false, true) => Err(NorthError::State(format!("the window move unit {unit} exited {status}"))),
        (false, false) => Err(NorthError::State(format!("could not start the window move unit {unit}: {status}"))),
    }
}

fn plan(request: &Request) -> NorthResult<()> {
    let fleet = router::open_fleet()?;
    let window = recorded_window(&fleet)?;
    let Some(to) = destination(&window, request.to.as_deref())? else { return Ok(()) };
    let target = target_home(&to)?;
    let ready = session::ready_for(&window, &target);
    println!("north proxy move: window process {} holds conversation {} on account {}; the move runs:", window.client, window.session.id, window.account);
    STEPS.iter().enumerate().for_each(|(at, step)| println!("  {}. {}", at + 1, step.describe(&window, &to)));
    println!("north proxy move: replacement command: {}", replacement(&window, &to, ancestor_named(window.client, "ghostty").unwrap_or(0)).join(" "));
    println!("north proxy move: turn ended: {}, target profile ready: {}", session::turn_ended(&window), ready.as_ref().map_or_else(NorthError::user_message, |()| "yes".to_owned()));
    if request.dry_run {
        println!("north proxy move: dry run; nothing moved");
        return Ok(());
    }
    ready?;
    start(SWAP_UNIT, "swap", &["--to", &to], false)
}

fn swap(request: &Request) -> NorthResult<()> {
    stamp();
    let fleet = router::open_fleet()?;
    let window = recorded_window(&fleet)?;
    let Some(to) = destination(&window, request.to.as_deref())? else { return Ok(()) };
    let target = target_home(&to)?;
    session::ready_for(&window, &target)?;
    let mover = OnDesktop { fleet: &fleet, to: to.clone(), target };
    steps::perform(&mover, &window, &to)?;
    println!("north proxy move: moved the window to account {to}");
    Ok(())
}

fn recorded_window(fleet: &Fleet) -> NorthResult<Window> {
    let conversation = fleet.fleet_proxy()?.and_then(|record| record.conversation).ok_or_else(|| NorthError::Refused("the fleet records no conversation for the coordinator".into()))?;
    session::choose(session::live_windows()?, &conversation)
}

fn destination(window: &Window, requested: Option<&str>) -> NorthResult<Option<String>> {
    match plan::decide(&window.account, &outlooks()?, requested) {
        Verdict::Stay { reason } => {
            println!("north proxy move: staying on account {}: {reason}", window.account);
            Ok(None)
        }
        Verdict::Stranded { reason } => Err(NorthError::Refused(format!("the window on account {} cannot move: {reason}", window.account))),
        Verdict::Move { to, reason } => {
            println!("north proxy move: account {} to {to}: {reason}", window.account);
            Ok(Some(to))
        }
    }
}

fn target_home(to: &str) -> NorthResult<PathBuf> {
    router::account_home("claude", to).ok_or_else(|| NorthError::State(format!("no claude profile for account {to}")))
}

fn outlooks() -> NorthResult<Vec<AccountOutlook>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| NorthError::Usage("no home directory".into()))?;
    let db = crate::usage::open_ledger(&home, None, &crate::threads::client::refresh_mirrored_snapshot).map_err(NorthError::State)?;
    crate::usage::outlook::claude_accounts(&db, &crate::platform::orchestration::path(), Timestamp::now()).map_err(NorthError::State)
}

fn stamp_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/north/window-move.stamp"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn cooled_down() -> bool {
    let Some(stamp) = stamp_path() else { return false };
    let last = std::fs::read_to_string(&stamp).ok().and_then(|text| text.trim().parse::<u64>().ok()).unwrap_or(0);
    now_secs().saturating_sub(last) >= COOLDOWN.as_secs()
}

fn stamp() {
    if let Some(stamp) = stamp_path() {
        let _ = std::fs::write(stamp, now_secs().to_string());
    }
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

fn replacement(window: &Window, to: &str, terminal: u32) -> Vec<String> {
    let launcher = format!("{}/.local/bin/claude-{to}", std::env::var("HOME").unwrap_or_default());
    let environment = environment_of(window.client);
    let mut command = vec!["ghostty".to_owned()];
    command.extend(arguments_of(terminal).into_iter().skip(1).filter(|argument| !argument.starts_with("--working-directory")));
    command.push(format!("--working-directory={}", window.cwd.display()));
    command.extend(["-e".to_owned(), "env".to_owned(), "-i".to_owned()]);
    command.extend(environment.iter().filter(|(key, _)| KEPT_ENVIRONMENT.iter().any(|kept| key == kept)).map(|(key, value)| format!("{key}={value}")));
    command.extend(["bash".to_owned(), "-c".to_owned(), "\"$@\"; exec bash --posix".to_owned(), "claude-move".to_owned(), launcher]);
    command.extend(resume_arguments(window));
    command
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
