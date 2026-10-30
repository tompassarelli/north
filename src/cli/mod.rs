use std::env;
use std::process::ExitCode;

use crate::error::{NorthError, NorthResult};
use crate::fleet::session_host;
use crate::{agent_catalog, contract_probe, grounding, guard, health, hook, language_gate, work_items};

mod abandon_witness;
mod agents;
mod cloud;
mod cloud_handback;
mod token_usage;
mod fleet;
mod fleet_admission;
mod fleet_claims;
mod fleet_duplicate;
mod fleet_list;
mod fleet_placement;
mod fleet_revive;
mod fleet_tier;
mod fleet_tree;
mod fleet_view;
mod gate;
mod ledger;
mod orders;
mod probe_account;
mod relay;
mod safe_push_result;
#[cfg(test)]
mod tests;
mod tools;
mod worker_ledger;

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
  citation-check [--root DIR] [--list]          Fail on a doc, skill or hook that cites a north subcommand the CLI does not define
  language-gate --repo DIR --base REV --head REV  Check language policy
  code-health imports                           Resolve the imports of files given as JSON on stdin
  guard language                                PreToolUse language guard
  grounding [refresh|query|hook|distill|score]    Retrieve existing infrastructure and history
  work [list|search|show|claim|release|need|unneed|close]  Work items
  agents [status|stale|inspect|path|on|off|sync|busy|plan|pace|usage|presence|flow|news|routines|report]  Run the agent catalog, plan, usage and routines commands
  config agents [sync|status|on|off|path|inspect|check-hook-paths|check-owners]  Manage skills and hooks
  session-host --name N --generation G --state-dir D -- ARGV  Host one headless runtime
  dep-gate BASE HEAD                            Check dependency changes before landing
  review-gate id|check BASE HEAD PATH... | reviewers BASE HEAD  Print or check the reviewed patch-id of covered paths, or list the reviewers
  denylist scan (--config FILE | --south DIR) BASE HEAD [--ref NAME]  Scan an outgoing range, its messages, ref name and arriving files for deny-listed terms
  denylist attest --south DIR --repo OWNER/NAME [--repo ...] [--branch claude/X]  Post the denylist status on claude/ tips and dispatch Autoland when clean
  autoland-status --sha SHA --branch BRANCH      Read this lane's Autoland result
  autoland-wait --sha SHA --branch BRANCH --destination BRANCH  Wait for its landed result
  land-abandon [BRANCH | --superseded BRANCH]    Delete a superseded or handed-back branch from origin
  land-clean LANE_ROOT | --sweep                 Remove a lane's cargo target after its land checks, or trim the shared cargo targets under 10 GB
  landed-tree [--repo DIR] BASE TIP              Exit 0 when TIP's changes are already on BASE by tree or superseded by later commits to the same lines
  stale-revert [--repo DIR] [--lane TIP] BASE HEAD  Refuse a landing whose diff from BASE removes lines that recent BASE commits outside the lane added
  bin-references --repo DIR --base REV --head REV [--nixos-config DIR] [--south DIR]  Refuse a landing that deletes a bin/ entry a deployed nixos-config or south unit, or this host's user units, still run
  land-tests [--jobs N] TEST...                  Run land-check tests (shell, Python or built test binaries) side by side, reporting every failure
  land-actions --workflow FILE --base B --head H --timeout S -- COMMAND...
                                                 Run land checks as a GitHub Actions workflow on a scratch branch and wait for its verdict
  safe-push [--dry-run] --tag NAME               Scan and publish one annotated tag (branches still land through bin/safe-push)
  safe-push-result STATUS SHA REASON MINUTES URL  Print safe-push.s terminal result
  contract-probe --validate FIXTURE | --runtime PATH  Check the claude stream contract
  provenance [check|debt|audit]                  Audit grounding and target debt
  issue-box-lint [REPO#N...] [--repo REPO]       Check issue acceptance boxes
  agent-config-check [--local] [--policy-only] [--verbose]  Check the shared agent harness, Codex wiring, routines, skill budgets, rules register and vocabulary
  agent-config-check policy-gate (budget|measure|session) [--repo FIRN] [--operator DIR]  Run only the skill budget or the startup-token regression gate agents sync uses
  agent-policy-contract --repo FIRN [--north DIR] [--local] [--write] [--codex-adapters]  Check policy surface ownership and generate or verify provider hook wiring
  mutant record ID | verify BASE HEAD           Record a scratch-copy mutant, or run each Test-kind kill claim once under one lease
  archives REPO NUMBER                           Print the archive tags whose commits reference REPO#NUMBER, with restore commands
  threads ARGV                                  Run a NorthThreads command on the ledger host
  cloud [list|events|sweep|send|wait|collect]    Read, message, wait on, collect and sweep Anthropic cloud sessions
  dispatch [--dry-run]                          Launch ready smashcraft and wisp boxes on every free worker slot
  tune [--dry-run]                              Daily learning tick: write the ledger-decided routing, ETA and budget cells and file cost rises
  incident-mine [--days N] [--chunks N] [--per-thread N] [--out DIR]  Mine transcript incident candidates through convo into classifier chunks
  defaults-measure DB KIND | --priors DB | DB --propose --train-until DATE --json  Sweep a measured default walk-forward over the ledger, count prior settling data, or propose read-only defaults
  worker-ledger [--session ID] [--since DATE] [--backfill] | --summary [--by LIST] | --recommend [--planner] FACETS  Record finished workers as runs, summarize them per cell and tier, or recommend a tier
  advisor-report FILE... --incumbent ARM [--arm-key FIELD] [--codex-plan-usd ARM=VALUE]  Compare policy-replay arms against an incumbent with a verdict per challenger
  fleet [list|launch|send|ask|show|stop|forget]  Run headless agent sessions; no argument opens the screen
  secrets-proxy-exec SOCKET -- CMD [ARGS...]   Run CMD with a Secret-Service-only view of the user D-Bus session at SOCKET
  private-desktop [start|control|capture|type]   Run or control an isolated private desktop
  ledger [runs|summary] [--json]                Fleet turn records, cost and task verdicts
  orders [list|add TEXT...]                     Tom's standing orders, rendered into every fleet launch and revive
  task [landed REF SHA | check REF -- CMD...]   Record a verdict north observed itself
  gate [open --session S --question Q --option O... --recommend N --default N --default-after D [--issue OWNER/REPO#N] | answer ID N [--note TEXT] | list [--json]]  Open or answer a decision gate
  follows-release [--repo-dir DIR]             Start queued boxes after predecessors land
  capacity admission < FACTS.json               Decide one machine-capacity admission
  capacity laptop-auto [--dry-run]              Sample laptop gaming, stop agent game load while Tom plays, reap orphaned scopes
  capacity seat-idle-hint [--seconds N]         Set logind IdleHint from Wayland input idle, which ignores idle inhibitors
  capacity game-reserve on [--auto]|off|status Reserve CPUs for Tom's game, moving it only once it has run 60 s
  capacity offload choose | run --host H --base B --head S --timeout N -- CMD  Run a held land check on another host with room, shipped by git bundle
  capacity watchdog step < SAMPLE.json | replay SAMPLES.jsonl  On sustained host overload name the top process trees by fleet session, cap the offending session scope and tell its owner and coordinators
  nexus-ram record                              Append one Nexus memory, CPU steal and pressure row (minutely timer)
  nexus-ram report [--days N]                   Judge Nexus RAM headroom: UPGRADE or STAY, steal minutes and longest row gap
  laptop open|close|drain|toggle|status|tick|observe  Set or read the laptop's placement availability on the ledger host
  laptop-runner                                 Take and run queued laptop and pool jobs while the laptop is available (timer)
  laptop-ntfy serve|notify|press open|close     Phone signals for the laptop state: button endpoint, daily window reminder
  agent-contain sweep [--dry-run]               Move agent children out of Tom's slices into agent-capacity.slice and freeze stray containers
  job-watch run [--registry DIR] [--peers FILE] [--state FILE] [--dry-run]  Alert once when a registered scheduled job is overdue, and when it recovers
  model-watch [--alerts-only] [--topic T] [--sources FILE]  Snapshot public model, release and market sources, rank new candidates once on the plan, send the digest; --alerts-only sends location warnings
  model-watch news [--days N] [--noise ID|--more ID]  Show the last delivered watch digest or record feedback on an item
  outside-watch run [--dry-run] [--days N]       Minimize and block promotional spam on Tom's GitHub repos, report real outside people to the proxy inbox, mark our own notifications read
  outside-watch status                          Show the last run and its log line
  reclaim [--dry-run] [--idle-hours H]          Remove /tmp scratch and Wisp LAN pair copies idle 24 h, cargo targets of north main and of every repository's worktrees and lane-private cargo-target-* caches idle 4 h, all held by no process or pool, and trim the shared cargo target to 10 GB (hourly timer)
  retain [--dry-run] [--host H] [--archive DIR]  Archive gex45 Smashcraft evidence and capture batches past their retention age to the laptop, sha256 verified, then delete the gex45 copy; a PINNED file or an open process keeps an entry
  worktree-layout-check                         List worktrees outside ~/code/<project>/main, worktrees/<slug> and pins/<full-object-id>
  hardcoded-repo-path-check [--write-allow]     Count hard-coded repository paths against config/hardcoded-repo-paths.tsv
  lane-gc [--remote] [--dry-run] [--repo NAME]... | --unlanded  Retire landed, superseded and stale lanes and origin branches behind verified archives; report unlanded work (hourly and daily timers)
  pin-retire [--dry-run] (--consumer-main PATH... | --unconsumed) -- PIN  Retire one verified-unused content-addressed pin and its sidecar
  wt-reap [--all] [--dry-run]                   Remove spent lanes: clean, two hours old, merged into main or patch-equivalent to it
  wt-rescue [path] [--dry-run]                  Move a dirty main checkout's changes into a new rescue lane and clean main
  usage-web-read                                Read the Claude reset counts from the automation browser into the usage cache (6 h timer)
  runtime update                                Promote origin/main as this host's north runtime with its own tree
  supply-chain-scan                             Scan origin/main lockfiles with osv-scanner and alert on new advisories (daily timer)
  codex-runtime-gcroots                         Root the Nix store dependencies of every installed Codex runtime
  skill-prune [--measure]                       Weekly skill prune session, or print the shared skill body lines
  cross-project-scan [--print-bundle]           Daily cross-project overlap scan posted to Plato
  skill-review-queue                            Open, update or close the Skill review queue issue
  git-maintenance-nightly                       Run git maintenance for each ~/code/*/main under a capacity lease
  agent-runtime-update [VERSION]                Update the Codex, North, Claude and Agy runtimes
  nexus-run [OPTIONS] -- COMMAND...             Run a bounded CPU job on Nexus and retrieve its outputs
  runtime deploy HOST...                        Build origin/main here, copy it to each HOST over ssh and promote it there
  runtime health                                Check that the running binary is the current runtime with its programs and tree
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
        Command::Hook(arguments) => {
            guard::worker_handoff::install_confirm(crate::ops::pacing::confirm_spawn);
            match hook::run(&arguments) {
                Ok(()) => ExitCode::SUCCESS,
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::from(2)
                }
            }
        }
        Command::Threads(arguments) => ExitCode::from(crate::threads::client::run(&arguments, std::sync::Arc::new(fleet_view::Registry))),
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
        Command::Health(arguments) => in_runtime(async move { Ok(health::run(&arguments, tools::defines)) }),
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
