use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::{
    agent_catalog, contract_probe, grounding, guard, health, hook, language_gate, session_host,
    threads_client, work_items,
};

mod cloud;
mod token_usage;
mod fleet;
mod fleet_duplicate;
mod fleet_list;
mod fleet_placement;
mod fleet_revive;
mod fleet_tier;
mod gate;
mod ledger;
mod orders;
mod probe_account;
mod relay;
mod safe_push_result;
#[cfg(test)]
mod tests;
mod tools;

use tools::Tool;

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Fleet(Vec<String>),
    Help,
    DesktopIdleState(Option<String>),
    Hook(Vec<String>),
    Threads(Vec<String>),
    Ledger(Vec<String>),
    Agents(Vec<String>),
    Work(Vec<String>),
    SessionHost(Vec<String>),
    Tool(Tool, Vec<String>),
    LanguageGate(Vec<String>),
    Health(Vec<String>),
    GuardLanguage,
    ContractProbe(Vec<String>),
    Grounding(Vec<String>),
}

const CLI_HELP: &str = "North — fleet session console

Usage: north [COMMAND]

Run north with no arguments to open the fleet screen.

Commands:
  wisp payoff [--since YYYY-MM-DD] [--json]      Measure Wisp agreement and native utilization
  health [--dry-run]                             Check harness invariants
  language-gate --repo DIR --base REV --head REV  Check language policy
  guard language                                PreToolUse language guard
  grounding [refresh|query|hook|distill|score]    Retrieve existing infrastructure and history
  work [list|search|show|claim|release|need|unneed|close]  Work items
  config agents [sync|status|on|off|path|inspect|check-hook-paths|check-owners]  Manage skills and hooks
  session-host --name N --generation G --state-dir D -- ARGV  Host one headless runtime
  dep-gate BASE HEAD                            Check dependency changes before landing
  review-gate id|check BASE HEAD PATH...         Print or check the reviewed patch-id of covered paths
  denylist scan (--config FILE | --south DIR) BASE HEAD [--ref NAME]  Scan an outgoing range, its messages, ref name and arriving files for deny-listed terms
  denylist attest --south DIR --repo OWNER/NAME [--repo ...] [--branch claude/X]  Post the denylist status on claude/ tips and dispatch Autoland when clean
  autoland-status --sha SHA --branch BRANCH      Read this lane's Autoland result
  autoland-wait --sha SHA --branch BRANCH --destination BRANCH  Wait for its landed result
  land-abandon [BRANCH]                          Delete a superseded or handed-back land branch from origin
  safe-push-result STATUS SHA REASON MINUTES URL  Print safe-push.s terminal result
  contract-probe --validate FIXTURE | --runtime PATH  Check the claude stream contract
  provenance [check|debt|audit]                  Audit grounding and target debt
  issue-box-lint [REPO#N...] [--repo REPO]       Check issue acceptance boxes
  threads ARGV                                  Run a NorthThreads command on the ledger host
  cloud [list|events|sweep|send|wait]            Read, message, wait on and sweep Anthropic cloud sessions
  dispatch [--dry-run]                          Launch ready smashcraft and wisp boxes on every free worker slot
  fleet [list|launch|send|ask|show|stop|forget]  Run headless agent sessions; no argument opens the screen
  private-desktop [start|control|capture|type]   Run or control an isolated private desktop
  ledger [runs|summary] [--json]                Fleet turn records, cost and task verdicts
  orders [list|add TEXT...]                     Tom's standing orders, rendered into every fleet launch and revive
  task [landed REF SHA | check REF -- CMD...]   Record a verdict north observed itself
  gate [open --session S --question Q --option O... --recommend N --default N --default-after D | answer ID N [--note TEXT] | list [--json]]  Open or answer a decision gate
  follows-release [--repo-dir DIR]             Start queued boxes after predecessors land
  capacity admission < FACTS.json               Decide one machine-capacity admission
  capacity laptop-auto [--dry-run]              Sample laptop gaming, stop agent game load while Tom plays, reap orphaned scopes
  capacity game-reserve on [--auto]|off|status Reserve CPUs for Tom's game, moving it only once it has run 60 s
  nexus-ram record                              Append one Nexus memory, CPU steal and pressure row (minutely timer)
  nexus-ram report [--days N]                   Judge Nexus RAM headroom: UPGRADE or STAY, steal minutes and longest row gap
  reclaim [--dry-run] [--idle-hours H]          Remove /tmp scratch and Wisp LAN pair copies idle 24 h and held by no process or pool (hourly timer)
  runtime update                                Promote origin/main as this host's north runtime with its own tree
  runtime deploy HOST...                        Build origin/main here, copy it to each HOST over ssh and promote it there
  help                                          Show this help

Options:
  -h, --help  Show this help";

fn parse_command(arguments: impl IntoIterator<Item = String>) -> NorthResult<Command> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.is_empty() {
        return Ok(Command::Fleet(Vec::new()));
    }
    if arguments[0] == "desktop-idle-state" {
        return Ok(Command::DesktopIdleState(arguments.get(1).cloned()));
    }
    if let Some(command) = forwarded(&arguments[0], arguments[1..].to_vec()) {
        return Ok(command);
    }
    if arguments.len() == 1 && matches!(arguments[0].as_str(), "-h" | "--help" | "help") {
        return Ok(Command::Help);
    }
    if arguments.len() >= 2 && arguments[0] == "config" && arguments[1] == "agents" {
        return Ok(Command::Agents(arguments[2..].to_vec()));
    }
    if arguments == ["guard", "language"] {
        return Ok(Command::GuardLanguage);
    }
    let kind = if arguments[0].starts_with('-') {
        "option"
    } else {
        "command"
    };
    Err(NorthError::Usage(format!(
        "unrecognized {kind} '{}'\n\nUsage: north [COMMAND]\n\nRun 'north' to open the TUI, or 'north --help' for help.",
        arguments.join(" ")
    )))
}

fn forwarded(command: &str, rest: Vec<String>) -> Option<Command> {
    if let Some(tool) = Tool::named(command) {
        return Some(Command::Tool(tool, rest));
    }
    Some(match command {
        "hook" => Command::Hook(rest),
        "threads" => Command::Threads(rest),
        "fleet" => Command::Fleet(rest),
        "ledger" | "task" => Command::Ledger([vec![command.to_owned()], rest].concat()),
        "orders" | "gate" => Command::Fleet([vec![command.to_owned()], rest].concat()),
        "work" => Command::Work(rest),
        "session-host" => Command::SessionHost(rest),
        "language-gate" => Command::LanguageGate(rest),
        "health" => Command::Health(rest),
        "contract-probe" => Command::ContractProbe(rest),
        "grounding" => Command::Grounding(rest),
        _ => return None,
    })
}

pub(crate) fn main() -> ExitCode {
    match parse_command(env::args().skip(1)) {
        Ok(command) => dispatch(command),
        Err(error) => failure(error),
    }
}

fn dispatch(command: Command) -> ExitCode {
    match command {
        Command::DesktopIdleState(state) => match state {
            Some(value) if guard::desktop_action::record_idle(value == "idle").is_ok() => ExitCode::SUCCESS,
            _ => ExitCode::from(1),
        },
        Command::Hook(arguments) => match hook::run(&arguments) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("{message}");
                ExitCode::from(2)
            }
        },
        Command::Threads(arguments) => ExitCode::from(threads_client::run(&arguments)),
        Command::Fleet(arguments) => fleet::run(&arguments),
        Command::Ledger(arguments) => ledger::run(&arguments),
        Command::Help => in_runtime(async {
            println!("{CLI_HELP}");
            Ok(ExitCode::SUCCESS)
        }),
        Command::Agents(arguments) => in_runtime(async move {
            agent_catalog::run(&arguments).map(|()| ExitCode::SUCCESS)
        }),
        Command::Work(arguments) => in_runtime(async move {
            work_items::run(&arguments).await.map(|()| ExitCode::SUCCESS)
        }),
        Command::SessionHost(arguments) => in_runtime(async move {
            session_host::run(&arguments, fleet::host_watch).await.map(ExitCode::from)
        }),
        Command::Tool(tool, arguments) => tool.run(&arguments),
        Command::LanguageGate(arguments) => in_runtime(async move { Ok(language_gate::run_gate(&arguments)) }),
        Command::Health(arguments) => in_runtime(async move { Ok(health::run(&arguments)) }),
        Command::GuardLanguage => in_runtime(async { Ok(language_gate::run_guard()) }),
        Command::ContractProbe(arguments) => in_runtime(async move {
            contract_probe::run(&arguments, probe_account::invocation).await.map(ExitCode::from)
        }),
        Command::Grounding(arguments) => in_runtime(async move {
            grounding::main(&arguments).map(|code| ExitCode::from(code as u8))
        }),
    }
}

#[tokio::main]
async fn in_runtime(work: impl Future<Output = NorthResult<ExitCode>>) -> ExitCode {
    match work.await {
        Ok(code) => code,
        Err(error) => failure(error),
    }
}

fn failure(error: NorthError) -> ExitCode {
    eprintln!("error: {}", error.user_message());
    if matches!(error, NorthError::Usage(_)) {
        ExitCode::from(2)
    } else {
        ExitCode::FAILURE
    }
}
