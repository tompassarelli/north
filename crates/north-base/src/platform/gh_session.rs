use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::gh_origin::{self, Kind};
use super::gh_pool::{self, Pool};
use super::github_app::{self, Host};
use crate::foundation::clock::Timestamp;
use crate::foundation::exit::NO_VERDICT;

const USAGE: &str = "usage: north gh-session REAL_GH [ARGS...]

The gh a fleet session runs through the bin/gh shim: it calls GitHub as the Socrates Agent App
(a fresh installation token in GH_TOKEN, never Tom's login; on a host the laptop pushes that token
to, gex45 and ccx33, the pushed one while fresh, named on stderr) and counts the GitHub API units it spends,
so one session's scraping cannot exhaust a token the others share.
A call costs 1 unit, --paginate 20, --limit N one per 100 rows; help, version and config cost none.
A session may spend while the shared token's remaining hourly quota (gh api rate_limit, read at most once a minute)
stays above a reserve of 1000 units. Under the reserve the sessions that spent most this hour (at or above the
average of the live sessions) are refused first, with exit 75 and a message naming the token's remaining quota and the spend.
Spend is counted per rolling hour in ${XDG_RUNTIME_DIR}/north-gh-budget/SESSION. Outside a fleet session (no NORTH_FLEET_SESSION)
REAL_GH runs unchanged. A call that takes a session to 50% or 80% of its fair share (400 units an hour) tells that session once.
A hook (a gh call under north hook) may spend 10% of the fair share, so one hook cannot starve its session; north ops github budget
breaks each session's spend down by hook, tick or command, the north verb behind it and the endpoint,
and a refused call tells the proxy at most once an hour per session (north ops github budget lists every session's spend and the top callers).";

const SESSION_ENV: &str = "NORTH_FLEET_SESSION";
const FAIR_SHARE: u32 = 400;
const WINDOW_SECONDS: i64 = 3600;
const PAGINATE_COST: u32 = 20;
const ROWS_PER_UNIT: u32 = 100;
const FREE_COMMANDS: [&str; 6] = ["help", "version", "config", "alias", "completion", "extension"];
const SCRUBBED: [&str; 4] = ["GH_TOKEN", "GITHUB_TOKEN", "GH_ENTERPRISE_TOKEN", "GITHUB_ENTERPRISE_TOKEN"];
const ALARM_PERCENTS: [u32; 2] = [50, 80];
const PROXY: &str = "proxy";
const HOOK_SHARE_PERCENT: u32 = 10;

pub fn run(arguments: &[String]) -> u8 {
    if matches!(arguments, [word] if word == "-h" || word == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some((real, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 1;
    };
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty());
    let Some(session) = session else {
        return exec(Command::new(real).args(rest));
    };
    let budget = fair_share();
    let ledger = ledger_path(&session);
    let now = Timestamp::now().unix_seconds();
    let before = used(&fresh_spend(&ledger, now));
    let pool = shared_pool(real, rest, &ledger, now);
    let mint = || {
        let (token, note) = github_app::agent_token(&Host::from_env(), Timestamp::now().unix_seconds(), std::env::var("GH_TOKEN").ok())?;
        if let Some(note) = note {
            eprintln!("north gh-session: {note}");
        }
        Ok(token)
    };
    let origin = gh_origin::of_parent();
    match prepare(real, rest, &session, Share { budget, ledger: &ledger, pool: &pool }, now, &origin, mint) {
        Ok(mut command) => match super::gh_broker_route::route(&mut command) {
            Ok(()) => {
                send(budget_notices(&session, &ledger, before, cost(rest), budget, Timestamp::now().unix_seconds(), false));
                exec(command)
            }
            Err(message) => {
                eprintln!("north gh-session: {message}");
                1
            }
        },
        Err(Refusal::Budget(message)) => {
            eprintln!("north gh-session: {message}");
            NO_VERDICT
        }
        Err(Refusal::Shared(message)) => {
            eprintln!("north gh-session: {message}");
            send(budget_notices(&session, &ledger, before, cost(rest), budget, Timestamp::now().unix_seconds(), true));
            NO_VERDICT
        }
        Err(Refusal::Token(message)) => {
            eprintln!("north gh-session: no Socrates Agent App token ({message}); not falling back to a personal login");
            1
        }
    }
}

fn shared_pool(real: &str, arguments: &[String], ledger: &Path, now: i64) -> Pool {
    if cost(arguments) == 0 {
        return Pool { remaining: None, resets_at: 0, others: Vec::new() };
    }
    let reading = gh_pool::read(&ledger_path(""), real, now);
    let mine = ledger.file_name().map(|name| name.to_string_lossy().into_owned());
    let others = session_spends(now).iter().filter(|(name, _)| Some(name) != mine.as_ref()).map(|(_, spent)| used(spent)).collect();
    Pool { remaining: reading.map(|(remaining, _)| remaining), resets_at: reading.map_or(0, |(_, resets_at)| resets_at), others }
}

fn exec(mut command: impl std::borrow::BorrowMut<Command>) -> u8 {
    let command = command.borrow_mut();
    let error = command.exec();
    eprintln!("north gh-session: cannot run {}: {error}", command.get_program().to_string_lossy());
    127
}

pub fn fair_share() -> u32 {
    FAIR_SHARE
}

pub fn session_spends(now: i64) -> Vec<(String, Vec<Spend>)> {
    let directory = ledger_path("");
    let mut spends: Vec<(String, Vec<Spend>)> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().extension().is_none_or(|extension| extension != "tmp" && extension != "refused"))
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(), fresh_spend(&entry.path(), now)))
        .filter(|(_, spent)| !spent.is_empty())
        .collect();
    spends.sort_by(|left, right| used(&right.1).cmp(&used(&left.1)).then_with(|| left.0.cmp(&right.0)));
    spends
}

pub fn used(spent: &[Spend]) -> u32 {
    spent.iter().map(|spend| spend.cost).sum()
}

fn alarm_crossed(before: u32, units: u32, budget: u32) -> Option<u32> {
    let reached = |percent: u32, used: u32| u64::from(used) * 100 >= u64::from(percent) * u64::from(budget);
    ALARM_PERCENTS.into_iter().filter(|percent| !reached(*percent, before) && reached(*percent, before + units)).max()
}

fn budget_notices(session: &str, ledger: &Path, before: u32, units: u32, budget: u32, now: i64, refused: bool) -> Vec<(String, String)> {
    if units > 0 && refused {
        let marker = refusal_marker(ledger);
        let first = fs::read_to_string(&marker).ok().and_then(|text| text.trim().parse::<i64>().ok());
        if first.is_some_and(|at| now - at < WINDOW_SECONDS) {
            return Vec::new();
        }
        let _ = marker.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| fs::write(&marker, now.to_string()));
        return vec![(PROXY.to_owned(), format!("north gh-session: session {session} was refused a gh call because the shared GitHub token is under its reserve (it has spent {before} units this hour against a fair share of {budget}, the call costs {units}); its gh calls stay refused while the token is short and the session is among the top spenders (north ops github budget lists the callers)"))];
    }
    alarm_crossed(before, units, budget)
        .map(|percent| (session.to_owned(), format!("north gh-session: you reached {percent}% of your GitHub fair share ({} of {budget} units this hour); your gh calls are refused first if the shared token runs short (north ops github budget lists the callers)", before + units)))
        .into_iter()
        .collect()
}

fn send(notices: Vec<(String, String)>) {
    let Ok(program) = std::env::current_exe() else {
        return;
    };
    for (recipient, message) in notices {
        let _ = Command::new(&program).args(["fleet", "send", "--from", "service:north-gh-budget", &recipient, &message]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).env_remove("NORTH_RUNTIME_LINK").spawn();
    }
}

fn refusal_marker(ledger: &Path) -> PathBuf {
    let mut name = ledger.as_os_str().to_owned();
    name.push(".refused");
    PathBuf::from(name)
}

fn ledger_path(session: &str) -> PathBuf {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let name: String = session.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') { character } else { '_' }).collect();
    runtime.join("north-gh-budget").join(name)
}

#[derive(Clone, Copy)]
struct Share<'a> {
    budget: u32,
    ledger: &'a Path,
    pool: &'a Pool,
}

#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Budget(String),
    Shared(String),
    Token(String),
}

fn prepare(real: &str, arguments: &[String], session: &str, share: Share, now: i64, origin: &gh_origin::Origin, mint: impl FnOnce() -> Result<String, String>) -> Result<Command, Refusal> {
    let Share { budget, ledger, pool } = share;
    let units = cost(arguments);
    if units > 0 {
        admit(&fresh_spend(ledger, now), now, units, Share { budget, ledger, pool }, session, origin.kind)?;
    }
    let token = mint().map_err(Refusal::Token)?;
    if units > 0 {
        record(ledger, Spend { at: now, cost: units, kind: origin.kind, caller: origin.caller.clone(), endpoint: gh_origin::endpoint(arguments) });
    }
    let mut command = Command::new(real);
    command.args(arguments);
    for name in SCRUBBED {
        command.env_remove(name);
    }
    command.env("GH_TOKEN", token).env("GH_PROMPT_DISABLED", "1");
    Ok(command)
}

fn cost(arguments: &[String]) -> u32 {
    let help = arguments.iter().any(|word| matches!(word.as_str(), "-h" | "--help" | "--version"));
    if help || arguments.first().is_none_or(|word| FREE_COMMANDS.contains(&word.as_str())) {
        return 0;
    }
    let rows = |text: &str| text.parse::<u32>().ok().map(|rows| rows.div_ceil(ROWS_PER_UNIT));
    let mut cost = 1;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let extra = match word.as_str() {
            "--paginate" => Some(PAGINATE_COST),
            "--limit" | "-L" => words.next().and_then(|value| rows(value)),
            other => other.strip_prefix("--limit=").and_then(rows),
        };
        cost = cost.max(extra.unwrap_or(0));
    }
    cost
}

fn admit(spent: &[Spend], now: i64, units: u32, share: Share, session: &str, kind: Kind) -> Result<(), Refusal> {
    let Share { budget, pool, .. } = share;
    let hooks: u32 = spent.iter().filter(|spend| spend.kind == Kind::Hook).map(|spend| spend.cost).sum();
    let hook_share = budget * HOOK_SHARE_PERCENT / 100;
    if kind == Kind::Hook && hooks + units > hook_share {
        return Err(Refusal::Budget(format!("session {session}'s hooks have spent {hooks} of their {hook_share} GitHub API units this hour ({HOOK_SHARE_PERCENT}% of the fair share) and this call costs {units}; a hook is refused past that so it cannot starve the session")));
    }
    let total = used(spent);
    if !pool.refuses(total, units) {
        return Ok(());
    }
    let left = pool.remaining.unwrap_or(0);
    let refills = pool.resets_at.checked_sub(now).filter(|seconds| *seconds > 0).map_or_else(String::new, |seconds| format!("; the token's hour refills in {} min", (seconds + 59) / 60));
    Err(Refusal::Shared(format!("the shared GitHub token has {left} API units left this hour, under its reserve of {}; session {session} has spent {total} this hour (the average of the live sessions is {}) and this call costs {units}, so it waits while the heaviest spenders are held back{refills}", Pool::reserve(), pool.average(total))))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spend {
    pub at: i64,
    pub cost: u32,
    pub kind: Kind,
    pub caller: String,
    pub endpoint: String,
}

fn fresh_spend(ledger: &Path, now: i64) -> Vec<Spend> {
    let text = fs::read_to_string(ledger).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let (at, cost) = (fields.next()?.parse::<i64>().ok()?, fields.next()?.parse::<u32>().ok()?);
            let kind = Kind::parse(fields.next().unwrap_or(""));
            Some(Spend { at, cost, kind, caller: fields.next().unwrap_or("unknown").to_owned(), endpoint: fields.next().unwrap_or("unknown").to_owned() })
        })
        .filter(|spend| now - spend.at < WINDOW_SECONDS)
        .collect()
}

fn record(ledger: &Path, spend: Spend) {
    let mut kept = fresh_spend(ledger, spend.at);
    kept.push(spend);
    let body: String = kept.iter().map(|spend| format!("{} {} {} {} {}\n", spend.at, spend.cost, spend.kind.name(), spend.caller, spend.endpoint)).collect();
    let written = ledger.parent().map_or(Ok(()), fs::create_dir_all).and_then(|()| {
        let temporary = ledger.with_extension(format!("{}.tmp", std::process::id()));
        fs::File::create(&temporary)?.write_all(body.as_bytes())?;
        fs::rename(&temporary, ledger)
    });
    if let Err(error) = written {
        eprintln!("north gh-session: cannot record spend in {}: {error}", ledger.display());
    }
}

#[cfg(test)]
#[path = "gh_session_tests.rs"]
mod tests;
