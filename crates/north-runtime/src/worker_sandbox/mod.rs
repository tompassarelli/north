//! `north --worker-sandbox COMMAND`: runs one shell command of an agent session under a Landlock domain. Every agent session's domain keeps the operator's own logins unreadable, since an agent calls GitHub as an App installation, and sets git's GitHub credential helper to that App. It also refuses to execute build output (a lane's `target/` and `node_modules/`) by its path, so the code a lane's dependencies bring runs only inside depbox, which the toolchain shims enter, or through `north lane exec` for a lane whose lock files equal main's. A Buck2 lane's `buck-out/` is made before the domain is built, so the Buck2 daemon, which outlives the command that starts it and keeps its domain, may run what it builds there (buck.rs). A worker's domain also confines removal: a file or directory can be removed or renamed away only beneath the worker's lane, its git state and the scratch and cache trees its builds need, and its lane's Buck2 daemon directory. A delete whose target is computed at run time (an unset variable, a split `xargs` word, a `dirname` of the wrong line, a `remove_dir_all` on a computed path) meets the kernel with the real path, so the home directory, project containers, main checkouts, other lanes, pins and transcripts survive it. A worker's domain also confines writing and creating files, directories and links to the trees it may remove, the device files, the agent state it writes (its spools, the hooks' records and the guard's logs), a fetch's `FETCH_HEAD` and its projects' worktrees directories, so a worker command cannot change the operator's dotfiles and configuration, the client profiles, the local bin directories, the north runtimes, the fleet's session records, the lifecycle ledger, a project's git hooks and configuration or a main checkout's files; a lane-only domain confines them to the trees it may remove and the device files. Reading is open apart from the logins and, for a worker, what another host trusts: the ssh directory and the files its links resolve to, the secrets directory and the agent socket, whose variable a worker command does not get; the ssh agent and control-master sockets are hidden with the other sockets, so a worker command reaches a build host only through the offload verbs the outside broker runs. When a worker command hands work to another local process, the outside broker carries the same removal roots to that process, and depbox applies them again inside the mount namespace it must build outside the domain. See docs/rationale/worker-sandbox.md.
mod broker;
mod broker_served;
mod broker_start;
mod brokered_reads;
mod buck;
mod check_request;
mod finish_request;
mod hold_request;
pub mod hold_client;
mod except;
mod executable;
mod file_tools;
mod lane_grant;
mod session_join;
mod lane_only;
mod lane_session;
mod offload_request;
mod packed_refs;
pub use packed_refs::restore as restore_packed_refs;
mod readable;
mod request_checkout;
mod request_env;
mod removable;
mod hook_screen;
mod sockets;
mod state;
mod sweep;
mod withdraw_request;
mod writable;
pub mod tool_server;

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use landlock::{AccessFs, BitFlags, CompatLevel, Compatible, RulesetAttr, RulesetCreatedAttr, RulesetStatus, path_beneath_rules};
use north_base::platform::git_credential;
use north_base::platform::session_scope::REMOVAL_ROOTS;

pub use executable::{build_outputs, executable};
pub use file_tools::unwritable_by_file_tool;
pub use readable::{Reader, readable};
pub use removable::{Context, removable};
pub use hook_screen::Screen;
pub use broker::SOCKET_ENV as BROKER_SOCKET_ENV;
pub use broker_start::words as broker_for;
pub use sockets::runtime;
use writable::writable;

pub const FLAG: &str = "--worker-sandbox";
const OUTSIDE: &str = "--outside";
const BROKER: &str = "--broker";
const CODE: &str = "--code";
const MAKER: &str = "--make-namespaces";
const BUILT: &str = "--built";
const SWEPT: &str = "NORTH_WORKER_SWEEP_PARENT";
const LANE: &str = "--lane";
const PROGRAM: &str = "--program";
const CHECKED: &str = "--checked";
const HAND_OFF: &str = "--hand-off";
const SOCKETS_HIDDEN: &str = north_base::platform::session_scope::SOCKETS_HIDDEN;
const CONFINED: &str = north_base::platform::session_scope::CONFINED;
const MARK: &str = north_base::platform::gh_session::SANDBOX_MARK;
const LANE_ONLY: &str = north_base::platform::session_scope::LANE_ONLY;
const HOSTS_HIDDEN: &str = north_base::platform::session_scope::HOSTS_HIDDEN;
const SESSION_SCOPE: &str = "NORTH_WORKER_SESSION_SCOPE";
const USAGE: &str = "usage: north --worker-sandbox COMMAND | north --worker-sandbox --built|--program -- PROGRAM [ARGS...] | north --worker-sandbox --checked -- PROGRAM [ARGS...] | north --worker-sandbox --lane -- PROGRAM [ARGS...] | north --worker-sandbox --hand-off -- PROGRAM [ARGS...] | north --worker-sandbox --outside -- NORTH ARGS... | north --worker-sandbox --broker [--code DIRECTORY [--detached | --peers CGROUP]]";

/// Restricts this process, and everything it starts, to removing and renaming away only beneath `roots`: the rule a process the outside broker handed work to keeps inside the namespace it builds, where Landlock could not be applied before the mount calls and the namespace alone does not know the worker's trees.
fn confine_removal(roots: &[PathBuf]) -> Result<RulesetStatus, String> {
    let removal = AccessFs::RemoveDir | AccessFs::RemoveFile | AccessFs::Refer;
    let ruleset = landlock::Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(removal).map_err(|error| error.to_string())?;
    let ruleset = ruleset.create().map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(["/"], AccessFs::Refer)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(roots.iter().filter(|root| root.is_dir()), removal)).map_err(|error| error.to_string())?;
    ruleset.restrict_self().map(|status| status.ruleset).map_err(|error| error.to_string())
}

fn writing() -> BitFlags<AccessFs> {
    AccessFs::WriteFile | AccessFs::Truncate | AccessFs::MakeReg | AccessFs::MakeDir | AccessFs::MakeSym | AccessFs::MakeFifo | AccessFs::MakeSock | AccessFs::MakeChar | AccessFs::MakeBlock
}

fn removal() -> BitFlags<AccessFs> {
    AccessFs::RemoveDir | AccessFs::RemoveFile | AccessFs::Refer
}

/// Restricts this process, and everything it starts, to reading only files beneath `readable`, given `executable` to executing only files beneath it, given `roots` to removing and renaming away only beneath them, and given `writable` to opening files for writing and creating entries only beneath it, apart from the `replaced` directories, whose regular files it may write, make and replace and where it may make nothing else. Refer is handled either way, so a file cannot be linked or moved to where it would become readable, writable or executable.
pub fn confine(roots: Option<&[PathBuf]>, readable: &[PathBuf], executable: Option<&[PathBuf]>, writable: Option<&[PathBuf]>, replaced: &[PathBuf]) -> Result<RulesetStatus, String> {
    let handled = if roots.is_some() { removal() } else { BitFlags::from(AccessFs::Refer) };
    let handled = if executable.is_some() { handled | AccessFs::Execute } else { handled };
    let handled = if writable.is_some() { handled | writing() } else { handled };
    let ruleset = landlock::Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(handled | AccessFs::ReadFile).map_err(|error| error.to_string())?;
    let ruleset = ruleset.create().map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(["/"], AccessFs::Refer)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(roots.unwrap_or_default().iter().filter(|root| root.is_dir()), removal())).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(readable, AccessFs::ReadFile)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(executable.unwrap_or_default(), AccessFs::Execute)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.add_rules(path_beneath_rules(writable.unwrap_or_default().iter().filter(|root| root.exists()), writing())).map_err(|error| error.to_string())?;
    let replacing = (AccessFs::WriteFile | AccessFs::Truncate | AccessFs::MakeReg | AccessFs::RemoveFile) & handled;
    let replaced = if replacing.is_empty() { &[][..] } else { replaced };
    let ruleset = ruleset.add_rules(path_beneath_rules(replaced, replacing)).map_err(|error| error.to_string())?;
    let ruleset = ruleset.set_compatibility(CompatLevel::BestEffort).log_new_exec(true).map_err(|error| error.to_string())?;
    ruleset.restrict_self().map(|status| status.ruleset).map_err(|error| error.to_string())
}

pub fn run(arguments: &[String], screen: Screen) -> std::process::ExitCode {
    let bound = north_base::platform::session_binding::unapplied();
    if !bound.is_empty() {
        return bind(arguments, bound);
    }
    match arguments {
        [broker, served @ ..] if broker == BROKER => broker_start::run(served),
        [maker, runtime, paths @ ..] if maker == MAKER => sockets::make(Path::new(runtime), &paths.iter().map(PathBuf::from).collect::<Vec<_>>()),
        [outside, separator, north, words @ ..] if outside == OUTSIDE && separator == "--" && !north.is_empty() => self::outside(north, words),
        [built, separator, program @ ..] if built == BUILT && separator == "--" && !program.is_empty() => self::program(program, true, screen),
        [lane, separator, program @ ..] if lane == LANE && separator == "--" && !program.is_empty() => self::in_lane(program, screen),
        [hand, separator, program @ ..] if hand == HAND_OFF && separator == "--" && !program.is_empty() => self::hand_off(program),
        [flag, separator, program @ ..] if flag == PROGRAM && separator == "--" && !program.is_empty() => self::program(program, false, screen),
        [flag, separator, program @ ..] if flag == CHECKED && separator == "--" && !program.is_empty() => in_domain(program, false, true),
        [command] => shell(command, screen),
        _ => {
            eprintln!("{USAGE}");
            std::process::ExitCode::from(2)
        }
    }
}

/// Runs this command again with the name a taken spare was bound to, so its domain, its hooks and every north verb below it see the session it now is, never the spare it started as.
fn bind(arguments: &[String], bound: Vec<(String, String)>) -> std::process::ExitCode {
    let error = Command::new(std::env::current_exe().unwrap_or_else(|_| PathBuf::from("north"))).arg(FLAG).args(arguments).envs(bound).exec();
    eprintln!("north worker-sandbox: {error}");
    std::process::ExitCode::from(126)
}

/// Runs `north WORDS` outside the worker's Landlock domain, through the broker that a fixed set of verbs may ask (broker.rs), for a tool that must build its own mount namespace (Landlock refuses mount(2) inside a domain, so bubblewrap cannot start there); run with neither the sandbox mark nor hidden sockets, it just execs `north WORDS`. The tool's own namespace then confines what it may write.
fn outside(north: &str, words: &[String]) -> std::process::ExitCode {
    if std::env::var_os(MARK).is_none() && std::env::var_os(SOCKETS_HIDDEN).is_none() {
        let error = Command::new(north).args(words).exec();
        eprintln!("north worker-sandbox: {north}: {error}");
        return std::process::ExitCode::from(126);
    }
    std::process::ExitCode::from(through_broker(words))
}

fn through_broker(words: &[String]) -> u8 {
    broker::request(words).unwrap_or_else(|error| {
        eprintln!("{error}");
        126
    })
}

/// Runs `program` under only the removal rule of the worker command the outside broker handed it from, read as the JSON roots [`REMOVAL_ROOTS`] carries. depbox builds the mount namespace that a Landlock domain refuses to a confined process, so the broker cannot hand its work a confined process; it hands it these roots instead, and this applies them to the command once the namespace exists.
fn hand_off(program: &[String]) -> std::process::ExitCode {
    let roots: Vec<PathBuf> = std::env::var(REMOVAL_ROOTS).ok().and_then(|roots| serde_json::from_str(&roots).ok()).unwrap_or_default();
    for root in &roots {
        if !root.is_dir() {
            let _ = std::fs::create_dir_all(root);
        }
    }
    report(confine_removal(&roots));
    let error = Command::new(&program[0]).args(&program[1..]).exec();
    eprintln!("north worker-sandbox: {}: {error}", program[0]);
    std::process::ExitCode::from(126)
}

/// A fleet worker, and a lead that holds a task (session_scope::confined_role, north#1137), is confined beyond the logins and build output once the `worker-sandbox` gate of harness/gates.toml enforces, or for a canary launch made with NORTH_WORKER_SANDBOX=1 while it is in shadow (north#706): its removal is confined and the sockets that start processes outside the domain are hidden (south#86). The proxy, a lead with no task and sessions started by hand keep both open, since they drive windows and units.
fn confines_worker() -> bool {
    let worker = north_base::platform::session_scope::confined_session();
    let applies = north_base::platform::harness::document("harness/gates.toml")["gate"]["worker-sandbox"]["mode"].as_str() == Some("enforce") || std::env::var_os("NORTH_WORKER_SANDBOX").is_some_and(|canary| canary == "1");
    worker && applies
}

fn shell(command: &str, screen: Screen) -> std::process::ExitCode {
    if let Some(refused) = hook_screen::refusal(command, screen) {
        return refused;
    }
    let context = Context::from_env(command);
    let confined = confines_worker();
    if confined && std::env::var_os(SWEPT).is_none() {
        return sweep::around(command, &context, SWEPT);
    }
    broker::ensure();
    let hidden = confined && hide_sockets(&context);
    let roots = confined.then(|| removable(&context));
    let writable = roots.as_deref().map(|roots| writable(&context, roots));
    let replaced = if writable.is_some() { packed_refs::directories(&context) } else { Vec::new() };
    report(confine(roots.as_deref(), &readable(&context, Reader::of(confined)), Some(&executable(&context)), writable.as_deref(), &replaced));
    let shell = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()).unwrap_or_else(|| OsString::from("bash"));
    let error = without_agent(Command::new(shell), confined).arg("-c").arg(command).envs(git_env(writable.is_some())).env_remove(SWEPT).env(MARK, "1").envs(hidden.then_some((SOCKETS_HIDDEN, "1"))).envs(confined.then_some((CONFINED, "1"))).exec();
    eprintln!("north worker-sandbox: {error}");
    std::process::ExitCode::from(126)
}

/// Runs `program` under the command's domain, with the rule against executing build output unless `lift` says a lane's own build output that `north lane exec` vouched for.
fn program(program: &[String], lift: bool, screen: Screen) -> std::process::ExitCode {
    if let Some(refused) = hook_screen::program_refusal(program, screen) {
        return refused;
    }
    in_domain(program, lift, confines_worker())
}

/// Runs `program` under the domain of a lane-only command when this one is, else of a worker when `worker` says so, else of an agent session.
fn in_domain(program: &[String], lift: bool, worker: bool) -> std::process::ExitCode {
    let context = Context::from_env(&program.join(" "));
    let fenced = std::env::var_os(LANE_ONLY).is_some();
    let confined = fenced || worker;
    let hidden = confined && hide_sockets(&context);
    let roots = confined.then(|| removable(&context));
    let executable = (!lift).then(|| executable(&context));
    let readable = if fenced { lane_only::readable_in_lane(&context, &[]) } else { readable(&context, Reader::of(confined)) };
    let writable = roots.as_deref().map(|roots| if fenced { lane_only::writable_in_lane(roots) } else { writable(&context, roots) });
    let replaced = if writable.is_some() { packed_refs::directories(&context) } else { Vec::new() };
    report(confine(roots.as_deref(), &readable, executable.as_deref(), writable.as_deref(), &replaced));
    let error = without_agent(Command::new(&program[0]), confined).args(&program[1..]).envs(git_env(writable.is_some())).env(MARK, "1").envs(hidden.then_some((SOCKETS_HIDDEN, "1"))).envs(confined.then_some((CONFINED, "1"))).exec();
    eprintln!("north worker-sandbox: {}: {error}", program[0]);
    std::process::ExitCode::from(126)
}

/// Runs `program` confined to its lane for an agent that is not Claude (OpenCode): removal confined as for a worker, and reading limited to the lane, its project's main checkout, the toolchains and the system, so no other checkout, no private repository, no sops key and no agent state is readable. Its own scratch directory, named by NORTH_SANDBOX_SCRATCH, stays readable. The session's shell tool asks the outside broker for a fresh domain of this kind for each command, since a lane the session creates after its long-lived domain was built holds no rule there; a fresh domain, known by the lane-only marker already in its environment, keeps what the long-lived one kept plus the lanes created since (lane_session.rs). The broker must answer before the long-lived domain hides the sockets.
fn in_lane(program: &[String], screen: Screen) -> std::process::ExitCode {
    if let Some(refused) = hook_screen::program_refusal(program, screen) {
        return refused;
    }
    session_join::join_named_scope();
    broker::ensure();
    let mut context = Context::from_env(&program.join(" "));
    let kept = if std::env::var_os(LANE_ONLY).is_some() {
        match lane_session::admit(&mut context) {
            Ok(kept) => kept,
            Err(message) => {
                eprintln!("{message}");
                return std::process::ExitCode::from(126);
            }
        }
    } else {
        lane_session::begin(&context.cwd);
        Vec::new()
    };
    let hidden = hide_sockets(&context);
    report(confine_to_lane(&context, &kept));
    let error = Command::new(&program[0]).args(&program[1..]).envs(git_env(true)).env(MARK, "1").env(LANE_ONLY, "1").env(CONFINED, "1").env_remove(SESSION_SCOPE).env_remove(sockets::AGENT_ENV).env(HOSTS_HIDDEN, "1").envs(hidden.then_some((SOCKETS_HIDDEN, "1"))).exec();
    eprintln!("north worker-sandbox: {}: {error}", program[0]);
    std::process::ExitCode::from(126)
}

/// Restricts this process, and everything it starts, to the lane-only domain of `context`: removal and writing only in its lane, scratch and caches, no build output run by its path, and reading only what `lane_only::readable_in_lane` keeps, with the `fresh` lanes a fresh domain adds.
pub fn confine_to_lane(context: &Context, fresh: &[PathBuf]) -> Result<RulesetStatus, String> {
    let roots = removable(context);
    confine(Some(&roots), &lane_only::readable_in_lane(context, fresh), Some(&executable(context)), Some(&lane_only::writable_in_lane(&roots)), &packed_refs::directories(context))
}

/// Hides the sockets that start processes outside the domain, before the domain is applied: a kernel before Landlock ABI 9 cannot limit which Unix sockets a command connects through, and Landlock refuses mount(2), so a session that already runs inside a domain (one launched from a confined command) joins the namespaces the broker laid, and one that does not lays them itself. Whether they are hidden.
fn hide_sockets(context: &Context) -> bool {
    if std::env::var_os(SOCKETS_HIDDEN).is_some_and(|hidden| hidden == "1") {
        return true;
    }
    let runtime = sockets::runtime(context);
    let direct = || sockets::hide(&runtime, &sockets::hidden(&runtime, &sockets::multiplexer(context.uid, std::env::var_os("TMUX_TMPDIR")), &sockets::ssh_reach(&context.home, context.uid, context.agent_socket.as_deref())));
    match broker::join_namespaces().or_else(|brokered| direct().map_err(|error| format!("{brokered}; {error}"))) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("north worker-sandbox: cannot hide the sockets that start processes outside the domain ({error}), so they stay reachable");
            false
        }
    }
}

/// The git environment of a command: the session's GitHub credential helper, and once its writes are confined, no upstream for a branch it creates, since recording one rewrites the repository's `config`, which the domain keeps unwritable (`north lane new` already creates its branch without one).
fn git_env(writes_confined: bool) -> Vec<(String, String)> {
    if writes_confined { git_credential::session_env_with(&[("branch.autoSetupMerge", "false")]) } else { git_credential::session_env() }
}

/// `command` without the agent socket variable, and marked as reaching no other host, when it runs `confined`: its socket is hidden, and a worker reaches no other host through it (south#118).
fn without_agent(mut command: Command, confined: bool) -> Command {
    if confined {
        command.env_remove(sockets::AGENT_ENV).env(HOSTS_HIDDEN, "1");
    }
    command
}

fn report(confined: Result<RulesetStatus, String>) {
    let why = match confined {
        Ok(RulesetStatus::FullyEnforced) => return,
        Ok(status) => format!("Landlock {status:?} on this kernel"),
        Err(error) => format!("Landlock unavailable ({error})"),
    };
    if north_base::platform::orchestration::read().is_ok_and(|policy| declares_no_landlock(&policy, &north_base::platform::host::name())) {
        return;
    }
    eprintln!("north worker-sandbox: {why}, so removal, personal logins and build output cannot be confined and the command was not run; a host without Landlock says `landlock = false` under its `[hosts.NAME]` in the orchestration policy");
    std::process::exit(126);
}

fn declares_no_landlock(policy: &toml_edit::DocumentMut, host: &str) -> bool {
    policy.get("hosts").and_then(|hosts| hosts.get(host)?.get("landlock")?.as_bool()) == Some(false)
}

/// Runs `north ARGUMENTS` outside the worker's Landlock domain when called inside it, for a verb that enforces its own removal rule (`north lane unlock`, `north lane stray`) or for `safe-push`, whose checks before queueing run tests that build their own mount namespace, which Landlock refuses to any process in a domain (north#1094); `None` outside a worker sandbox, where the verb runs in place.
pub fn reenter_outside(arguments: &[String]) -> Option<u8> {
    std::env::var_os(MARK)?;
    Some(through_broker(arguments))
}

/// Has the outside broker run `north ARGUMENTS` with the host's GitHub credentials when called in a lane-only session's domain, which holds none: landing its own lane and writing its own task issue (lane_grant.rs). The broker decides what the caller may from its cgroup; the marker only routes the request. `None` elsewhere, where the verb runs in place.
pub fn reenter_lane_only(arguments: &[String]) -> Option<u8> {
    std::env::var_os(MARK)?;
    std::env::var_os(LANE_ONLY)?;
    Some(through_broker(arguments))
}

/// The refusal a lane-only session's shell gets instead of running safe-push itself: the broker lands a lane-only
/// session's lane only for a caller in that session's own scope, and every OpenCode and Pi shell command runs in the
/// broker's fresh `--lane` child, so the worker commits, ends with `Done:` and the review-and-land loop lands it
/// (north#1041). It names the lane, branch and commit the reviewer reads.
pub fn lane_only_safe_push_refusal() -> Option<String> {
    north_base::platform::session_scope::lane_only_outside_scope().then(|| {
        let fact = |arguments: &[&str]| {
            Command::new("git")
                .args(arguments)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .filter(|line| !line.is_empty())
        };
        let worktree = fact(&["rev-parse", "--show-toplevel"]).unwrap_or_else(|| ".".to_owned());
        let branch = fact(&["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_else(|| "HEAD".to_owned());
        let sha = fact(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
        format!("safe-push: lane-only session: end with Done: {worktree} {branch} {sha}; the review-and-land loop lands it")
    })
}

/// Why a git write in `checkout` would strand its lock files when called inside a worker sandbox: the domain lets git create them anywhere but remove them only beneath the trees `removable` names, so a failed or finished write leaves `index.lock` for the next writer (north#1172). `None` outside a sandbox or for a checkout whose locks the session may remove.
pub fn stranded_git_locks(checkout: &Path) -> Option<String> {
    std::env::var_os(MARK)?;
    let context = Context::from_env("");
    (!removable::holds_git_locks(&context, checkout)).then(|| format!("{} is not this session's lane, and inside the worker sandbox git could create its lock files there but not remove them; hand that lane in with `safe-push --to main --wait` run from it", checkout.display()))
}

/// Asks the outside broker for the Done-when check of one box of one issue, in the words `task check --done-when --box TEXT OWNER/REPO#N`, which it runs outside this process's domain and whose verdict it alone writes (check_request.rs); the check's exit code.
pub fn ask_check(arguments: &[String]) -> u8 {
    broker::request_check(arguments).unwrap_or_else(|error| {
        eprintln!("{error}");
        126
    })
}

/// Has the outside broker run the acceptance command `command` of a finish call in `lane` in a new worker domain, with `log` as its output, and record its verdict for the caller's own session, when called inside a worker sandbox's domain, which keeps the verdict store unwritable and lets a worker name none (finish_request.rs); the command's exit code. `None` outside a sandbox, where the caller runs the command and writes the record itself.
pub fn finish_check(lane: &Path, command: &str, head: &str, log: &std::fs::File) -> Option<Result<u8, String>> {
    std::env::var_os(MARK)?;
    Some(broker::request_finish_check(lane, &finish_request::request_words(command, head), log))
}

/// The command that runs the program `program` in a new worker domain whatever role starts it, judged by no hook: what the checker the broker starts for a session runs a box's Done-when command as, since the command's text comes from an issue a session may have written.
pub fn checked_shell(program: &[String]) -> Command {
    let mut shell = Command::new(std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary()));
    shell.args([FLAG, CHECKED, "--"]).args(program);
    shell
}

/// Whether this process runs inside a worker sandbox's Landlock domain.
pub fn confined() -> bool {
    std::env::var_os(MARK).is_some()
}

/// Runs `program` without the domain's rule against executing build output, keeping its other rules, when called inside a worker sandbox; `None` outside one, where nothing refuses it.
pub fn run_built(program: &[String]) -> Option<u8> {
    std::env::var_os(MARK)?;
    let words: Vec<String> = [FLAG.to_owned(), BUILT.to_owned(), "--".to_owned()].into_iter().chain(program.iter().cloned()).collect();
    Some(through_broker(&words))
}

#[cfg(test)]
mod tests;
