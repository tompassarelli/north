use std::io::Read;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::oneshot::{self, Asked, Measured};
use crate::runtime_access::RuntimeAccess;

const USAGE: &str = "usage: north check oneshot --model M [--effort E] --system TEXT [--seconds N] [--json --account A --session S] < INPUT";
const AT_LIMIT: u8 = 3;

struct Options {
    model: String,
    effort: String,
    system: String,
    limit: Duration,
    json: bool,
    charge: Option<Charge>,
}

/// The subscription account a `--json` call is pinned to, and the session the call is recorded against.
struct Charge {
    account: String,
    session: String,
}

#[derive(Default)]
struct Given {
    model: Option<String>,
    effort: Option<String>,
    system: Option<String>,
    seconds: Option<u64>,
    json: bool,
    account: Option<String>,
    session: Option<String>,
}

impl Given {
    fn set(&mut self, option: &str, value: &str) -> Option<()> {
        match option {
            "--model" => self.model = Some(value.to_owned()),
            "--effort" => self.effort = Some(value.to_owned()),
            "--system" => self.system = Some(value.to_owned()),
            "--seconds" => self.seconds = Some(value.parse().ok()?),
            "--account" => self.account = Some(value.to_owned()),
            "--session" => self.session = Some(value.to_owned()),
            _ => return None,
        }
        Some(())
    }
}

fn parse(arguments: &[String]) -> Option<Options> {
    let mut given = Given::default();
    let mut rest = arguments.iter();
    while let Some(option) = rest.next() {
        if option == "--json" {
            given.json = true;
            continue;
        }
        given.set(option, rest.next()?)?;
    }
    let charge = match (given.account, given.session) {
        (None, None) => None,
        (Some(account), Some(session)) if given.json => Some(Charge { account, session }),
        _ => return None,
    };
    Some(Options {
        model: given.model?,
        effort: given.effort.unwrap_or_else(|| "low".to_owned()),
        system: given.system?,
        limit: Duration::from_secs(given.seconds.unwrap_or(300)),
        json: given.json,
        charge,
    })
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

fn record(charge: &Charge, options: &Options, measured: &Measured, started_ms: u64) -> Result<(), String> {
    let ledger = north_providers::ledger::Ledger::open_default().map_err(|error| error.to_string())?;
    let tokens = north_pace::router::pricing::TokenUse {
        input: i64::try_from(measured.input_tokens).unwrap_or(i64::MAX),
        output: i64::try_from(measured.output_tokens).unwrap_or(i64::MAX),
        cache_read: i64::try_from(measured.cache_read_tokens).unwrap_or(i64::MAX),
        cache_write: i64::try_from(measured.cache_write_tokens).unwrap_or(i64::MAX),
    };
    let call = north_providers::ledger::Call { session: &charge.session, account: &charge.account, model: &options.model, effort: &options.effort, tokens, started_ms, ended_ms: now_ms() };
    ledger.record_call(&call).map_err(|error| error.to_string())
}

/// Runs `north check oneshot`; `admit` says whether a call pinned to a `--account` may start on it now (the caller's spend view).
pub fn run(arguments: &[String], access: &RuntimeAccess, admit: &dyn Fn(&str) -> Result<(), String>) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if let Some(charge) = &options.charge
        && let Err(why) = admit(&charge.account)
    {
        eprintln!("north check oneshot: {why}");
        return 2;
    }
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("north check oneshot: cannot read the input: {error}");
        return 2;
    }
    let question = oneshot::Ask { model: &options.model, effort: &options.effort, system: &options.system, input: &input };
    if options.json {
        return json(&question, &options, access);
    }
    match oneshot::ask_claude(&question, options.limit, access, &mut |line| eprintln!("north check oneshot: {line}")) {
        Asked::Answer(answer) => {
            print!("{answer}");
            0
        }
        Asked::AtLimit => {
            eprintln!("north check oneshot: every claude account is at its usage limit");
            AT_LIMIT
        }
        Asked::Failed(reason) => {
            eprintln!("north check oneshot: {reason}");
            2
        }
    }
}

fn json(question: &oneshot::Ask, options: &Options, access: &RuntimeAccess) -> u8 {
    let started_ms = now_ms();
    let account = options.charge.as_ref().map(|charge| charge.account.as_str());
    match oneshot::ask_claude_measured_on(question, options.limit, account, access, &mut |line| eprintln!("north check oneshot: {line}")) {
        Ok(measured) => {
            if let Some(charge) = &options.charge
                && let Err(why) = record(charge, options, &measured, started_ms)
            {
                eprintln!("north check oneshot: the call was not recorded to the ledger: {why}");
                return 2;
            }
            println!("{}", oneshot::answer_json(&measured));
            0
        }
        Err(reason) => {
            eprintln!("north check oneshot: {reason}");
            2
        }
    }
}
