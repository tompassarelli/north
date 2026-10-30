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
const SLEEP_TASK: &str = "Run the shell command `sh -c \"sleep 120; echo done\"` in the foreground, not in the background, then say done.";
const NINE_PLUS_ONE: &str = "Reply with only the number: what is 9 plus 1?";
const TWO_PLUS_TWO: &str = "Reply with only the number: what is 2 plus 2?";
const READY: &str = "Reply with only the word: ready";
const CLAUDE_MODEL: &str = "haiku";
const SIZE: Size = Size { columns: 160, rows: 45 };
const SCREEN_WAIT: Duration = Duration::from_secs(15);
const LAUNCH_WAIT: Duration = Duration::from_mins(2);
const SLEEP_WAIT: Duration = Duration::from_secs(90);
const SLEEP_HOLD: Duration = Duration::from_secs(2);
const URGENT_WAIT: Duration = Duration::from_secs(10);
const IDLE_WAIT: Duration = Duration::from_mins(1);
const DEEPEN_WAIT: Duration = Duration::from_mins(3);
const CODEX_READY_WAIT: Duration = Duration::from_mins(3);
const ASK_SECS: u64 = 150;
const QUERY_WAIT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(300);

type Step = fn(&mut Journey) -> Result<String, String>;

const STEPS: [(&str, &str, Step); 6] = [
    ("1", "open `north fleet` in a 160x45 terminal", open_fleet),
    ("2", "Ctrl+N launches claude haiku from the form; its reply holds 42", launch_claude),
    ("3", "Alt+U urgent ask answers 10 within 10 s while sleep 120 runs", urgent_ask),
    ("4", "Ctrl+G deepen send records a deepen run with a system prompt sha", deepen_send),
    ("5", "the CLI asks a codex session; its reply holds 42", ask_codex),
    ("6", "`north task check -- true` records exit 0 at HEAD", task_check),
];

pub struct Plan {
    pub stamp: u64,
    pub claude_account: String,
    pub codex_account: String,
}

pub const NO_ACCOUNT: &str = "none";

const CODEX_STEP: &str = "5";

impl Plan {
    fn skips(&self, number: &str) -> bool {
        number == CODEX_STEP && self.codex_account == NO_ACCOUNT
    }

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
        if journey.plan.skips(number) {
            report::skipped(number, title);
            continue;
        }
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
    let account = journey.plan.claude_account.clone();
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

fn urgent_ask(journey: &mut Journey) -> Result<String, String> {
    let name = journey.plan.session("claude");
    let terminal = journey.terminal()?;
    terminal.type_text(SLEEP_TASK)?;
    terminal.keys(&["Enter"])?;
    journey.await_sleep(&name)?;
    terminal.type_text(NINE_PLUS_ONE)?;
    let sent = Instant::now();
    terminal.keys(&["M-u"])?;
    let reply = journey.await_reply(&name, NINE_PLUS_ONE, "10", URGENT_WAIT)?;
    let answered = sent.elapsed();
    terminal.wait_for(URGENT_WAIT.saturating_sub(answered), "the urgent reply 10", |screen| screen_shows_reply(screen, "10"))?;
    Ok(format!("replied {:?} {} ms after Alt+U, sleep still running", shorten(&reply), answered.as_millis()))
}

fn deepen_send(journey: &mut Journey) -> Result<String, String> {
    let name = journey.plan.session("claude");
    let terminal = journey.terminal()?;
    journey.settle(&name, terminal)?;
    terminal.type_text(TWO_PLUS_TWO)?;
    terminal.keys(&["C-g"])?;
    let deepened = |run: &serde_json::Value| {
        run["session"] == name.as_str() && run["deepen"] == true && run["system_prompt_sha"].as_str().is_some_and(|sha| !sha.is_empty())
    };
    let run = exec::wait_for(DEEPEN_WAIT, POLL, || Ok(journey.north.runs()?.into_iter().find(deepened)))?;
    let run = run.ok_or_else(|| format!("`north ledger runs --json` holds no deepen=true run with a system prompt sha for {name} within {} s", DEEPEN_WAIT.as_secs()))?;
    let sha: String = run["system_prompt_sha"].as_str().unwrap_or_default().chars().take(12).collect();
    Ok(format!("run {} deepen=true system_prompt_sha {sha}", run["run_id"]))
}

fn ask_codex(journey: &mut Journey) -> Result<String, String> {
    let account = journey.plan.codex_account.clone();
    let name = journey.plan.session("codex");
    journey.notes.note(&Note::Session(name.clone()));
    let before = processes::session_hosts();
    let directory = journey.north.directory().to_string_lossy().into_owned();
    let launch = ["fleet", "launch", "--provider", "codex", "--account", &account, "--name", &name, "--parent", "none", "--cwd", &directory, READY];
    journey.north.run(&launch, LAUNCH_WAIT)?;
    journey.watch_host(&name, &before)?;
    journey.await_settled(&name, CODEX_READY_WAIT)?;
    let reply = journey.north.run(&["fleet", "ask", &name, SIX_TIMES_SEVEN, "--timeout", &ASK_SECS.to_string()], Duration::from_secs(ASK_SECS) + QUERY_WAIT)?;
    if !holds_number(&reply, "42") {
        return Err(format!("`north fleet ask {name}` replied {:?}, which does not hold 42", shorten(&reply)));
    }
    Ok(format!("{name} on codex/{account} replied {:?}", shorten(&reply)))
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

    fn await_sleep(&self, name: &str) -> Result<(), String> {
        let mut last = String::new();
        let mut since: Option<Instant> = None;
        let running = exec::wait_for(SLEEP_WAIT, POLL, || {
            let Some(transcript) = self.transcript(name)? else { return Ok(None) };
            last = transcript.state();
            since = if sleep_running(&transcript)? { since.or_else(|| Some(Instant::now())) } else { None };
            Ok(since.filter(|since| since.elapsed() >= SLEEP_HOLD).map(drop))
        })?;
        running.ok_or_else(|| format!("sleep 120 was not running in the foreground for {} s within {} s; {last}", SLEEP_HOLD.as_secs(), SLEEP_WAIT.as_secs()))
    }

    fn await_settled(&self, name: &str, limit: Duration) -> Result<(), String> {
        let mut last = String::new();
        let settled = exec::wait_for(limit, POLL, || {
            let Some(transcript) = self.transcript(name)? else { return Ok(None) };
            last = transcript.state();
            Ok(matches!(transcript.status.as_str(), "idle" | "parked").then_some(()))
        })?;
        settled.ok_or_else(|| format!("{name} did not settle within {} s; {last}", limit.as_secs()))
    }

    fn settle(&self, name: &str, terminal: &Terminal) -> Result<(), String> {
        if self.transcript(name)?.is_some_and(|transcript| transcript.status == "working") {
            terminal.keys(&["C-c"])?;
        }
        self.await_settled(name, IDLE_WAIT)
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

fn sleep_running(transcript: &Transcript) -> Result<bool, String> {
    let Some(entries) = transcript.after(SLEEP_TASK) else { return Ok(false) };
    let Some(call) = entries.iter().position(|entry| entry.kind == "tool" && entry.text.contains("sleep 120")) else { return Ok(false) };
    if let Some(result) = entries[call + 1..].iter().find(|entry| entry.kind == "result") {
        return Err(format!("sleep 120 returned before the urgent ask, so nothing was left to interrupt: {:?}", shorten(&result.text)));
    }
    Ok(transcript.status == "working")
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
