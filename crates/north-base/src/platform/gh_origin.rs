use std::fs;
use std::path::Path;

const DEPTH: usize = 24;
const VALUE_FLAGS: [&str; 16] = ["-H", "--header", "-X", "--method", "-f", "--raw-field", "-F", "--field", "-q", "--jq", "-t", "--template", "-R", "--repo", "--input", "--hostname"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hook,
    Tick,
    Command,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Hook => "hook",
            Self::Tick => "tick",
            Self::Command => "command",
        }
    }

    pub fn parse(name: &str) -> Self {
        match name {
            "hook" => Self::Hook,
            "tick" => Self::Tick,
            _ => Self::Command,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Origin {
    pub kind: Kind,
    pub caller: String,
}

pub fn of_parent() -> Origin {
    classify(&ancestors(std::process::id()))
}

pub fn endpoint(arguments: &[String]) -> String {
    let mut words = positional(arguments).into_iter();
    let first = words.next().map_or("", String::as_str);
    let rest = if first == "api" { words.next().map(|path| path.split('?').next().unwrap_or(path).trim_start_matches('/').to_owned()) } else { words.next().cloned() };
    let name = rest.map_or_else(|| first.to_owned(), |rest| format!("{first}:{rest}"));
    name.chars().filter(|character| !character.is_whitespace()).collect()
}

fn positional(arguments: &[String]) -> Vec<&String> {
    let mut words = Vec::new();
    let mut skip = false;
    for word in arguments {
        if std::mem::take(&mut skip) {
            continue;
        }
        if word.starts_with('-') {
            skip = VALUE_FLAGS.contains(&word.as_str());
        } else {
            words.push(word);
        }
    }
    words
}

pub fn classify(chain: &[Vec<String>]) -> Origin {
    let mut nearest = None;
    for argv in chain {
        let Some(program) = argv.first().and_then(|program| program.rsplit('/').next()) else {
            continue;
        };
        if program != "north" {
            continue;
        }
        match argv.get(1).map(String::as_str) {
            Some("hook") => return Origin { kind: Kind::Hook, caller: "north-hook".to_owned() },
            Some("fleet") if argv.get(2).is_some_and(|verb| verb == "tick") => return Origin { kind: Kind::Tick, caller: "north-fleet-tick".to_owned() },
            Some(verb) if nearest.is_none() => nearest = Some(format!("north-{verb}")),
            _ => {}
        }
    }
    Origin { kind: Kind::Command, caller: nearest.unwrap_or_else(|| "shell".to_owned()) }
}

fn ancestors(start: u32) -> Vec<Vec<String>> {
    let mut chain = Vec::new();
    let mut pid = parent_of(start);
    while let Some(current) = pid.filter(|pid| *pid > 1) {
        if chain.len() >= DEPTH {
            break;
        }
        let process = Path::new("/proc").join(current.to_string());
        let argv = fs::read(process.join("cmdline")).unwrap_or_default().split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
        chain.push(argv);
        pid = parent_of(current);
    }
    chain
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(") ")?.1.split_whitespace().nth(1)?.parse().ok()
}

