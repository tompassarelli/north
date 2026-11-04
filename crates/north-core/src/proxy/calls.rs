//! What one tool call of the proxy did, by kind: `north` commands ask the verb table the CLI
//! declares, and an ordered rule list classifies the foreign commands.
use serde_json::Value;

use super::shell::{self, Stage};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    Delegate,
    ChildMessage,
    TomFacing,
    Brief,
    Monitoring,
    Bookkeeping,
    Landing,
    LandingLogs,
    Status,
    Diagnosis,
}

impl Kind {
    pub const ALL: [Self; 10] = [
        Self::Delegate,
        Self::ChildMessage,
        Self::TomFacing,
        Self::Brief,
        Self::Monitoring,
        Self::Bookkeeping,
        Self::Landing,
        Self::LandingLogs,
        Self::Status,
        Self::Diagnosis,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Delegate => "delegate",
            Self::ChildMessage => "child message",
            Self::TomFacing => "Tom-facing",
            Self::Brief => "briefing",
            Self::Monitoring => "monitoring scripts",
            Self::Bookkeeping => "bookkeeping",
            Self::Landing => "hands-on landing and editing",
            Self::LandingLogs => "landing logs",
            Self::Status => "status checks",
            Self::Diagnosis => "diagnosis",
        }
    }

    pub fn hands_work_away(self) -> bool {
        matches!(self, Self::Delegate | Self::ChildMessage | Self::Brief | Self::TomFacing)
    }
}

pub struct VerbKinds(&'static [(&'static str, Kind)]);

impl VerbKinds {
    pub const fn new(table: &'static [(&'static str, Kind)]) -> Self {
        Self(table)
    }

    pub fn kind(&self, words: &[&str]) -> Option<Kind> {
        let Some(verb) = words.first() else {
            return self.declared("");
        };
        words.get(1).and_then(|sub| self.declared(&format!("{verb} {sub}"))).or_else(|| self.declared(verb))
    }

    pub fn undeclared<'a>(&self, verbs: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
        verbs.into_iter().filter(|verb| self.declared(verb).is_none()).collect()
    }

    pub fn strays(&self, verbs: &[&str]) -> Vec<&'static str> {
        self.0.iter().map(|(path, _)| *path).filter(|path| !path.is_empty() && !verbs.contains(&path.split(' ').next().unwrap_or(path))).collect()
    }

    fn declared(&self, path: &str) -> Option<Kind> {
        self.0.iter().find(|(known, _)| *known == path).map(|(_, kind)| *kind)
    }
}

pub fn classify(tool: &str, input: &Value, verbs: &VerbKinds) -> Kind {
    let text = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or_default();
    match tool {
        "Agent" | "Task" => Kind::Delegate,
        "SendMessage" | "ListAgents" => Kind::ChildMessage,
        "Monitor" | "TaskStop" | "ScheduleWakeup" | "CronCreate" | "CronDelete" | "CronList" => Kind::Monitoring,
        "Write" if text("content").trim_start().starts_with("Item:") => Kind::Brief,
        "Write" if is_helper_script(text("file_path")) => Kind::Monitoring,
        "Edit" | "Write" | "NotebookEdit" | "MultiEdit" => Kind::Landing,
        "Read" | "Grep" | "Glob" if is_landing_log(&format!("{} {}", text("file_path"), text("path"))) => Kind::LandingLogs,
        "Bash" => command(text("command"), verbs),
        _ => Kind::Diagnosis,
    }
}

pub fn command(text: &str, verbs: &VerbKinds) -> Kind {
    let whole = whole_text(text);
    let stages = shell::stages(text);
    whole.into_iter().chain(stages.iter().map(|stage| stage_kind(stage, verbs))).min().unwrap_or(Kind::Diagnosis)
}

fn whole_text(text: &str) -> Option<Kind> {
    if text.lines().any(|line| line.trim_start().starts_with("Item:")) {
        return Some(Kind::Brief);
    }
    if text.contains("date '+%I:%M %p'") || text.contains("date +'%I:%M %p'") || text.contains("date \"+%I:%M %p\"") {
        return Some(Kind::TomFacing);
    }
    if shell::writes(text).iter().any(|target| is_helper_script(target)) {
        return Some(Kind::Monitoring);
    }
    if shell::appends(text).iter().any(|target| target.ends_with(".md")) {
        return Some(Kind::Bookkeeping);
    }
    is_landing_log(text).then_some(Kind::LandingLogs)
}

const PROGRAMS: &[(&str, Kind)] = &[
    ("safe-push", Kind::Landing),
    ("sd", Kind::Landing),
    ("sleep", Kind::Monitoring),
    ("nohup", Kind::Monitoring),
    ("setsid", Kind::Monitoring),
    ("watch", Kind::Monitoring),
    ("inotifywait", Kind::Monitoring),
    ("journalctl", Kind::Status),
    ("ps", Kind::Status),
    ("pgrep", Kind::Status),
    ("uptime", Kind::Status),
    ("free", Kind::Status),
    ("nvidia-smi", Kind::Status),
];

fn stage_kind(stage: &Stage, verbs: &VerbKinds) -> Kind {
    if stage.backgrounded || stage.loop_keyword {
        return Kind::Monitoring;
    }
    let args: Vec<&str> = stage.args.iter().map(String::as_str).collect();
    match stage.program.as_str() {
        "north" => verbs.kind(&verb_words(&args)).unwrap_or(Kind::Diagnosis),
        "threads" => threads(&args),
        "git" => git(&args),
        "gh" => gh(&args),
        "sed" | "perl" if args.iter().any(|arg| arg.starts_with("-i") || arg.starts_with("-pi")) => Kind::Landing,
        "systemctl" if args.iter().any(|arg| matches!(*arg, "status" | "list-timers" | "is-active" | "list-units")) => Kind::Status,
        program => PROGRAMS.iter().find(|(known, _)| *known == program).map_or(Kind::Diagnosis, |(_, kind)| *kind),
    }
}

fn verb_words<'a>(args: &[&'a str]) -> Vec<&'a str> {
    args.iter().copied().filter(|arg| !arg.starts_with('-')).take(2).collect()
}

fn threads(args: &[&str]) -> Kind {
    match verb_words(args).as_slice() {
        ["queue", "follow", ..] => Kind::Delegate,
        [verb, ..] if matches!(*verb, "tick" | "close" | "claim" | "release") => Kind::Bookkeeping,
        _ => Kind::Status,
    }
}

fn git(args: &[&str]) -> Kind {
    let mut rest = args;
    while let [flag, _, tail @ ..] = rest
        && matches!(*flag, "-C" | "-c")
    {
        rest = tail;
    }
    match rest {
        [verb, ..] if matches!(*verb, "commit" | "push" | "rebase" | "add" | "cherry-pick" | "am" | "merge" | "reset" | "revert") => Kind::Landing,
        ["merge-base", tail @ ..] if tail.contains(&"--is-ancestor") => Kind::Bookkeeping,
        ["worktree", sub, ..] if matches!(*sub, "remove" | "prune") => Kind::Bookkeeping,
        ["branch", tail @ ..] if tail.iter().any(|arg| matches!(*arg, "-d" | "-D" | "--delete")) => Kind::Bookkeeping,
        _ => Kind::Diagnosis,
    }
}

fn gh(args: &[&str]) -> Kind {
    match args {
        ["issue" | "pr", verb, ..] if matches!(*verb, "edit" | "close" | "comment" | "reopen") => Kind::Bookkeeping,
        ["run", ..] => Kind::Status,
        _ => Kind::Diagnosis,
    }
}

fn is_helper_script(path: &str) -> bool {
    path.ends_with(".sh") || path.ends_with(".ts")
}

fn is_landing_log(text: &str) -> bool {
    ["north/land-queue", "/safe-push/", "land-check-", "push.log"].iter().any(|marker| text.contains(marker))
        || (text.contains("journalctl") && text.contains("north-land"))
}
