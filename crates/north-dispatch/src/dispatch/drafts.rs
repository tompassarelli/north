//! Drafts: the change a qualifying open box would get, computed on the small model before the box is
//! launched and kept as one unpushed commit in a lane (north#1240). Each state of a draft is one JSON
//! line in `drafts.jsonl` beside the launch record, and a later line for a name supersedes an earlier one.
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const FILE: &str = "drafts.jsonl";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Running,
    DraftPass,
    DraftFail,
    PassesOnBase,
    Abandoned,
    Landed,
}

impl Outcome {
    pub fn word(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::DraftPass => "draft-pass",
            Self::DraftFail => "draft-fail",
            Self::PassesOnBase => "passes-on-base",
            Self::Abandoned => "abandoned",
            Self::Landed => "landed",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Draft {
    pub name: String,
    pub at: f64,
    pub item: String,
    pub box_text: String,
    pub box_sha256: String,
    pub checkout: PathBuf,
    pub base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    pub check: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_exit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt: Option<PathBuf>,
    pub host: String,
    pub lane: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ground {
    Standing,
    Moved(Vec<String>),
    Unknown(String),
}

pub fn append(directory: &Path, draft: &Draft) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(FILE)).map_err(|error| error.to_string())?;
    let line = format!("{}\n", serde_json::to_string(draft).map_err(|error| error.to_string())?);
    file.write_all(line.as_bytes()).map_err(|error| error.to_string())
}

pub fn read(directory: &Path) -> Vec<Draft> {
    let mut drafts: Vec<Draft> = Vec::new();
    for draft in std::fs::read_to_string(directory.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str::<Draft>(line).ok()) {
        match drafts.iter_mut().find(|kept| kept.name == draft.name) {
            Some(kept) => *kept = draft,
            None => drafts.push(draft),
        }
    }
    drafts
}

pub fn ground(draft: &Draft) -> Ground {
    let output = Command::new("git").arg("-C").arg(&draft.checkout).args(["diff", "--name-only", draft.base.as_str(), "origin/main", "--"]).output();
    match output {
        Ok(output) if output.status.success() => {
            let changed: Vec<&str> = std::str::from_utf8(&output.stdout).unwrap_or_default().lines().collect();
            let moved: Vec<String> = draft.files.iter().filter(|file| changed.contains(&file.as_str())).cloned().collect();
            if moved.is_empty() { Ground::Standing } else { Ground::Moved(moved) }
        }
        Ok(output) => Ground::Unknown(String::from_utf8_lossy(&output.stderr).lines().next().unwrap_or("git diff failed").to_owned()),
        Err(error) => Ground::Unknown(error.to_string()),
    }
}

pub fn line(draft: &Draft, ground: &Ground, now: f64) -> String {
    let tokens = draft.tokens.map_or_else(|| "- tokens".to_owned(), |tokens| format!("{tokens} tokens"));
    let cost = draft.cost_usd.map_or_else(|| "$-".to_owned(), |usd| format!("${usd:.4}"));
    let main = match ground {
        Ground::Standing => "main stands under its files".to_owned(),
        Ground::Moved(files) => format!("main moved under {}", files.join(", ")),
        Ground::Unknown(why) => format!("main unknown: {why}"),
    };
    let check = draft.check_exit.map_or_else(|| draft.outcome.word().to_owned(), |exit| format!("{} (exit {exit})", draft.outcome.word()));
    format!("{} {} {} {check} {tokens} {cost} {main}: {}", draft.name, draft.item, age(now - draft.at), draft.box_text.chars().take(80).collect::<String>())
}

fn age(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0) as u64;
    if minutes < 60 { format!("{minutes}m") } else { format!("{}h{:02}m", minutes / 60, minutes % 60) }
}
