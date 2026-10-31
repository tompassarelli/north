use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use time::Date;

use crate::foundation::clock;

const DEFAULT_WINDOW_MINUTES: u64 = 5;

pub(super) struct Roots {
    pub codex: PathBuf,
    pub claude: PathBuf,
}

impl Roots {
    pub(super) fn from_environment() -> Self {
        let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
        let var = |name: &str| {
            env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        let codex = var("AGENTS_BUSY_CODEX_ROOT").unwrap_or_else(|| {
            var("NORTH_CODEX_CONVERSATION_HOME")
                .or_else(|| var("NORTH_CODEX_POOLED_HOME"))
                .unwrap_or_else(|| home.join(".local/state/north/codex-pooled"))
                .join("sessions")
        });
        let claude =
            var("AGENTS_BUSY_CLAUDE_ROOT").unwrap_or_else(|| home.join(".claude/projects"));
        Self { codex, claude }
    }
}

pub(super) fn run(_arguments: &[String]) -> u8 {
    let window = env::var("AGENTS_BUSY_MINUTES")
        .ok()
        .and_then(|minutes| minutes.parse().ok())
        .unwrap_or(DEFAULT_WINDOW_MINUTES);
    let today = clock::local_today();
    u8::from(!working(
        &Roots::from_environment(),
        today,
        window,
        SystemTime::now(),
    ))
}

pub(super) fn working(roots: &Roots, today: Date, window_minutes: u64, now: SystemTime) -> bool {
    let window = Duration::from_secs(window_minutes * 60);
    let days = [Some(today), today.previous_day()];
    days.into_iter()
        .flatten()
        .any(|day| written_within(&roots.codex.join(day_folder(day)), 1, window, now))
        || written_within(&roots.claude, 2, window, now)
}

fn day_folder(day: Date) -> String {
    format!(
        "{:04}/{:02}/{:02}",
        day.year(),
        u8::from(day.month()),
        day.day()
    )
}

fn written_within(root: &Path, depth: usize, window: Duration, now: SystemTime) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        match depth {
            1 => {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
                    && fresh(&path, window, now)
            }
            _ => path.is_dir() && written_within(&path, depth - 1, window, now),
        }
    })
}

fn fresh(path: &Path, window: Duration, now: SystemTime) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    now.duration_since(modified)
        .map_or(true, |age| age < window)
}
