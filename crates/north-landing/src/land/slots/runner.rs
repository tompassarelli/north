use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::watch;
use tokio::signal::unix::{SignalKind, signal};

use super::affected::{self, CHANGED_ENV, PACKAGES_ENV, Scope};
use super::it_modules::{self, IT_ENV};
use super::clippy_first;
use super::deps;
use super::shared_build;
use super::select::{Config, changed, path_class};
use crate::landing::check_timings::outcomes::{Record, Records};
use arguments::{Arguments, Environment, Outcome, parse};
use checkout::Trees;

mod arguments;
mod batch;
mod build_host;
mod checkout;
mod execute;
mod fail_fast;
mod judge;
mod log;
mod queue_checks;
mod spawn;
mod timeout_row;
mod verdict;
pub use queue_checks::refuse_queue_checks;
use crate::land::after::follow::program;
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category, Span};
use north_base::foundation::exit::NO_VERDICT_STATUS;

const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE] [--destination BRANCH] [--event-log FILE --wait-event LINE]\n       north land checks --portable --base BASE --head HEAD\n       north land checks --any --head HEAD\n       north land checks --after --base BASE --head HEAD [--tree TREE] [--outcome FILE]\n       north land checks --replay [--records FILE] [--since EPOCH] [--until EPOCH]";
const OFFLOAD_GRACE: u64 = 900;
const BATCH_GRACE: u64 = 30;
const REAP: Duration = Duration::from_secs(5);
const LOG_KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
const TIMED_OUT: i32 = 124;
const KILLED: i32 = 137;
const RED_MAIN: i32 = 3;
const CANCELLED: i32 = 130;
const CANCEL_GRACE: Duration = Duration::from_secs(15);
const ACTIONS_CLASS: &str = "actions";


#[derive(Clone)]
struct Shared {
    root: PathBuf,
    arguments: Arguments,
    environment: Environment,
    config: Config,
    outcome: Outcome,
    cancel: watch::Receiver<bool>,
    workflow: String,
    trees: Trees,
    free_slots: Option<u32>,
    actions_down: bool,
    paths: String,
    fail_fast: bool,
    scope: Option<Scope>,
    it: Option<String>,
    selected_paths: Option<String>,
}

fn host_name(class: &str) -> String {
    if class != "local" {
        return class.to_owned();
    }
    fs::read_to_string("/proc/sys/kernel/hostname").map_or_else(|_| class.to_owned(), |name| name.trim().to_owned())
}

fn say(text: &str) {
    println!("{text}");
}

fn warn(text: &str) {
    eprintln!("{text}");
}

fn toplevel() -> Option<PathBuf> {
    let output = std::process::Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok()?;
    output.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end()))
}


async fn resolve_runner_host(value: &str) -> String {
    build_host::chosen(&program(), value, None).await.unwrap_or_else(|| {
        warn("safe-push: no build host has room; the land checks run on this host under the land-check capacity lease");
        String::new()
    })
}

async fn runner_host(environment: &Environment, config: &Config) -> String {
    let land_runner = environment.runner.clone().unwrap_or_else(|| config.runner.clone().unwrap_or_default());
    if land_runner.is_empty() { String::new() } else { resolve_runner_host(&land_runner).await }
}

async fn forward_signals(cancel: watch::Sender<bool>) {
    let (Ok(mut terminate), Ok(mut interrupt), Ok(mut hangup)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup())) else { return };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
        _ = hangup.recv() => {}
    }
    let _ = cancel.send(true);
    tokio::time::sleep(CANCEL_GRACE).await;
    std::process::exit(CANCELLED);
}

fn scope_for(root: &Path, arguments: &Arguments, commands: &[String]) -> Option<Scope> {
    if !commands.iter().any(|command| command.contains(PACKAGES_ENV) || command.contains(CHANGED_ENV)) {
        return None;
    }
    let scope = affected::scope(root, &arguments.base, &arguments.head).unwrap_or_else(|error| {
        warn(&format!("safe-push: {error}; the Rust checks cover the whole workspace"));
        Scope::Workspace
    });
    if !arguments.leased {
        say(&format!("safe-push: the Rust checks cover {}", scope.arguments()));
    }
    Some(scope)
}

fn with_unchecked(mut commands: Vec<String>) -> Vec<String> {
    let unchecked = std::env::var(super::super::checks::UNCHECKED_ENV).unwrap_or_default();
    let added: Vec<String> = unchecked.lines().filter(|command| !command.is_empty() && !commands.iter().any(|listed| listed == command)).map(str::to_owned).collect();
    if !added.is_empty() {
        say(&format!("safe-push: running {} check(s) whose run before queueing reached no verdict: {}", added.len(), added.join("; ")));
        commands.extend(added);
    }
    commands
}

fn defined_on_tree(root: &Path, arguments: &Arguments, commands: Vec<String>) -> Vec<String> {
    if !arguments.pinned() {
        return commands;
    }
    let tree = Config::load(root, arguments.tree());
    let tree = if arguments.after { tree.after_set() } else { tree };
    commands.into_iter().filter(|command| tree.runs(command)).collect()
}

fn it_for(root: &Path, arguments: &Arguments, commands: &[String]) -> Option<String> {
    if !commands.iter().any(|command| command.contains(IT_ENV)) {
        return None;
    }
    let selection = it_modules::selection(root, &arguments.base, &arguments.head).unwrap_or_else(|error| {
        warn(&format!("safe-push: {error}; the tests/it run covers every module"));
        it_modules::Selection::Every
    });
    if !arguments.leased {
        say(&format!("safe-push: the tests/it run is {}", selection.arguments()));
    }
    Some(selection.arguments())
}

fn portable(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    let config = Config::load(&root, &arguments.head);
    let Ok(commands) = config.commands(&root, &arguments.base, &arguments.head) else { return 1 };
    let portable = config.workflow.is_empty() && !commands.is_empty() && commands.iter().all(|command| config.local_reason(command).is_none());
    u8::from(!portable)
}

fn record_wait(arguments: &Arguments) {
    let Some(log) = &arguments.event_log else { return };
    if let Some(directory) = log.parent() {
        let _ = fs::create_dir_all(directory);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(file, "{}", arguments.wait_event);
    }
}

fn start_stage(arguments: &Arguments) -> Option<u8> {
    let destination = arguments.destination.as_deref()?;
    if destination != "main" {
        return Some(0);
    }
    let Some(root) = toplevel() else { return Some(0) };
    if !Config::load(&root, "HEAD").has_checks() {
        return Some(0);
    }
    record_wait(arguments);
    let Err(error) = deps::sync(&root) else { return None };
    warn(&error);
    Outcome(arguments.outcome.clone()).record(&["skip", "skipped: bun install failed"]);
    Some(1)
}

fn any(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    u8::from(!Config::load(&root, &arguments.head).has_checks())
}

fn replay(arguments: &Arguments) -> u8 {
    let until = if arguments.until == 0 { u64::MAX } else { arguments.until };
    let records: Vec<Record> = Records::at(arguments.records.clone().unwrap_or_else(Records::default_path)).read().into_iter().filter(|record| (arguments.since..until).contains(&record.started)).collect();
    say(&fail_fast::replay(&records).to_string());
    0
}

async fn orchestrate(arguments: Arguments) -> i32 {
    let Some(root) = toplevel() else {
        warn("safe-push: not in a git repo");
        return 1;
    };
    let loaded = Config::load(&root, &arguments.head);
    let local = !arguments.after && loaded.local_checks();
    let config = if arguments.after {
        loaded.after_set()
    } else if local {
        loaded.without_placement()
    } else {
        loaded.clone()
    };
    let (cancel_sender, cancel) = watch::channel(false);
    tokio::spawn(forward_signals(cancel_sender));
    let environment = if local { Environment::local() } else { Environment::load() };
    let commands = if arguments.leased {
        arguments.commands.clone()
    } else if !arguments.commands.is_empty() {
        arguments.commands.iter().filter(|command| config.runs(command)).cloned().collect()
    } else {
        match config.commands(&root, &arguments.base, &arguments.head) {
            Ok(commands) => with_unchecked(defined_on_tree(&root, &arguments, commands)),
            Err(message) => {
                warn(&format!("safe-push: {message}; NOT pushed."));
                return 1;
            }
        }
    };
    if !arguments.leased && !commands.is_empty() && commands.iter().all(|command| config.is_inferred(command)) {
        say(&format!("safe-push: no land.check lines in .safe-push; running the {} land check(s) inferred from the tree", commands.len()));
    }
    let (commands, built) = if arguments.leased { (commands, Vec::new()) } else { lander_only(&root, &arguments, local, commands).await };
    let lander = run_set(root.clone(), arguments.clone(), config, environment, cancel.clone(), commands);
    if built.is_empty() {
        return lander.await;
    }
    say(&format!("safe-push: the live runtime does not cover the checker code of {} land check(s), so a build host builds north from the lane's tree and runs them: {}", built.len(), built.join("; ")));
    let (lander, built) = tokio::join!(lander, run_set(root, arguments, loaded, Environment::load(), cancel, built));
    if lander != 0 { lander } else { built }
}

async fn lander_only(root: &Path, arguments: &Arguments, local: bool, commands: Vec<String>) -> (Vec<String>, Vec<String>) {
    let (mut here, mut built) = (Vec::new(), Vec::new());
    for command in commands {
        match shared_build::lander_only_code(&command) {
            Some(code) if local && covers(root, arguments.tree(), code).await => here.push(command),
            Some(_) if local => built.push(shared_build::built(&command)),
            Some(_) => here.push(shared_build::built(&command)),
            None => here.push(command),
        }
    }
    (here, built)
}

async fn covers(root: &Path, head: &str, code: &str) -> bool {
    let mut command = Command::new(shared_build::live_runtime());
    command.args(["runtime", "covers", "--head", head, "--code", code]).current_dir(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command.status().await.is_ok_and(|status| status.success())
}

async fn run_set(root: PathBuf, arguments: Arguments, config: Config, environment: Environment, cancel: watch::Receiver<bool>, commands: Vec<String>) -> i32 {
    let scope = scope_for(&root, &arguments, &commands);
    let it = it_for(&root, &arguments, &commands);
    let trees = Trees::of(&root, &arguments.base, arguments.tree());
    let paths = path_class(&root, &arguments.base, &arguments.head);
    let selected_paths = arguments.pinned().then(|| changed(&root, &arguments.base, &arguments.head, &[]).join("\n"));
    let mut shared = Shared {
        selected_paths,
        trees,
        paths,
        fail_fast: false,
        workflow: config.workflow.clone(),
        outcome: Outcome(arguments.outcome.clone()),
        root,
        arguments,
        environment,
        config,
        cancel,
        free_slots: None,
        actions_down: false,
        scope,
        it,
    };
    if shared.arguments.leased {
        return Arc::new(shared).run_list(&commands).await.status();
    }
    let commands = shared.reuse(commands).await;
    if commands.is_empty() {
        return 0;
    }
    shared.arguments.runner_host = runner_host(&shared.environment, &shared.config).await;
    shared.free_slots = shared.count_free_slots().await;
    let shared = Arc::new(shared);
    let Some(plan) = clippy_first::plan(&commands, |command| !shared.config.is_inferred(command)).filter(|_| !shared.arguments.after) else {
        return shared.execute_sharing_build(commands).await;
    };
    say(&clippy_first::order(&plan, shared.scope.as_ref()));
    let status = Arc::clone(&shared).execute(plan.first()).await;
    if status != 0 {
        warn(&format!("safe-push: clippy on the affected crates exited {status}, so the other {} land check(s) did not run; NOT pushed.", plan.rest.len()));
        return status;
    }
    shared.execute(plan.rest).await
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(parsed) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    if parsed.portable {
        return portable(&parsed);
    }
    if parsed.any {
        return any(&parsed);
    }
    if parsed.replay {
        return replay(&parsed);
    }
    if let Some(code) = start_stage(&parsed) {
        return code;
    }
    let mut parsed = parsed;
    let root = root_span(&parsed);
    parsed.span.clone_from(&root.id);
    let code = block_on(orchestrate(parsed)).unwrap_or_else(|error| {
        eprintln!("north land checks: {error}");
        1
    });
    trace::record(&[Span { end: epoch(), ..root }.with("exit", code.to_string())]);
    u8::try_from(code).unwrap_or(1)
}

fn root_span(arguments: &Arguments) -> Span {
    let parent = std::env::var(trace::PARENT_ENV).ok().filter(|parent| !parent.is_empty()).or_else(|| (!arguments.after).then(|| trace::check_id(&arguments.head)));
    let name = if arguments.after { "post-land checks" } else { "land checks" };
    let span = Span::new(&arguments.head, name, Category::Lander, epoch(), epoch()).on(&trace::this_host());
    let span = if arguments.after { span.with(trace::PHASE, trace::AFTER) } else { span };
    match parent {
        Some(parent) => span.under(&parent),
        None => span,
    }
}

fn block_on(work: impl Future<Output = i32>) -> std::io::Result<i32> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return Ok(tokio::task::block_in_place(|| handle.block_on(work)));
    }
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(work))
}

pub fn fails_first(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let commands = config.added_since(&Config::load(root, base)).commands(root, base, head)?;
    if commands.is_empty() {
        return Ok(());
    }
    say(&format!("safe-push: running the {} land check(s) this lane adds before queueing it", commands.len()));
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() };
    let status = block_on(run_added(root.to_owned(), arguments, config, commands)).map_err(|error| error.to_string())?;
    match status {
        0 => Ok(()),
        NO_VERDICT_STATUS => {
            say("safe-push: a land check this lane adds reached no verdict (exit 75: no build host answered), which is not the lane's fault; queueing the lane");
            Ok(())
        }
        _ => Err(format!("a land check this lane adds exited {status}")),
    }
}

async fn run_added(root: PathBuf, mut arguments: Arguments, config: Config, commands: Vec<String>) -> i32 {
    let mut environment = Environment::load();
    if environment.capacity.is_empty() {
        environment.capacity = config.capacity.clone().unwrap_or_default();
    }
    let wanted = environment.runner.clone().unwrap_or_else(|| config.runner.clone().unwrap_or_default());
    if !wanted.is_empty() {
        let Some(host) = build_host::chosen(&program(), &wanted, Some(&north_base::platform::host::name())).await else {
            warn("safe-push: no build host has room, so the checks before queueing reach no verdict rather than compile on this host; the lander runs them");
            return NO_VERDICT_STATUS;
        };
        arguments.runner_host = host;
    }
    let (_cancel_sender, cancel) = watch::channel(false);
    let scope = scope_for(&root, &arguments, &commands);
    let it = it_for(&root, &arguments, &commands);
    let trees = Trees::of(&root, &arguments.base, &arguments.head);
    let paths = path_class(&root, &arguments.base, &arguments.head);
    let shared = Arc::new(Shared { root, arguments, environment, config, outcome: Outcome(None), cancel, workflow: String::new(), trees, paths, fail_fast: false, free_slots: None, actions_down: false, scope, it , selected_paths: None });
    let pending = shared.cached(&commands).await;
    if pending.is_empty() { 0 } else { shared.run_list(&pending).await.status() }
}
