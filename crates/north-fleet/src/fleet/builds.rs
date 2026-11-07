//! The builds a fleet session started that capacity admission still holds: a ticket waiting in its queue or a lease its wrapper still runs under. The session's turn may have ended on a report while it waits for them, so a stop ordered on that report would cut the build's result off from the session it wakes.
use std::fs;
use std::path::Path;

use serde_json::Value;

/// The owner the capacity gate gives a fleet session's build.
fn owner(session: &str) -> String {
    format!("gate:{session}")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Standing {
    Queued,
    Running,
}

impl Standing {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued in admission",
            Self::Running => "running under an admission lease",
        }
    }
}

/// The builds of `session` that admission under `admission` still holds, read against the process table at `proc`.
pub fn held(admission: &Path, proc: &Path, session: &str) -> Vec<Standing> {
    let owner = owner(session);
    [("queue", Standing::Queued), ("leases", Standing::Running)]
        .into_iter()
        .flat_map(|(directory, standing)| {
            let entries = fs::read_dir(admission.join(directory)).into_iter().flatten().filter_map(Result::ok);
            entries
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
                .filter_map(|path| serde_json::from_str::<Value>(&fs::read_to_string(path).ok()?).ok())
                .filter(|entry| entry["owner"] == owner.as_str() && wrapper_alive(proc, entry))
                .map(move |_| standing)
        })
        .collect()
}

fn wrapper_alive(proc: &Path, entry: &Value) -> bool {
    let (Some(pid), Some(start)) = (entry["wrapperPid"].as_u64(), entry["wrapperStart"].as_str()) else { return false };
    started(proc, pid).as_deref() == Some(start)
}

fn started(proc: &Path, pid: u64) -> Option<String> {
    let stat = fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?;
    let fields: Vec<&str> = stat.get(stat.rfind(')')? + 2..)?.split(' ').collect();
    if fields.first() == Some(&"Z") { None } else { fields.get(19).map(|field| (*field).to_owned()) }
}
