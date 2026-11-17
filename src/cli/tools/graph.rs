use std::time::Duration;

use north_graph_change::split::{Answer, Planner, Question, Tokens};

use crate::cli::oneshot::session::{self as oneshot, Ask};

const PLANNING_LIMIT: Duration = Duration::from_secs(600);

const PUBLIC_REPOS: [&str; 3] = ["north", "wisp", "makgora"];

pub(super) fn admits(repository: &str) -> bool {
    crate::platform::orchestration::snapshot().is_ok_and(|policy| {
        let rules = policy.get("placement");
        let mut public = strings(rules.and_then(|placement| placement.get("public_repos")));
        if public.is_empty() {
            public = PUBLIC_REPOS.iter().map(|name| (*name).to_owned()).collect();
        }
        !repository.is_empty() && !strings(rules.and_then(|placement| placement.get("private_repos"))).iter().any(|name| name == repository) && public.iter().any(|name| name == repository)
    })
}

fn strings(item: Option<&toml_edit::Item>) -> Vec<String> {
    item.and_then(toml_edit::Item::as_array).map(|array| array.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

pub(super) fn ask(question: &Question) -> Result<Answer, String> {
    let Question { model, effort, system, input } = *question;
    let measured = oneshot::ask_claude_measured(&Ask { model, effort, system, input }, PLANNING_LIMIT, &mut |line| eprintln!("north graph split: {line}"))?;
    let tokens = Tokens { input: measured.input_tokens, output: measured.output_tokens, cache_read: measured.cache_read_tokens, cache_write: measured.cache_write_tokens };
    Ok(Answer { text: measured.text, tokens })
}

pub(super) fn graph(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "ready" => north_graph_ground::ready_command(rest),
        Some((verb, rest)) if verb == "ask" => north_graph_change::ask(rest),
        Some((verb, rest)) if verb == "split" => north_graph_change::split::run(rest, &Planner { admits: &admits, ask: &ask }),
        _ => north_graph_ingest::run(arguments),
    }
}
