use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use super::client::{Account, North};
use super::exec;
use super::notes::{Note, Notes};
use super::processes::{self, Process};
use super::product;
use super::report;
use super::terminal::{Size, Terminal};
use super::transcript::{Transcript, holds_number, screen_shows_reply, shorten};

const SIX_TIMES_SEVEN: &str = "Reply with only the number: what is 6 times 7?";
const CLAUDE_MODEL: &str = "haiku";
const SIZE: Size = Size { columns: 160, rows: 45 };
const SCREEN_WAIT: Duration = Duration::from_secs(15);
const LAUNCH_WAIT: Duration = Duration::from_mins(2);
const QUERY_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(300);

type Step = fn(&mut Journey) -> Result<String, String>;

const STEPS: [(&str, &str, Step); 3] = [
    ("1", "open `north fleet` in a 160x45 terminal", open_fleet),
    ("2", "Ctrl+N launches claude haiku from the form; its reply holds 42", launch_claude),
    ("3", "`north task check -- true` records exit 0 at HEAD", task_check),
];

pub struct Plan {
    pub stamp: u64,
    pub claude_account: Option<String>,
}

impl Plan {
    fn session(&self, provider: &str) -> String {
        format!("vn-{}-{provider}", self.stamp)
    }
}

pub struct Journey {
    north: North,
    notes: Notes,
    plan: Plan,
    terminal: Option<Terminal>,
}

struct FormPresses {
    provider: usize,
    account: usize,
}

pub fn walk(north: North, notes: Notes, plan: Plan) -> Option<&'static str> {
    let mut journey = Journey { north, notes, plan, terminal: None };
    for (index, (number, title, step)) in STEPS.iter().enumerate() {
        let started = Instant::now();
        let outcome = step(&mut journey);
        report::step(number, title, &outcome, started.elapsed());
        if outcome.is_err() {
            STEPS[index + 1..].iter().for_each(|(number, title, _)| report::skipped(number, title));
            return Some(number);
        }
    }
    None
}

fn open_fleet(journey: &mut Journey) -> Result<String, String> {
    let socket = format!("verify-north-{}", journey.plan.stamp);
    journey.notes.note(&Note::Terminal(socket.clone()));
    let program = journey.north.program().to_string_lossy().into_owned();
    let terminal = Terminal::open(&socket, journey.north.directory(), &SIZE, &["env", "NORTH_FLEET_TEST=1", &program, "fleet"])?;
    journey.notes.note(&Note::Watch(terminal.program()?));
    terminal.wait_for(SCREEN_WAIT, "the sessions list", |screen| screen.contains(" sessions "))?;
    let size = terminal.size()?;
    let expected = format!("{}x{}", SIZE.columns, SIZE.rows);
    journey.terminal = Some(terminal);
    if size == expected { Ok(format!("tmux -L {socket}, {size}")) } else { Err(format!("terminal is {size}, expected {expected}")) }
}

fn launch_claude(journey: &mut Journey) -> Result<String, String> {
    let account = match journey.plan.claude_account.clone() {
        Some(account) => account,
        None => journey.north.placed_claude_account()?,
    };
    let presses = form_presses(&journey.north.accounts()?, "claude", &account)?;
    let name = journey.plan.session("claude");
    journey.notes.note(&Note::Session(name.clone()));
    let before = processes::session_hosts();
    let terminal = journey.terminal()?;
    terminal.keys(&["C-n"])?;
    terminal.wait_for(SCREEN_WAIT, "the new session form", |screen| screen.contains(" new session "))?;
    fill_form(terminal, &presses, &name)?;
    let chosen = format!("‹ {account} (");
    terminal.wait_for(SCREEN_WAIT, "the filled form", |screen| {
        screen.contains("‹ claude ›") && screen.contains(&chosen) && screen.contains(&name) && screen.contains(CLAUDE_MODEL)
    })?;
    terminal.keys(&["Enter"])?;
    let reply = journey.await_reply(&name, SIX_TIMES_SEVEN, "42", LAUNCH_WAIT)?;
    terminal.wait_for(SCREEN_WAIT, "the reply 42", |screen| screen_shows_reply(screen, "42"))?;
    journey.watch_host(&name, &before)?;
    Ok(format!("{name} on claude/{account} replied {:?}", shorten(&reply)))
}

fn task_check(journey: &mut Journey) -> Result<String, String> {
    let task = format!("verify-{}", journey.plan.stamp);
    journey.north.run(&["task", "check", &task, "--", "true"], QUERY_WAIT)?;
    let head = product::head(journey.north.directory())?;
    let tasks = journey.north.tasks()?;
    let recorded = tasks.iter().find(|recorded| recorded["task_ref"] == task.as_str()).ok_or_else(|| format!("`north ledger summary --json` lists no task {task}"))?;
    let exit = recorded["last_check_exit"].as_i64();
    let sha = recorded["last_check_sha"].as_str();
    if exit == Some(0) && sha == Some(head.as_str()) {
        Ok(format!("{task} exit 0 at {}", &head[..head.len().min(12)]))
    } else {
        Err(format!("ledger holds {task} exit {exit:?} at {sha:?}; expected exit 0 at {head}"))
    }
}

impl Journey {
    fn terminal(&self) -> Result<&Terminal, String> {
        self.terminal.as_ref().ok_or_else(|| "the fleet TUI is not open".to_owned())
    }

    fn transcript(&self, name: &str) -> Result<Option<Transcript>, String> {
        match self.north.transcript(name) {
            Ok(transcript) => Ok(Some(transcript)),
            Err(error) if error.contains("no session named") => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn await_reply(&self, name: &str, question: &str, number: &str, limit: Duration) -> Result<String, String> {
        let mut last = format!("{name} does not exist yet");
        let reply = exec::wait_for(limit, POLL, || {
            let Some(transcript) = self.transcript(name)? else { return Ok(None) };
            last = transcript.state();
            Ok(transcript.reply_after(question).filter(|reply| holds_number(reply, number)))
        })?;
        reply.ok_or_else(|| format!("no reply holding {number} to {question:?} within {} s; {last}", limit.as_secs()))
    }

    fn watch_host(&self, name: &str, before: &BTreeSet<Process>) -> Result<(), String> {
        let pid = self.north.host_pid(name)?.ok_or_else(|| format!("{name} reports no session host pid"))?;
        let host = Process::of(pid).ok_or_else(|| format!("{name}'s session host {pid} is not running"))?;
        if !before.contains(&host) {
            self.notes.watch_all(std::iter::once(host).chain(processes::descendants(&[host])));
        }
        Ok(())
    }
}

fn form_presses(accounts: &[Account], provider: &str, label: &str) -> Result<FormPresses, String> {
    let mut providers: Vec<&str> = Vec::new();
    for account in accounts {
        if !providers.contains(&account.provider.as_str()) {
            providers.push(&account.provider);
        }
    }
    let provider_index = providers.iter().position(|known| *known == provider).ok_or_else(|| format!("`north fleet accounts` lists no {provider} account"))?;
    let labels: Vec<&str> = accounts.iter().filter(|account| account.provider == provider).map(|account| account.label.as_str()).collect();
    let account_index = labels.iter().position(|known| *known == label).ok_or_else(|| format!("no {provider} account named {label}; have {}", labels.join(", ")))?;
    Ok(FormPresses { provider: provider_index, account: account_index })
}

fn fill_form(terminal: &Terminal, presses: &FormPresses, name: &str) -> Result<(), String> {
    terminal.press("Right", presses.provider)?;
    terminal.keys(&["Tab"])?;
    terminal.press("Right", presses.account)?;
    terminal.keys(&["Tab", "C-u"])?;
    terminal.type_text(name)?;
    terminal.keys(&["Tab", "Tab"])?;
    terminal.type_text(CLAUDE_MODEL)?;
    terminal.keys(&["Tab"])?;
    terminal.type_text(SIX_TIMES_SEVEN)
}
