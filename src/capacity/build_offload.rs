//! Whether a heavy build this host would defer runs on another host instead, and which one.

pub(crate) const REASON: &str = "OFFLOAD";
pub(crate) const REMOTE_MARK: &str = "NORTH_BUILD_OFFLOADED";
pub(crate) const PROBE_LIMIT_SECONDS: u64 = 5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Job<'a> {
    pub(crate) owner: &'a str,
    pub(crate) timeout_seconds: u64,
    pub(crate) command: &'a [String],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Room {
    Free,
    Short(String),
    Unreachable,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Candidate<'a> {
    pub(crate) class: &'a str,
    pub(crate) landing: bool,
    pub(crate) need: Option<&'a str>,
    pub(crate) deferred: bool,
    pub(crate) offloaded_already: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Route {
    Local,
    Offload { host: String },
    Declined(Declined),
}

/// Why a deferred heavy build stays queued here although an offload host is configured.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Declined {
    NoRoom(Vec<String>),
    Unreachable(Vec<String>),
    Refused { class: String, detail: String },
    Failed(String),
}

impl Declined {
    pub(crate) fn reason(&self) -> String {
        match self {
            Self::NoRoom(_) => "no room".to_owned(),
            Self::Unreachable(_) => "unreachable".to_owned(),
            Self::Refused { class, .. } => format!("refused: {class}"),
            Self::Failed(_) => "failed".to_owned(),
        }
    }

    pub(crate) fn detail(&self) -> String {
        match self {
            Self::NoRoom(hosts) | Self::Unreachable(hosts) => hosts.join("; "),
            Self::Refused { detail, .. } => detail.clone(),
            Self::Failed(why) => why.clone(),
        }
    }

    pub(crate) fn refused(class: &str, detail: String) -> Self {
        Self::Refused { class: class.to_owned(), detail }
    }

    /// True when this decline names a different reason than the one last printed, which it then becomes.
    pub(crate) fn changes(&self, last: &mut Option<String>) -> bool {
        let reason = self.reason();
        let fresh = last.as_deref() != Some(reason.as_str());
        *last = Some(reason);
        fresh
    }
}

/// The class that keeps a deferred heavy build on this host: a local need (GPU, native, Warcraft client, private asset) names its host by `serves`, and the landing check has its own offload.
pub(crate) fn refusal(candidate: &Candidate) -> Option<Declined> {
    if candidate.landing {
        return Some(Declined::refused("landing", String::new()));
    }
    if candidate.offloaded_already {
        return Some(Declined::refused("already offloaded", String::new()));
    }
    candidate.need.filter(|need| !need.is_empty()).map(|need| Declined::refused(need, String::new()))
}

pub(crate) fn wants_offload(candidate: &Candidate) -> bool {
    candidate.class == "heavy" && candidate.deferred
}

pub(crate) fn first_free(hosts: impl IntoIterator<Item = (String, Room)>) -> Result<String, Declined> {
    let (mut short, mut dark) = (Vec::new(), Vec::new());
    for (name, room) in hosts {
        match room {
            Room::Free => return Ok(name),
            Room::Short(why) => short.push(format!("{name}: {why}")),
            Room::Unreachable => dark.push(format!("{name}: unreachable or unreadable over ssh")),
        }
    }
    if short.is_empty() {
        return Err(Declined::Unreachable(dark));
    }
    short.extend(dark);
    Err(Declined::NoRoom(short))
}

/// The probe runs only for a deferred heavy request this host may move, so a local admit, a pinned box or a host with no offload host costs no ssh.
pub(crate) fn route(candidate: &Candidate, rooms: impl FnOnce() -> Vec<(String, Room)>) -> Route {
    if !wants_offload(candidate) {
        return Route::Local;
    }
    if let Some(refused) = refusal(candidate) {
        return Route::Declined(refused);
    }
    let rooms = rooms();
    if rooms.is_empty() {
        return Route::Local;
    }
    first_free(rooms).map_or_else(Route::Declined, |host| Route::Offload { host })
}

/// Files whose extension marks a private or game asset, which never leave the laptop in a lane's uncommitted diff.
pub(crate) const PRIVATE_EXTENSIONS: [&str; 12] = ["w3x", "w3m", "w3n", "mpq", "blp", "mdx", "mdl", "casc", "dds", "tga", "wav", "mp3"];
pub(crate) const MAX_DIFF_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn ships(changed: &[&str], diff_bytes: usize) -> Result<(), Declined> {
    if diff_bytes > MAX_DIFF_BYTES {
        return Err(Declined::refused("oversized diff", format!("the uncommitted diff is {diff_bytes} bytes, over the {MAX_DIFF_BYTES} byte limit for shipping a lane")));
    }
    let private = changed.iter().find(|path| path.rsplit_once('.').is_some_and(|(_, extension)| PRIVATE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())));
    private.map_or(Ok(()), |path| Err(Declined::refused("private asset", (*path).to_owned())))
}

/// Runs a candidate's job on another host and returns its exit code, calling the notifier with the host once the command starts there; Err leaves it to the local path, with the reason when an offload was declined.
pub(crate) type Offloader = fn(&Candidate, &Job, &dyn Fn(&str)) -> Result<u8, Option<Declined>>;
