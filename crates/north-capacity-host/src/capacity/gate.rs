//! `north capacity gate`: the door the cargo, bun and nix shims and depbox go through, which holds a build for a capacity lease on a host whose `[hosts.NAME] lease_builds` is set.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use super::effects::Host;
use crate::nix_command::{self, Inner};
use crate::shell;

const PREFIXES: &[&str] = &["exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time"];
const LEASE_SCOPES: [&str; 2] = ["/agent-capacity-", "/private-desktop-"];
const EXECUTED: &str = "NORTH_CAPACITY_GATE_EXECUTED";
const TOOLCHAIN_MISSING: u8 = 75;
const SHIM_MARKER: &str = "# north capacity gate shim: the real program is the next one on PATH that is not a shim";
const SHIMS_OUTPUT: &str = "-capacity-gate-shims/bin/";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Moderate,
    Heavy,
}

impl Class {
    fn name(self) -> &'static str {
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
            let process = process();
            let executed = executed(&process, std::env::var(EXECUTED).ok().as_deref());
            let Some(real) = real_program(program, &name, rest, &executed) else {
                let command: Vec<String> = std::iter::once(name.clone()).chain(rest.iter().cloned()).collect();
                if let Some(words) = super::dev_shell::project(&north_base::platform::north_binary::north_binary().to_string_lossy(), &command) {
                    let error = std::process::Command::new(&words[0]).args(&words[1..]).exec();
                    eprintln!("north capacity gate: {}: {error}", words[0]);
                    return 126;
                }
                eprintln!("{name}: command not found behind the north capacity gate: this host has no {name} on PATH outside the shims, so a check that needs it waits for a host that has it (infrastructure, exit 75)");
                return TOOLCHAIN_MISSING;
            };
            let Some(class) = lease_class(&words(program, rest), host) else {
                let sized = std::env::var("CARGO_BUILD_JOBS").ok().filter(|_| in_lease()).and_then(|jobs| nix_command::sized(&words(program, rest), &jobs));
                let rest = sized.as_deref().map_or(rest, |words| &words[1..]);
                let error = std::process::Command::new(&real).arg0(&name).args(rest).env(EXECUTED, executed_value(&process, &executed, &real, rest)).exec();
                eprintln!("north capacity gate: {}: {error}", real.display());
                return 126;
            };
            eprintln!("north capacity gate: this host runs builds only inside a capacity lease; {name} waits for a {} lease", class.name());
            let mut leased = ["run", "--class", class.name(), "--owner", &owner(), "--timeout-seconds", "3600", "--"].map(str::to_owned).to_vec();
            leased.push(real.to_string_lossy().into_owned());
            leased.extend(rest.iter().cloned());
            super::host::run(&leased, host)
        }
        _ => {
            eprintln!("usage: north capacity gate [--class] PROGRAM [ARG...]");
            2
        }
    }
}

fn words(program: &str, rest: &[String]) -> Vec<String> {
    std::iter::once(shell::basename(program).to_owned()).chain(rest.iter().cloned()).collect()
}

pub fn lease_class(words: &[String], host: &Host) -> Option<Class> {
    let class = class_of(words, 0)?;
    (!in_lease() && (host.lease_builds)()).then_some(class)
}

fn in_lease() -> bool {
    inside_lease(&std::fs::read_to_string(std::env::var_os("AGENT_CAPACITY_SELF_CGROUP").unwrap_or_else(|| "/proc/self/cgroup".into())).unwrap_or_default())
}

pub fn inside_lease(cgroup: &str) -> bool {
    cgroup.lines().any(|line| LEASE_SCOPES.iter().any(|scope| line.contains(scope)))
}

fn owner() -> String {
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
    if depth > 6 {
        return None;
    }
    let mut start = shell::command(words, PREFIXES).min(words.len());
    while words.get(start).is_some_and(|word| word.parse::<f64>().is_ok()) {
        start += shell::command(&words[start + 1..], PREFIXES) + 1;
    }
    let words = &words[start.min(words.len())..];
    let (name, args) = words.split_first()?;
    match shell::basename(name) {
        "cargo" => cargo(args),
        "bun" => bun(args),
        "depbox" => args.iter().position(|word| word == "--").and_then(|marker| class_of(&args[marker + 1..], depth + 1)),
        _ => match nix_command::unwrap(words)? {
            Inner::Command { words, .. } => class_of(&words, depth + 1),
            Inner::Script(source) => script(source, depth + 1),
            Inner::NixBuild => Some(Class::Heavy),
        },
    }
}

fn script(source: &str, depth: usize) -> Option<Class> {
    let source = shell::strip_heredocs(source);
    let tokens = shell::words(&source).unwrap_or_else(|| {
        let spaced: String = source.chars().flat_map(|c| if ";&|()\n".contains(c) { vec![' ', ';', ' '] } else if "'\"".contains(c) { vec![' '] } else { vec![c] }).collect();
        spaced.split_whitespace().map(str::to_owned).collect()
    });
    shell::segments(&tokens).iter().filter_map(|segment| class_of(segment, depth)).max()
}

fn cargo(args: &[String]) -> Option<Class> {
    let mut words = args.iter();
    let subcommand = loop {
        let word = words.next()?;
        if matches!(word.as_str(), "-C" | "--manifest-path" | "--config" | "-Z" | "--color") {
            words.next();
        } else if !word.starts_with('-') && !word.starts_with('+') {
            break word.as_str();
        }
    };
    match subcommand {
        "build" | "b" | "test" | "t" | "bench" | "run" | "r" | "install" | "rustc" | "doc" | "d" | "nextest" | "llvm-cov" => Some(Class::Heavy),
        "check" | "c" | "clippy" | "fix" => Some(Class::Moderate),
        _ => None,
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
