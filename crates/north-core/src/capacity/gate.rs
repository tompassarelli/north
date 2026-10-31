//! `north capacity gate`: the door the cargo, bun and nix shims and depbox go through, which holds a build for a capacity lease on a host whose `[hosts.NAME] lease_builds` is set.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use super::Host;
use crate::shell;

const PREFIXES: &[&str] = &["exec", "command", "nohup", "setsid", "nice", "ionice", "stdbuf", "timeout", "time", "env"];
const LEASE_SCOPES: [&str; 2] = ["/agent-capacity-", "/private-desktop-"];

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
            let Some(real) = real_program(program, &name) else {
                eprintln!("{name}: command not found behind the north capacity gate");
                return 127;
            };
            let Some(class) = lease_class(&words(program, rest), host) else {
                let error = std::process::Command::new(&real).arg0(&name).args(rest).exec();
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
    let cgroup = std::fs::read_to_string(std::env::var_os("AGENT_CAPACITY_SELF_CGROUP").unwrap_or_else(|| "/proc/self/cgroup".into())).unwrap_or_default();
    (!inside_lease(&cgroup) && (host.lease_builds)()).then_some(class)
}

pub fn inside_lease(cgroup: &str) -> bool {
    cgroup.lines().any(|line| LEASE_SCOPES.iter().any(|scope| line.contains(scope)))
}

fn owner() -> String {
    let session = std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.is_empty());
    let directory = || std::env::current_dir().ok().and_then(|cwd| cwd.file_name().map(|name| name.to_string_lossy().into_owned()));
    format!("gate:{}", session.or_else(directory).unwrap_or_else(|| "unknown".to_owned()))
}

fn real_program(program: &str, name: &str) -> Option<PathBuf> {
    let own = std::fs::canonicalize(program).ok();
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|directory| directory.join(name))
        .find(|candidate| executable(candidate) && std::fs::canonicalize(candidate).ok() != own && !is_shim(candidate))
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn is_shim(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 256];
    let read = std::fs::File::open(path).and_then(|mut file| file.read(&mut head)).unwrap_or(0);
    head[..read].starts_with(b"#!") && head[..read].windows(13).any(|window| window == b"capacity gate")
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
        "nix" if args.first().is_some_and(|word| word == "develop") => {
            args.iter().position(|word| word == "-c" || word == "--command").and_then(|marker| class_of(&args[marker + 1..], depth + 1))
        }
        "sh" | "bash" => {
            let marker = args.iter().position(|word| word.starts_with('-') && !word.starts_with("--") && word.contains('c'))?;
            script(args.get(marker + 1)?, depth + 1)
        }
        _ => None,
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
        if matches!(word.as_str(), "-C" | "--config" | "-Z" | "--color") {
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
