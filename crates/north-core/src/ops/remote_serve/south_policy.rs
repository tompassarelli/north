use std::fmt;

const ALLOWED_PREFIXES: [&str; 3] = ["agents/decisions/", "agents/postmortems/", "agents/references/"];
const REGULAR_FILE_MODE: &str = "100644";
const ABSENT_MODE: &str = "000000";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    pub old_mode: String,
    pub new_mode: String,
    pub status: char,
    pub path: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    Malformed(String),
    Status { path: String, status: char },
    Mode { path: String, mode: String },
    Path(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(reason) => write!(formatter, "unreadable diff: {reason}"),
            Self::Status { path, status } => write!(formatter, "{path}: status {status} refused, only add and modify land"),
            Self::Mode { path, mode } => write!(formatter, "{path}: mode {mode} refused, only 100644 lands"),
            Self::Path(path) => write!(formatter, "{path}: outside the allowed agents/decisions, agents/postmortems and agents/references markdown files"),
        }
    }
}

pub fn parse_raw_diff(output: &[u8]) -> Result<Vec<Change>, Refusal> {
    let mut fields = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    if fields.last().is_some_and(|last| last.is_empty()) {
        fields.pop();
    }
    let mut fields = fields.into_iter();
    let mut changes = Vec::new();
    while let Some(header) = fields.next() {
        let header = text(header)?;
        let header = header.strip_prefix(':').ok_or_else(|| Refusal::Malformed(format!("header without leading colon: {header}")))?;
        let parts = header.split(' ').collect::<Vec<_>>();
        let [old_mode, new_mode, _, _, status] = parts.as_slice() else {
            return Err(Refusal::Malformed(format!("header with {} fields: {header}", parts.len())));
        };
        let status_letter = status.chars().next().ok_or_else(|| Refusal::Malformed(format!("header without status: {header}")))?;
        let path = text(fields.next().ok_or_else(|| Refusal::Malformed(format!("header without path: {header}")))?)?;
        if matches!(status_letter, 'R' | 'C') {
            let destination = text(fields.next().ok_or_else(|| Refusal::Malformed(format!("copy or rename without destination: {path}")))?)?;
            changes.push(Change { old_mode: (*old_mode).to_owned(), new_mode: (*new_mode).to_owned(), status: status_letter, path: format!("{path} -> {destination}") });
            continue;
        }
        changes.push(Change { old_mode: (*old_mode).to_owned(), new_mode: (*new_mode).to_owned(), status: status_letter, path });
    }
    Ok(changes)
}

pub fn check(changes: &[Change]) -> Result<(), Refusal> {
    changes.iter().try_for_each(check_change)
}

pub fn check_raw_diff(output: &[u8]) -> Result<(), Refusal> {
    check(&parse_raw_diff(output)?)
}

fn check_change(change: &Change) -> Result<(), Refusal> {
    let (old_required, new_required) = match change.status {
        'A' => (ABSENT_MODE, REGULAR_FILE_MODE),
        'M' => (REGULAR_FILE_MODE, REGULAR_FILE_MODE),
        status => return Err(Refusal::Status { path: change.path.clone(), status }),
    };
    if change.old_mode != old_required {
        return Err(Refusal::Mode { path: change.path.clone(), mode: change.old_mode.clone() });
    }
    if change.new_mode != new_required {
        return Err(Refusal::Mode { path: change.path.clone(), mode: change.new_mode.clone() });
    }
    if allowed_path(&change.path) { Ok(()) } else { Err(Refusal::Path(change.path.clone())) }
}

fn allowed_path(path: &str) -> bool {
    let Some(relative) = ALLOWED_PREFIXES.iter().find_map(|prefix| path.strip_prefix(prefix)) else {
        return false;
    };
    let Some(stem) = relative.strip_suffix(".md") else {
        return false;
    };
    !stem.is_empty()
        && relative.split('/').all(|component| !matches!(component, "" | "." | "..") && component != ".md")
        && !path.contains(|character: char| character.is_control() || character == '\\')
}

fn text(bytes: &[u8]) -> Result<String, Refusal> {
    String::from_utf8(bytes.to_vec()).map_err(|_| Refusal::Malformed("path is not UTF-8".to_owned()))
}
