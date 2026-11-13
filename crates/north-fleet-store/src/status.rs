#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    Queued,
    Now,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Starting,
    Working,
    Idle,
    Parked,
    Exited,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Starting => "starting",
            Status::Working => "working",
            Status::Idle => "idle",
            Status::Parked => "parked",
            Status::Exited => "exited",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Report {
    Done,
    NotDone,
    Split,
    Blocked,
}

impl Report {
    const WORDS: [(&'static str, Report); 4] = [("Done", Report::Done), ("Not done", Report::NotDone), ("Split", Report::Split), ("Blocked", Report::Blocked)];

    pub fn of(reply: &str) -> Option<Self> {
        let opening = reply.trim_start().trim_start_matches(['*', '#', '`', ' ']);
        Self::WORDS.into_iter().find(|(word, _)| opening.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric()))).map(|(_, report)| report)
    }

    pub fn label(self) -> &'static str {
        match self {
            Report::Done => "done",
            Report::NotDone => "not done",
            Report::Split => "split",
            Report::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionPhase {
    Live(Status),
    Waiting,
    Reported(Report),
}

impl SessionPhase {
    pub fn of(status: Status, gated: bool, reply: Option<&str>) -> Self {
        match status {
            Status::Exited => SessionPhase::Live(status),
            _ if gated => SessionPhase::Waiting,
            Status::Idle => reply.and_then(Report::of).map_or(SessionPhase::Live(status), SessionPhase::Reported),
            _ => SessionPhase::Live(status),
        }
    }

    pub fn needs_answer(self) -> bool {
        matches!(self, SessionPhase::Waiting | SessionPhase::Reported(Report::NotDone | Report::Split | Report::Blocked))
    }

    pub fn label(self) -> &'static str {
        match self {
            SessionPhase::Live(status) => status.label(),
            SessionPhase::Waiting => "waiting",
            SessionPhase::Reported(report) => report.label(),
        }
    }
}
