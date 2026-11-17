use std::io::Read;
use std::time::Duration;

use super::session::{self as oneshot, Asked};

const USAGE: &str = "usage: north check oneshot --model M [--effort E] --system TEXT [--seconds N] [--json] < INPUT";
const AT_LIMIT: u8 = 3;

struct Options {
    model: String,
    effort: String,
    system: String,
    limit: Duration,
    json: bool,
}

#[derive(Default)]
struct Given {
    model: Option<String>,
    effort: Option<String>,
    system: Option<String>,
    seconds: Option<u64>,
    json: bool,
}

impl Given {
    fn set(&mut self, option: &str, value: &str) -> Option<()> {
        match option {
            "--model" => self.model = Some(value.to_owned()),
            "--effort" => self.effort = Some(value.to_owned()),
            "--system" => self.system = Some(value.to_owned()),
            "--seconds" => self.seconds = Some(value.parse().ok()?),
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
    Some(Options {
        model: given.model?,
        effort: given.effort.unwrap_or_else(|| "low".to_owned()),
        system: given.system?,
        limit: Duration::from_secs(given.seconds.unwrap_or(300)),
        json: given.json,
    })
}

/// Runs `north check oneshot`.
pub fn run(arguments: &[String]) -> u8 {
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("north check oneshot: cannot read the input: {error}");
        return 2;
    }
    let question = oneshot::Ask { model: &options.model, effort: &options.effort, system: &options.system, input: &input };
    if super::gemini::serves(&options.model) {
        return gemini(&question, &options);
    }
    if options.json {
        return json(&question, &options);
    }
    match oneshot::ask_claude(&question, options.limit, &mut |line| eprintln!("north check oneshot: {line}")) {
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

fn gemini(question: &oneshot::Ask, options: &Options) -> u8 {
    if options.json {
        eprintln!("north check oneshot: --json counts Claude tokens into the ledger; a Gemini call takes no --json");
        return 2;
    }
    match super::gemini::ask(question, options.limit) {
        Ok(answer) => {
            print!("{answer}");
            0
        }
        Err(reason) => {
            eprintln!("north check oneshot: {reason}");
            2
        }
    }
}

fn json(question: &oneshot::Ask, options: &Options) -> u8 {
    match oneshot::ask_claude_measured(question, options.limit, &mut |line| eprintln!("north check oneshot: {line}")) {
        Ok(measured) => {
            println!("{}", oneshot::answer_json(&measured));
            0
        }
        Err(reason) => {
            eprintln!("north check oneshot: {reason}");
            2
        }
    }
}
