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
}

/// Only code-only heavy work waits to be moved: a box with a local need (GPU, native, Warcraft client, private asset) names its host by `serves`, and the landing check has its own offload.
pub(crate) fn wants_offload(candidate: &Candidate) -> bool {
    candidate.class == "heavy" && !candidate.landing && candidate.deferred && !candidate.offloaded_already && candidate.need.is_none_or(str::is_empty)
}

pub(crate) fn first_free(hosts: impl IntoIterator<Item = (String, Room)>) -> Result<String, Vec<String>> {
    let mut refused = Vec::new();
    for (name, room) in hosts {
        match room {
            Room::Free => return Ok(name),
            Room::Short(why) => refused.push(format!("{name}: {why}")),
            Room::Unreachable => refused.push(format!("{name}: unreachable or unreadable over ssh")),
        }
    }
    Err(refused)
}

/// The probe runs only for a request that wants offload, so a local admit or a pinned box costs no ssh.
pub(crate) fn route(candidate: &Candidate, rooms: impl FnOnce() -> Vec<(String, Room)>) -> Route {
    if !wants_offload(candidate) {
        return Route::Local;
    }
    first_free(rooms()).map_or(Route::Local, |host| Route::Offload { host })
}

/// Files whose extension marks a private or game asset, which never leave the laptop in a lane's uncommitted diff.
pub(crate) const PRIVATE_EXTENSIONS: [&str; 12] = ["w3x", "w3m", "w3n", "mpq", "blp", "mdx", "mdl", "casc", "dds", "tga", "wav", "mp3"];
pub(crate) const MAX_DIFF_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn ships(changed: &[&str], diff_bytes: usize) -> Result<(), String> {
    if diff_bytes > MAX_DIFF_BYTES {
        return Err(format!("the uncommitted diff is {diff_bytes} bytes, over the {MAX_DIFF_BYTES} byte limit for shipping a lane"));
    }
    let private = changed.iter().find(|path| path.rsplit_once('.').is_some_and(|(_, extension)| PRIVATE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())));
    private.map_or(Ok(()), |path| Err(format!("{path} is a private asset, so the lane stays on this host")))
}

/// Runs a candidate's job on another host and returns its exit code, calling the notifier with the host once the command starts there; None leaves it to the local path.
pub(crate) type Offloader = fn(&Candidate, &Job, &dyn Fn(&str)) -> Option<u8>;
