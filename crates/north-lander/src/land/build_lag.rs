use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use super::candidate::git;
use north_runtime_follow::runtime_of;

pub const LANDER_INPUTS: &[&str] = &["crates/north-landing/", "crates/north-land-checks/", "crates/north-lander/", "crates/north-base/", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml"];
/// How long main may hold lander code the running build lacks: promote-on-landing moves the link within minutes, so a count of changes says nothing.
pub const BEHIND_LIMIT_SECS: u64 = 15 * 60;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lag {
    Unpromoted,
    Unknown { build: String, why: String },
    Current { build: String },
    Behind { build: String, main: String, changes: usize, secs: u64 },
}

impl Lag {
    pub fn over_limit(&self) -> bool {
        matches!(self, Lag::Behind { secs, .. } if *secs > BEHIND_LIMIT_SECS)
    }

    /// Whether `other` describes the same lag, apart from how long it has lasted.
    pub fn same_episode(&self, other: &Lag) -> bool {
        match (self, other) {
            (Lag::Behind { build, main, .. }, Lag::Behind { build: other_build, main: other_main, .. }) => build == other_build && main == other_main && self.over_limit() == other.over_limit(),
            _ => self == other,
        }
    }

    pub fn note(&self) -> String {
        match self {
            Lag::Unpromoted => "the lander runs no promoted runtime, so its build is unnamed".to_owned(),
            Lag::Unknown { build, why } => format!("the lander runs build {}, lag unknown: {why}", short(build)),
            Lag::Current { build } => format!("the lander runs build {}, main's lander code", short(build)),
            Lag::Behind { build, main, changes, secs } => {
                let over = if self.over_limit() { format!(", over the limit of {} minutes", BEHIND_LIMIT_SECS / 60) } else { String::new() };
                format!("the lander runs older build {}, {changes} lander changes behind main {}, the first landed {} minutes ago{over}", short(build), short(main), secs / 60)
            }
        }
    }
}

fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

fn own_build() -> Option<&'static str> {
    static BUILD: OnceLock<Option<String>> = OnceLock::new();
    BUILD.get_or_init(|| std::env::current_exe().ok().and_then(|exe| runtime_of(&exe)).map(|(build, _)| build)).as_deref()
}

/// The lag of `build` behind `main` at `now` (epoch seconds): the lander-input commits it lacks and how long ago the first of them was committed.
pub fn measure(checkout: &Path, build: Option<&str>, main: &str, now: u64) -> Lag {
    let Some(build) = build else { return Lag::Unpromoted };
    let unknown = |why: String| Lag::Unknown { build: build.to_owned(), why };
    let range = format!("{build}..{main}");
    let mut args = vec!["log", "--format=%ct", range.as_str(), "--"];
    args.extend(LANDER_INPUTS);
    let times = match git(checkout, &args) {
        Ok(times) => times.lines().map(|time| time.trim().parse::<u64>()).collect::<Result<Vec<u64>, _>>(),
        Err(error) => return unknown(error),
    };
    match times {
        Ok(times) if times.is_empty() => Lag::Current { build: build.to_owned() },
        Ok(times) => Lag::Behind { build: build.to_owned(), main: main.to_owned(), changes: times.len(), secs: now.saturating_sub(times.iter().copied().min().unwrap_or(now)) },
        Err(error) => unknown(error.to_string()),
    }
}

fn north_checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north/main"))
}

/// The lag of the lander serving north, and none from a lander serving any other repository; the lander doctor's link-behind bound pages it.
pub fn report(repo: &Path) -> Option<Lag> {
    let checkout = north_checkout();
    if !same_checkout(repo, &checkout) {
        return None;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    Some(match git(&checkout, &["rev-parse", "--verify", "refs/remotes/origin/main^{commit}"]) {
        Ok(main) => measure(&checkout, own_build(), main.trim(), now),
        Err(error) => own_build().map_or(Lag::Unpromoted, |build| Lag::Unknown { build: build.to_owned(), why: error }),
    })
}

fn same_checkout(repo: &Path, north: &Path) -> bool {
    matches!((repo.canonicalize(), north.canonicalize()), (Ok(repo), Ok(north)) if repo == north)
}

#[cfg(test)]
#[path = "build_lag_tests.rs"]
mod tests;
