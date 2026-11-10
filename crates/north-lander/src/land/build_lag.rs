use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::candidate::git;
use north_runtime_follow::runtime_of;

pub const LANDER_INPUTS: &[&str] = &["crates/north-landing/", "crates/north-land-checks/", "crates/north-lander/", "crates/north-base/", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml"];
pub const BEHIND_LIMIT: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lag {
    Unpromoted,
    Unknown { build: String, why: String },
    Current { build: String },
    Behind { build: String, main: String, changes: usize },
}

impl Lag {
    pub fn over_limit(&self) -> bool {
        matches!(self, Lag::Behind { changes, .. } if *changes > BEHIND_LIMIT)
    }

    pub fn note(&self) -> String {
        match self {
            Lag::Unpromoted => "the lander runs no promoted runtime, so its build is unnamed".to_owned(),
            Lag::Unknown { build, why } => format!("the lander runs build {}, lag unknown: {why}", short(build)),
            Lag::Current { build } => format!("the lander runs build {}, main's lander code", short(build)),
            Lag::Behind { build, main, changes } => {
                let over = if self.over_limit() { format!(", over the limit of {BEHIND_LIMIT}") } else { String::new() };
                format!("the lander runs older build {}, {changes} lander changes behind main {}{over}", short(build), short(main))
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

pub fn measure(checkout: &Path, build: Option<&str>, main: &str) -> Lag {
    let Some(build) = build else { return Lag::Unpromoted };
    let unknown = |why: String| Lag::Unknown { build: build.to_owned(), why };
    let range = format!("{build}..{main}");
    let mut args = vec!["rev-list", "--count", range.as_str(), "--"];
    args.extend(LANDER_INPUTS);
    match git(checkout, &args).map(|count| count.trim().parse::<usize>()) {
        Ok(Ok(0)) => Lag::Current { build: build.to_owned() },
        Ok(Ok(changes)) => Lag::Behind { build: build.to_owned(), main: main.to_owned(), changes },
        Ok(Err(error)) => unknown(error.to_string()),
        Err(error) => unknown(error),
    }
}

fn north_checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north/main"))
}

/// The lag note of the lander serving north, and none from a lander serving any other repository; the lander doctor's link-behind bound pages it.
pub fn report(repo: &Path) -> Option<String> {
    let checkout = north_checkout();
    if !same_checkout(repo, &checkout) {
        return None;
    }
    let lag = match git(&checkout, &["rev-parse", "--verify", "origin/main^{commit}"]) {
        Ok(main) => measure(&checkout, own_build(), main.trim()),
        Err(error) => own_build().map_or(Lag::Unpromoted, |build| Lag::Unknown { build: build.to_owned(), why: error }),
    };
    Some(lag.note())
}

fn same_checkout(repo: &Path, north: &Path) -> bool {
    matches!((repo.canonicalize(), north.canonicalize()), (Ok(repo), Ok(north)) if repo == north)
}

#[cfg(test)]
#[path = "build_lag_tests.rs"]
mod tests;
