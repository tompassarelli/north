use std::collections::BTreeSet;

const STEP: &str = "land step failed: ";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Failure {
    Cargo(String),
    Harness(String),
    Line(String),
    Step(String),
}

impl Failure {
    pub fn key(&self) -> String {
        match self {
            Failure::Cargo(name) => format!("cargo {name}"),
            Failure::Harness(path) => format!("harness {path}"),
            Failure::Line(text) => format!("FAIL {text}"),
            Failure::Step(command) => format!("step {command}"),
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        let key = key.trim();
        key.strip_prefix("cargo ").map(|name| Failure::Cargo(name.to_owned()))
            .or_else(|| key.strip_prefix("harness ").map(|path| Failure::Harness(path.to_owned())))
            .or_else(|| key.strip_prefix("FAIL ").map(|text| Failure::Line(text.to_owned())))
            .or_else(|| key.strip_prefix("step ").map(|command| Failure::Step(command.to_owned())))
    }

    pub fn label(&self) -> &str {
        match self {
            Failure::Cargo(name) | Failure::Harness(name) | Failure::Line(name) | Failure::Step(name) => name,
        }
    }

    pub fn line(&self) -> String {
        match self {
            Failure::Cargo(name) => format!("test {name} ... FAILED"),
            Failure::Harness(path) => format!("land check: {path} exited 1"),
            Failure::Line(text) => format!("FAIL {text}"),
            Failure::Step(command) => format!("{STEP}{command}"),
        }
    }
}

pub fn log_line(line: &str) -> &str {
    let line = line.rsplit('\t').next().unwrap_or(line);
    match line.split_once(' ') {
        Some((stamp, rest)) if stamp.len() >= 20 && stamp.as_bytes()[4] == b'-' && stamp.ends_with('Z') => rest,
        _ => line,
    }
}

pub fn log_text(line: &str) -> &str {
    log_line(line).trim()
}

pub fn failures(log: &str) -> BTreeSet<Failure> {
    let mut found = BTreeSet::new();
    for line in log.lines().map(log_text) {
        if let Some(name) = line.strip_prefix("---- ").and_then(|rest| rest.strip_suffix(" stdout ----")) {
            found.insert(Failure::Cargo(name.trim().to_owned()));
        } else if let Some(name) = line.strip_prefix("test ").and_then(|rest| rest.strip_suffix(" ... FAILED")) {
            found.insert(Failure::Cargo(name.trim().to_owned()));
        } else if let Some(rest) = line.strip_prefix("land check: ") {
            let (held, rest) = rest.strip_prefix("held: ").map_or((false, rest), |rest| (true, rest));
            let Some((path, verdict)) = rest.split_once(' ') else { continue };
            if (path.ends_with(".sh") || path.ends_with(".py")) && (held || verdict.starts_with("exited") || verdict.starts_with("was killed")) {
                found.insert(Failure::Harness(path.to_owned()));
            }
        } else if let Some(command) = line.strip_prefix(STEP) {
            found.insert(Failure::Step(command.trim().to_owned()));
        } else if let Some(name) = line.strip_prefix("FAIL [").and_then(|rest| rest.split_once(']')).filter(|(took, _)| took.trim().ends_with('s')).and_then(|(_, rest)| rest.split_whitespace().last()) {
            found.insert(Failure::Cargo(name.to_owned()));
        } else if let Some(text) = line.strip_prefix("FAIL ").or_else(|| line.strip_prefix("FAIL: ")) {
            found.insert(Failure::Line(text.trim().to_owned()));
        }
    }
    found
}

/// The first line of `log` where git could not lock a ref, as two fetches racing on one checkout leave (north#902).
pub fn ref_lock(log: &str) -> Option<String> {
    log.lines().find(|line| line.contains("cannot lock ref")).map(|line| line.trim().chars().take(200).collect())
}
