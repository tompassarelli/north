use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::watch;
use tokio::signal::unix::{SignalKind, signal};

use super::affected::{self, Scope};
use super::selection_record::{self, Recorded};
use super::shared_build;
use super::verdict_key;
use super::select::{Config, changed, path_class};
use arguments::{Arguments, Environment, Outcome, parse};
use build_host::{resolve_runner_host, runner_host};
use checkout::Trees;
use entry::{any, portable, replay, start_stage};
use selection::{Selections, defined_on_tree, selections, without_empty_test_selections, without_tests};

mod arguments;
mod base_tests;
mod batch;
mod build_host;
mod checkout;
mod entry;
mod execute;
mod fail_fast;
mod judge;
mod log;
mod offload_job;
mod queue_checks;
mod selection;
mod spawn;
mod steps;
mod timeout_row;
mod verdict;
pub use queue_checks::{Accepted, Judged, QueueRefusal, joins_queue_run, judge_queue_checks, judge_with_acceptance, queue_commands, refuse_queue_checks};
pub use steps::taken as queue_steps;
pub use steps::{Window, windows as queue_windows};
use crate::platform::north_binary::land_north as program;
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category, Span};
use north_base::foundation::exit::{CANCELLED_STATUS, KILLED_STATUS, NO_VERDICT_STATUS, RED_MAIN_STATUS, TIMED_OUT_STATUS};

pub const BROKEN_BASE: &str = "main's check environment is broken: ";
pub const BASE_FAILS: &str = "and also fails on base tree";
const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE] [--destination BRANCH] [--event-log FILE --wait-event LINE]\n       north land checks --portable --base BASE --head HEAD\n       north land checks --any --head HEAD\n       north land checks --after --base BASE --head HEAD [--tree TREE] [--outcome FILE] [--fresh]\n       north land checks --replay [--records FILE] [--since EPOCH] [--until EPOCH]\n       north land checks --accept COMMAND (--joins | [--runner-host HOST]) --base BASE --head HEAD";
const OFFLOAD_GRACE: u64 = 900;
const BATCH_GRACE: u64 = 30;
const REAP: Duration = Duration::from_secs(5);
const LOG_KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
const CANCEL_GRACE: Duration = Duration::from_secs(15);
const ACTIONS_CLASS: &str = "actions";
const OFF_HOST_PATIENCE: Duration = Duration::from_secs(10 * 60);
const OFF_HOST_RETRY: Duration = Duration::from_secs(20);


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
    placed: Arc<HashMap<String, String>>,
    pinned: usize,
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


async fn forward_signals(cancel: watch::Sender<bool>) {
    let (Ok(mut terminate), Ok(mut interrupt), Ok(mut hangup)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup())) else { return };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
        _ = hangup.recv() => {}
    }
    let _ = cancel.send(true);
    tokio::time::sleep(CANCEL_GRACE).await;
    std::process::exit(CANCELLED_STATUS);
}

async fn orchestrate(arguments: Arguments) -> i32 {
    let Some(root) = toplevel() else {
        warn("safe-push: not in a git repo");
        return 1;
    };
    let mut placed = Environment::load();
    let off_host = placed.off_host;
    let mut loaded = Config::load(&root, &arguments.head);
    if placed.local && !arguments.after && !off_host {
        loaded = loaded.without_placement();
        placed.runner = None;
    }
    let local = !arguments.after && !off_host && loaded.local_checks();
    let config = if arguments.after {
        loaded.after_set()
    } else if local {
        loaded.without_placement()
    } else {
        loaded.clone()
    };
    let (cancel_sender, cancel) = watch::channel(false);
    tokio::spawn(forward_signals(cancel_sender));
    let environment = if local { Environment::local() } else { placed.clone() };
    let commands = if arguments.leased {
        arguments.commands.clone()
    } else if !arguments.commands.is_empty() {
        arguments.commands.iter().filter(|command| config.runs(command)).cloned().collect()
    } else {
        match config.commands(&root, &arguments.base, &arguments.head) {
            Ok(commands) => defined_on_tree(&root, &arguments, commands),
            Err(message) => {
                warn(&format!("safe-push: {message}; NOT pushed."));
                return 1;
            }
        }
    };
    let commands = if arguments.leased { commands } else { without_empty_test_selections(&root, &arguments, commands) };
    if !arguments.leased && !commands.is_empty() && commands.iter().all(|command| config.is_inferred(command)) {
        say(&format!("safe-push: no land.check lines in .safe-push; running the {} land check(s) inferred from the tree", commands.len()));
    }
    let (commands, built) = if arguments.leased { (commands, Vec::new()) } else { lander_only(&root, &arguments, local, commands).await };
    let lander = run_set(root.clone(), arguments.clone(), config, environment, cancel.clone(), commands);
    if built.is_empty() {
        return lander.await;
    }
    say(&format!("safe-push: the live runtime does not cover the checker code of {} land check(s), so a build host builds north from the lane's tree and runs them: {}", built.len(), built.join("; ")));
    let (lander, built) = tokio::join!(lander, run_set(root, arguments, loaded, placed, cancel, built));
    if lander != 0 { lander } else { built }
}

/// `command` as the run that holds it runs it, rewritten as `lander_only` rewrites the run's own commands.
async fn rewritten(root: &Path, arguments: &Arguments, command: Option<&str>) -> Vec<String> {
    let Some(command) = command else { return Vec::new() };
    let (here, built) = lander_only(root, arguments, arguments.runner_host.is_empty(), vec![command.to_owned()]).await;
    [here, built].concat()
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

fn no_build_host() -> i32 {
    warn("safe-push: no build host had room for 10 minutes and these checks never run on this host, so they reach no verdict");
    NO_VERDICT_STATUS
}

struct Chosen {
    commands: Vec<String>,
    scope: Option<Scope>,
    it: Option<String>,
    ahead: Option<String>,
}

async fn selected(root: &Path, arguments: &Arguments, (environment, config): (&Environment, &Config), commands: Vec<String>) -> Result<Chosen, i32> {
    let recorded = arguments.after.then(|| selection_record::load(&arguments.head)).flatten();
    let lists = !arguments.leased && verdict_key::reads_it(&commands) && recorded.as_ref().is_none_or(|recorded| recorded.it.is_none());
    let ahead = if lists { Some(runner_host(&program(), environment, config.runner.as_deref()).await.ok_or_else(no_build_host)?) } else { None };
    let listing = ahead.as_deref().or(environment.runner.as_deref()).unwrap_or_default();
    let Selections { scope, it, no_test } = selections(root, arguments, &commands, recorded.as_ref(), listing);
    let commands = match &no_test {
        Some(reason) => without_tests(commands, reason),
        None => commands,
    };
    if !arguments.after && !arguments.leased {
        selection_record::save(&arguments.head, Recorded { scope: scope.clone(), it: it.clone() });
    }
    Ok(Chosen { commands, scope, it, ahead })
}

async fn run_set(root: PathBuf, arguments: Arguments, config: Config, environment: Environment, cancel: watch::Receiver<bool>, commands: Vec<String>) -> i32 {
    let Chosen { commands, scope, it, ahead } = match selected(&root, &arguments, (&environment, &config), commands).await {
        Ok(chosen) => chosen,
        Err(status) => return status,
    };
    let trees = Trees::of(&root, &arguments.base, arguments.tree());
    let paths = path_class(&root, &arguments.base, &arguments.head);
    let selected_paths = arguments.pinned().then(|| changed(&root, &arguments.base, &arguments.head, &[]).ok().map(|files| files.join("\n"))).flatten();
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
        placed: Arc::default(),
        pinned: 0,
    };
    if shared.arguments.leased {
        return Arc::new(shared).run_list(&commands).await.status();
    }
    let commands = if shared.arguments.fresh { commands } else { shared.reuse(commands).await };
    if commands.is_empty() {
        return 0;
    }
    let host = match ahead {
        Some(host) => host,
        None => match runner_host(&program(), &shared.environment, shared.config.runner.as_deref()).await {
            Some(host) => host,
            None => return no_build_host(),
        },
    };
    shared.arguments.runner_host = host;
    shared.free_slots = shared.count_free_slots().await;
    Arc::new(shared).execute_sharing_build(commands).await
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
    if let Some(command) = &parsed.accept {
        return accept(&parsed, command);
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

/// A finish call's acceptance `command` run as one more check before queueing `head` (north#1036), as the outside broker asks it for a sandboxed session: with `--joins`, exit 0 when the command joins that run; else run it, on `--runner-host` alone when given, and exit with `Judged::status`.
fn accept(arguments: &Arguments, command: &str) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    if arguments.base.is_empty() {
        eprintln!("{USAGE}");
        return 64;
    }
    if arguments.joins {
        return u8::from(!joins_queue_run(&root, &arguments.base, &arguments.head, command));
    }
    let on = (!arguments.runner_host.is_empty()).then_some(arguments.runner_host.as_str());
    let judged = judge_with_acceptance(&root, &arguments.base, &arguments.head, command, on);
    let status = judged.status();
    if let Err(refusal) = judged.run {
        warn(&format!("safe-push: {}", refusal.into_text()));
    }
    status
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

fn block_on<T>(work: impl Future<Output = T>) -> std::io::Result<T> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return Ok(tokio::task::block_in_place(|| handle.block_on(work)));
    }
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(work))
}

pub fn fails_first(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let queued = queue_commands(root, base, head)?;
    let commands: Vec<String> = config.added_since(&Config::load(root, base)).commands(root, base, head)?.into_iter().filter(|command| !queued.contains(&shared_build::built(command))).collect();
    if commands.is_empty() {
        return Ok(());
    }
    say(&format!("safe-push: running the {} land check(s) this lane adds before queueing it", commands.len()));
    let status = block_on(run_added(root.to_owned(), Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() }, config, commands)).map_err(|error| error.to_string())?;
    match status {
        0 => Ok(()),
        NO_VERDICT_STATUS => {
            say("safe-push: a land check this lane adds reached no verdict (exit 75: no build host answered), which is not the lane's fault; queueing the lane");
            Ok(())
        }
        _ => Err(format!("a land check this lane adds exited {status}")),
    }
}

pub(super) struct Added {
    pub(super) status: i32,
    pub(super) host: String,
    pub(super) unanswered: bool,
    pub(super) accepted: Option<bool>,
}

async fn run_added(root: PathBuf, arguments: Arguments, config: Config, commands: Vec<String>) -> i32 {
    run_added_avoiding(root, arguments, config, commands, None, &[]).await.status
}

async fn run_added_avoiding(root: PathBuf, mut arguments: Arguments, config: Config, commands: Vec<String>, watched: Option<&str>, failed: &[String]) -> Added {
    let mut environment = Environment::load();
    if environment.capacity.is_empty() {
        environment.capacity = config.capacity.clone().unwrap_or_default();
    }
    let wanted = environment.runner.clone().unwrap_or_else(|| config.runner.clone().unwrap_or_default());
    if !wanted.is_empty() && arguments.runner_host.is_empty() {
        match steps::awaited("placement", build_host::placed(&program(), &wanted, &north_base::platform::host::name(), failed)).await {
            build_host::Placed::Host(host) => arguments.runner_host = host,
            build_host::Placed::Here => warn("safe-push: no build host has room, so the checks before queueing run on this host under the land-check capacity lease"),
            build_host::Placed::Unanswered => {
                warn(&format!("safe-push: no build host answered for {} minutes, and the checks before queueing never run uncompiled on a busy laptop", OFF_HOST_PATIENCE.as_secs() / 60));
                return Added { status: NO_VERDICT_STATUS, host: String::new(), unanswered: true, accepted: None };
            }
        }
    }
    let host = arguments.runner_host.clone();
    let commands = steps::awaited("coverage", async { let (here, built) = lander_only(&root, &arguments, arguments.runner_host.is_empty(), commands).await; [here, built].concat() }).await;
    let watched = rewritten(&root, &arguments, watched).await;
    let (_cancel_sender, cancel) = watch::channel(false);
    let Selections { scope, it, .. } = steps::timed("selection", || selections(&root, &arguments, &commands, None, &arguments.runner_host));
    let (trees, paths) = steps::timed("trees", || (Trees::of(&root, &arguments.base, &arguments.head), path_class(&root, &arguments.base, &arguments.head)));
    let outcome = Outcome(arguments.outcome.clone());
    let shared = Arc::new(Shared { root, arguments, environment, config, outcome, cancel, workflow: String::new(), trees, paths, fail_fast: false, free_slots: None, actions_down: false, scope, it , selected_paths: None, placed: Arc::default(), pinned: 0 });
    let pending = steps::awaited("key lookup", shared.cached(&commands)).await;
    let status = if pending.is_empty() { 0 } else { shared.run_list(&pending).await.status() };
    let accepted = shared.passed(&watched, status).await;
    Added { status, host, unanswered: false, accepted }
}
