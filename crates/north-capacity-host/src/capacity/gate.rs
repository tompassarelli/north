//! `north capacity gate`: the door the cargo, bun and nix shims and depbox go through, which holds a build for a capacity lease on a host whose `[hosts.NAME] lease_builds` is set, and sends a fleet session's compile to a build host on any host.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use super::effects::Host;
use super::build_offload;
use super::build_host_command;
use super::depbox_mode::Mode;
use own_run::OwnRun;
use crate::nix_command::{self, Inner};
use crate::shell;
use north_base::foundation::exit::NO_VERDICT;
use north_base::platform::attribution::{self, Cause, Harness};

mod own_run;

const PREFIXES: &[&str] = &["exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time"];
const LEASE_SCOPES: [&str; 2] = ["/agent-capacity-", "/private-desktop-"];
const EXECUTED: &str = "NORTH_CAPACITY_GATE_EXECUTED";
const RUST_PROGRAMS: [&str; 3] = ["cargo", "rustc", "cargo-nextest"];
const SANDBOX: &str = "DEPBOX";
const SHIM_MARKER: &str = "# north capacity gate shim: the real program is the next one on PATH that is not a shim";
const SHIMS_OUTPUT: &str = "-capacity-gate-shims/bin/";
pub const LEASED: &str = "DEPBOX_LEASED";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Moderate,
    Heavy,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Self::Moderate => "moderate",
            Self::Heavy => "heavy",
        }
    }
}

pub fn run(arguments: &[String], host: &Host) -> u8 {
    match arguments {
        [flag, program, rest @ ..] if flag == "--class" => {
            println!("{}", lease_class(&words(program, rest), host).map_or("", Class::name));
            0
        }
        [program, rest @ ..] if !program.starts_with('-') => {
            let name = shell::basename(program).to_owned();
            if let Some(mode) = sandboxed(&name, rest) {
                return enter_depbox(mode, program, rest);
            }
            let process = process();
            let executed = executed(&process, std::env::var(EXECUTED).ok().as_deref());
            let Some(real) = real_program(program, &name, rest, &executed) else {
                if !(host.lease_builds)() && agent_compile(&words(program, rest)) && !in_lease() && let Some(class) = class_of(&words(program, rest), 0) {
                    return recorded(class, &name, &name, rest, host);
                }
                let command: Vec<String> = std::iter::once(name.clone()).chain(rest.iter().cloned()).collect();
                if let Some(words) = super::dev_shell::project(&north_base::platform::north_binary::north_binary().to_string_lossy(), &command) {
                    let error = std::process::Command::new(&words[0]).args(&words[1..]).exec();
                    eprintln!("north capacity gate: {}: {error}", words[0]);
                    return 126;
                }
                eprintln!("{name}: command not found behind the north capacity gate: this host has no {name} on PATH outside the shims, so a check that needs it waits for a host that has it (infrastructure, exit 75)");
                attribution::record_here("capacity", Cause::Harness(Harness::Capacity), &format!("{name} is not on this host (exit 75)"));
                return NO_VERDICT;
            };
            let Some(class) = lease_class(&words(program, rest), host) else {
                return unleased(&real, &name, rest, (&process, &executed), OwnRun::of(&name, rest));
            };
            recorded(class, &real.to_string_lossy(), &name, rest, host)
        }
        _ => {
            eprintln!("usage: north capacity gate [--class] PROGRAM [ARG...]");
            2
        }
    }
}

/// Runs `program` in a lease of `class`, recording a clippy pass the land checks can reuse.
fn recorded(class: Class, program: &str, name: &str, rest: &[String], host: &Host) -> u8 {
    let own = OwnRun::of(name, rest);
    let code = leased(class, program, rest, host);
    own.map_or(code, |own| own.settle(code))
}

/// Becomes the real program outside a new lease, or waits for it when its exit is a verdict to record.
fn unleased(real: &Path, name: &str, rest: &[String], (process, executed): (&str, &[String]), own: Option<OwnRun>) -> u8 {
    let sized = std::env::var("CARGO_BUILD_JOBS").ok().filter(|_| in_lease()).and_then(|jobs| nix_command::sized(&words(name, rest), &jobs));
    let rest = sized.as_deref().map_or(rest, |words| &words[1..]);
    let mut command = std::process::Command::new(real);
    command.arg0(name).args(rest).env(EXECUTED, executed_value(process, executed, real, rest));
    let error = match own {
        Some(own) => match own_run::wait(&mut command) {
            Ok(code) => return own.settle(code),
            Err(error) => error,
        },
        None => command.exec(),
    };
    eprintln!("north capacity gate: {}: {error}", real.display());
    126
}

/// Holds `program` for a capacity lease of `class`, which sends a fleet session's compile to a build host.
fn leased(class: Class, program: &str, rest: &[String], host: &Host) -> u8 {
    if std::env::var_os(SANDBOX).is_some_and(|sandbox| !sandbox.is_empty()) && !(host.lease_builds)() && agent_compile(&words(program, rest)) {
        eprintln!("north capacity gate: a fleet session's {} inside depbox reaches no build host, since the sandbox has no network or ssh keys; make the compile depbox's own command (depbox run -- cargo ...) so the gate sends the whole run to a build host (infrastructure, exit 75)", shell::basename(program));
        attribution::record_here("capacity", Cause::Harness(Harness::Capacity), "sandbox reaches no build host (exit 75)");
        return NO_VERDICT;
    }
    eprintln!("north capacity gate: this host runs builds only inside a capacity lease; {} waits for a {} lease", shell::basename(program), class.name());
    let mut run = ["run", "--class", class.name(), "--owner", &owner(), "--timeout-seconds", "3600", "--"].map(str::to_owned).to_vec();
    run.push(program.to_owned());
    run.extend(rest.iter().cloned());
    super::host::run(&run, host)
}

fn words(program: &str, rest: &[String]) -> Vec<String> {
    std::iter::once(shell::basename(program).to_owned()).chain(rest.iter().cloned()).collect()
}

pub fn lease_class(words: &[String], host: &Host) -> Option<Class> {
    let class = class_of(words, 0)?;
    (!in_lease() && ((host.lease_builds)() || agent_compile(words))).then_some(class)
}

/// A Rust compile a fleet session starts, which leaves any host for a build host whether or not that host leases its own builds.
fn agent_compile(words: &[String]) -> bool {
    agent_compile_in(words, build_offload::is_agent_build(|name| std::env::var(name).ok()))
}

pub(super) fn agent_compile_in(words: &[String], agent: bool) -> bool {
    agent && compiles_rust(words)
}

/// Whether this process runs inside a capacity lease: in a lease scope, or in a depbox sandbox whose depbox was admitted, which a worker sandbox starts outside the lease scope once it leaves the Landlock domain.
pub(super) fn in_lease() -> bool {
    let admitted = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    (admitted(SANDBOX) && admitted(LEASED)) || inside_lease(&std::fs::read_to_string(std::env::var_os("AGENT_CAPACITY_SELF_CGROUP").unwrap_or_else(|| "/proc/self/cgroup".into())).unwrap_or_default())
}

pub fn inside_lease(cgroup: &str) -> bool {
    cgroup.lines().any(|line| LEASE_SCOPES.iter().any(|scope| line.contains(scope)))
}

pub fn owner() -> String {
    let session = std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.is_empty());
    let directory = || std::env::current_dir().ok().and_then(|cwd| cwd.file_name().map(|name| name.to_string_lossy().into_owned()));
    format!("gate:{}", session.or_else(directory).unwrap_or_else(|| "unknown".to_owned()))
}

fn process() -> String {
    let namespace = std::fs::read_link("/proc/self/ns/pid").map(|link| link.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{}@{namespace}", std::process::id())
}

pub(super) fn executed(process: &str, value: Option<&str>) -> Vec<String> {
    let mut fields = value.unwrap_or_default().split('\t');
    if fields.next() != Some(process) {
        return Vec::new();
    }
    fields.filter(|field| !field.is_empty()).map(str::to_owned).collect()
}

pub(super) fn executed_value(process: &str, executed: &[String], real: &Path, args: &[String]) -> String {
    std::iter::once(process.to_owned()).chain(executed.iter().cloned()).chain([invocation(real, args)]).collect::<Vec<_>>().join("\t")
}

pub(super) fn invocation(program: &Path, args: &[String]) -> String {
    use std::hash::{Hash, Hasher};
    let program = std::fs::canonicalize(program).unwrap_or_else(|_| program.to_owned());
    let mut hasher = std::hash::DefaultHasher::new();
    args.hash(&mut hasher);
    format!("{:016x} {}", hasher.finish(), program.display())
}

fn real_program(program: &str, name: &str, args: &[String], executed: &[String]) -> Option<PathBuf> {
    let own = std::fs::canonicalize(program).ok();
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(name)).find(|candidate| {
        executable(candidate) && std::fs::canonicalize(candidate).ok() != own && !is_shim(candidate) && !executed.contains(&invocation(candidate, args))
    })
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub fn is_shim(path: &Path) -> bool {
    use std::io::Read;
    if std::fs::canonicalize(path).is_ok_and(|real| real.to_string_lossy().contains(SHIMS_OUTPUT)) {
        return true;
    }
    let mut head = Vec::with_capacity(4096);
    let _ = std::fs::File::open(path).and_then(|file| file.take(4096).read_to_end(&mut head));
    let head = String::from_utf8_lossy(&head);
    head.starts_with("#!") && (head.lines().any(|line| line == SHIM_MARKER) || head.contains(SHIMS_OUTPUT))
}

pub fn class_of(words: &[String], depth: usize) -> Option<Class> {
    let mut class = None;
    leaves(words, depth, &mut |leaf| class = class.max(leaf.map_or(Some(Class::Heavy), leaf_class)));
    class
}

/// Whether the command, through every wrapper it names (depbox, `north capacity dev-shell run`, nix, `bash -c`), runs a Rust toolchain program that compiles; one that rewrites the lane's files (`fix`) or only prints help or a version does not, since a build host's edits never come back.
pub fn compiles_rust(words: &[String]) -> bool {
    let mut found = false;
    leaves(words, 0, &mut |leaf| found |= leaf.is_some_and(|leaf| RUST_PROGRAMS.contains(&shell::basename(&leaf[0])) && leaf_class(leaf).is_some() && !stays(leaf)));
    found
}

fn stays(leaf: &[String]) -> bool {
    let before_filters = || leaf.iter().skip(1).take_while(|word| *word != "--");
    before_filters().any(|word| matches!(word.as_str(), "--fix" | "--version" | "-V" | "--help" | "-h")) || before_filters().find(|word| !word.starts_with(['-', '+'])).is_some_and(|word| word == "fix")
}

/// Calls `visit` with each toolchain command the words run once their wrappers are unwrapped, and with None for a Nix build.
fn leaves(words: &[String], depth: usize, visit: &mut dyn FnMut(Option<&[String]>)) {
    if depth > 6 {
        return;
    }
    let mut start = shell::command(words, PREFIXES).min(words.len());
    while words.get(start).is_some_and(|word| word.parse::<f64>().is_ok()) {
        start += shell::command(&words[start + 1..], PREFIXES) + 1;
    }
    let words = &words[start.min(words.len())..];
    let inner = build_host_command::without_depbox(words);
    if inner.len() < words.len() {
        return leaves(inner, depth + 1, visit);
    }
    let Some(name) = words.first() else { return };
    match shell::basename(name) {
        "cargo" | "rustc" | "cargo-nextest" | "bun" => visit(Some(words)),
        "north" if words.get(1..4).is_some_and(|head| head == ["capacity", "dev-shell", "run"]) => {
            if let Some(marker) = words.iter().position(|word| word == "--") {
                leaves(&words[marker + 1..], depth + 1, visit);
            }
        }
        _ => match (nix_command::launched(words), nix_command::unwrap(words)) {
            (Some(commands), _) => commands.into_iter().for_each(|command| leaves(command, depth + 1, visit)),
            (None, Some(Inner::Command { words, .. })) => leaves(&words, depth + 1, visit),
            (None, Some(Inner::Script(source))) => script(source, depth + 1, visit),
            (None, Some(Inner::NixBuild)) => visit(None),
            (None, None) => {}
        },
    }
}

fn leaf_class(words: &[String]) -> Option<Class> {
    let (name, args) = words.split_first()?;
    match shell::basename(name) {
        "cargo" => cargo(args),
        "rustc" => args.iter().any(|word| word.ends_with(".rs")).then_some(Class::Heavy),
        "cargo-nextest" => Some(Class::Heavy),
        "bun" => bun(args),
        _ => None,
    }
}

fn script(source: &str, depth: usize, visit: &mut dyn FnMut(Option<&[String]>)) {
    let source = shell::strip_heredocs(source);
    let tokens = shell::words(&source).unwrap_or_else(|| {
        let spaced: String = source.chars().flat_map(|c| if ";&|()\n".contains(c) { vec![' ', ';', ' '] } else if "'\"".contains(c) { vec![' '] } else { vec![c] }).collect();
        spaced.split_whitespace().map(str::to_owned).collect()
    });
    shell::segments(&tokens).iter().for_each(|segment| leaves(segment, depth, visit));
}

fn cargo_subcommand(args: &[String]) -> Option<&str> {
    let mut words = args.iter();
    loop {
        let word = words.next()?;
        if matches!(word.as_str(), "-C" | "--manifest-path" | "--config" | "-Z" | "--color") {
            words.next();
        } else if !word.starts_with('-') && !word.starts_with('+') {
            return Some(word.as_str());
        }
    }
}

fn cargo(args: &[String]) -> Option<Class> {
    match cargo_subcommand(args)? {
        "build" | "b" | "test" | "t" | "bench" | "run" | "r" | "install" | "rustc" | "doc" | "d" | "nextest" | "llvm-cov" => Some(Class::Heavy),
        "check" | "c" | "clippy" | "fix" => Some(Class::Moderate),
        _ => None,
    }
}

/// Whether the command, through every wrapper it names, runs a build whose purpose is a file the caller then uses: a cargo `build`, `rustc` or `install`, or a `bun build` writing an output or an executable. A build host's slot keeps those files, so the build stays on the caller's host; `check`, `clippy`, `test` (even `--no-run`) and `run` leave nothing the caller needs.
pub fn builds_artifact(words: &[String]) -> bool {
    let mut found = false;
    leaves(words, 0, &mut |leaf| found |= leaf.is_some_and(|leaf| !stays(leaf) && makes_file(leaf)));
    found
}

fn makes_file(leaf: &[String]) -> bool {
    let (name, args) = leaf.split_first().map_or(("", &[][..]), |(name, args)| (shell::basename(name), args));
    match name {
        "cargo" => match cargo_subcommand(args) {
            Some("build" | "b" | "rustc" | "install") => true,
            Some("test" | "t") => args.iter().any(|word| word == "--no-run"),
            _ => false,
        },
        "rustc" => args.iter().any(|word| word.ends_with(".rs")),
        "bun" => args.iter().find(|word| !word.starts_with('-')).is_some_and(|word| word == "build") && args.iter().any(|word| word.split('=').next().is_some_and(|flag| matches!(flag, "--outdir" | "--outfile" | "--compile"))),
        _ => false,
    }
}

fn bun(args: &[String]) -> Option<Class> {
    let plain: Vec<&str> = args.iter().filter(|word| !word.starts_with('-')).map(String::as_str).collect();
    let plain = plain.strip_prefix(&["run"]).unwrap_or(&plain);
    let wisp = |word: &str| word == "wisp" || word.ends_with("/wisp.ts") || word == "wisp.ts";
    match plain {
        ["build", ..] => Some(Class::Moderate),
        ["test", targets @ ..] => (!matches!(targets, [file] if script_file(file))).then_some(Class::Moderate),
        [script, "map", "build", ..] if wisp(script) => Some(Class::Moderate),
        _ => None,
    }
}

fn script_file(path: &str) -> bool {
    !path.contains(['*', '?', '[']) && [".ts", ".js", ".tsx", ".jsx"].iter().any(|suffix| path.ends_with(suffix))
}

/// The depbox mode a toolchain program runs in when a shim starts it outside depbox: `fetch` for the verbs that download, `run` for every other one that may run a dependency's code, and None for a call that runs none (a version, help, metadata or formatting query), inside depbox already, or a bun or node script that landed code ships.
pub fn sandboxed(name: &str, args: &[String]) -> Option<Mode> {
    if std::env::var_os(SANDBOX).is_some_and(|sandbox| !sandbox.is_empty()) {
        return None;
    }
    sandbox_mode(name, args, &|script| landed(Path::new(script)))
}

pub(super) fn sandbox_mode(name: &str, args: &[String], landed: &dyn Fn(&str) -> bool) -> Option<Mode> {
    let plain: Vec<&str> = args.iter().map(String::as_str).filter(|word| !word.starts_with(['-', '+'])).collect();
    let query = plain.is_empty() && args.iter().any(|word| matches!(word.as_str(), "--version" | "-V" | "-v" | "--help" | "-h" | "--list"));
    if query {
        return None;
    }
    let fetches = match name {
        "cargo" => match cargo_subcommand(args) {
            Some("version" | "help" | "metadata" | "locate-project" | "pkgid" | "tree" | "fmt" | "clean" | "verify-project" | "read-manifest") => return None,
            subcommand => matches!(subcommand, Some("fetch" | "update" | "add" | "generate-lockfile")),
        },
        "rustc" | "cargo-nextest" => false,
        "bun" | "node" => {
            let script = plain.strip_prefix(&["run"]).unwrap_or(&plain).first().copied();
            if script.is_some_and(landed) {
                return None;
            }
            name == "bun" && matches!(plain.first(), Some(&("install" | "i" | "add" | "a" | "update" | "remove" | "rm" | "x" | "pm")))
        }
        "bunx" | "npx" => true,
        "npm" | "pnpm" | "yarn" => matches!(plain.first(), None | Some(&("install" | "i" | "ci" | "add" | "update" | "up" | "upgrade" | "uninstall" | "remove" | "rm" | "dlx" | "exec" | "x"))),
        _ => return None,
    };
    Some(if fetches { Mode::Fetch } else { Mode::Run })
}

/// Whether `script` is code that landed: inside the Nix store or a promoted north runtime, which no lane writes.
fn landed(script: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(script) else { return false };
    let runtimes = std::env::var_os("HOME").map(|home| Path::new(&home).join(".local/state/north/runtimes")).and_then(|runtimes| std::fs::canonicalize(runtimes).ok());
    real.starts_with("/nix/store") || runtimes.is_some_and(|runtimes| real.starts_with(runtimes))
}

/// Runs the shim's `program` again inside depbox, where the gate finds the real program and leases it as before.
fn enter_depbox(mode: Mode, program: &str, rest: &[String]) -> u8 {
    let north = std::env::current_exe().unwrap_or_else(|_| north_base::platform::north_binary::north_binary());
    let error = std::process::Command::new(&north).args(["depbox", mode.name(), "--", program]).args(rest).exec();
    eprintln!("north capacity gate: {}: {error}", north.display());
    126
}
