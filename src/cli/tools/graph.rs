use std::time::Duration;

use north_graph_change::split::{Answer, Planner, Question, Tokens};

use crate::timers::oneshot::{self, Ask};

const PLANNING_LIMIT: Duration = Duration::from_secs(600);

fn admits(repository: &str) -> bool {
    crate::platform::orchestration::snapshot().is_ok_and(|policy| crate::pacing::placement::gate::public(repository, &policy))
}

fn ask(question: &Question) -> Result<Answer, String> {
    let Question { model, effort, system, input } = *question;
    let access = crate::cli::agents::claude_access();
    let measured = oneshot::ask_claude_measured(&Ask { model, effort, system, input }, PLANNING_LIMIT, &access, &mut |line| eprintln!("north graph split: {line}"))?;
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
