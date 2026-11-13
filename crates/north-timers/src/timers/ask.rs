use std::io::Read;
use std::time::Duration;

use super::oneshot::{self, Asked};
use crate::runtime_access::RuntimeAccess;

const USAGE: &str = "usage: north check oneshot --model M [--effort E] --system TEXT [--seconds N] [--json] < INPUT";
const AT_LIMIT: u8 = 3;

struct Options {
    model: String,
    effort: String,
    system: String,
    limit: Duration,
    json: bool,
}

fn parse(arguments: &[String]) -> Option<Options> {
    let (mut model, mut effort, mut system, mut seconds, mut json) = (None, "low".to_owned(), None, 300, false);
    let mut rest = arguments.iter();
    while let Some(option) = rest.next() {
        if option == "--json" {
            json = true;
            continue;
        }
        let value = rest.next()?;
        match option.as_str() {
            "--model" => model = Some(value.clone()),
            "--effort" => effort.clone_from(value),
            "--system" => system = Some(value.clone()),
            "--seconds" => seconds = value.parse().ok()?,
            _ => return None,
        }
    }
    Some(Options { model: model?, effort, system: system?, limit: Duration::from_secs(seconds), json })
}

pub fn run(arguments: &[String], access: &RuntimeAccess) -> u8 {
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
    if options.json {
        return match oneshot::ask_claude_measured(&question, options.limit, access, &mut |line| eprintln!("north check oneshot: {line}")) {
            Ok(measured) => {
                let counts = serde_json::json!({"input": measured.input_tokens, "output": measured.output_tokens, "cache_read": measured.cache_read_tokens, "cache_write": measured.cache_write_tokens});
                println!("{}", serde_json::json!({"text": measured.text, "tokens": counts}));
                0
            }
            Err(reason) => {
                eprintln!("north check oneshot: {reason}");
                2
            }
        };
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
