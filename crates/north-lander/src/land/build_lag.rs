use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::after;
use super::candidate::git;
use super::lander::state_home;
use north_base::platform::harness::is_sha;

pub const LANDER_INPUTS: &[&str] = &["crates/north-landing/", "crates/north-land-checks/", "crates/north-lander/", "crates/north-base/", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml"];
pub const BEHIND_LIMIT: usize = 3;
const PROXY: &str = "proxy";
const ALARMED: &str = "lander-build-alarmed";

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

pub fn build_of(executable: &Path) -> Option<String> {
    executable.ancestors().find_map(|dir| {
        let name = dir.file_name()?.to_str()?;
        (is_sha(name) && dir.parent()?.file_name()? == "runtimes").then(|| name.to_owned())
    })
}

fn own_build() -> Option<&'static str> {
    static BUILD: OnceLock<Option<String>> = OnceLock::new();
    BUILD.get_or_init(|| std::env::current_exe().ok().and_then(|exe| build_of(&exe))).as_deref()
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

pub fn alarm_once(home: &Path, lag: &Lag, page: &dyn Fn(&str) -> bool) -> bool {
    let build = match lag {
        Lag::Behind { build, .. } if lag.over_limit() => build,
        Lag::Unknown { .. } => return false,
        _ => {
            let _ = fs::remove_file(home.join(ALARMED));
            return false;
        }
    };
    let marked = fs::read_to_string(home.join(ALARMED)).unwrap_or_default();
    if marked.lines().any(|alarmed| alarmed == build) {
        return false;
    }
    let told = page(&format!("north land: {}; lanes are judged by lander code main has replaced, so read why north land promote holds the lander link", lag.note()));
    if told {
        let _ = fs::create_dir_all(home);
        let _ = fs::write(home.join(ALARMED), format!("{marked}{build}\n"));
    }
    told
}

fn north_checkout() -> PathBuf {
    std::env::var_os("NORTH_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north/main"))
}

pub fn report(repo: &Path) -> Option<String> {
    let checkout = north_checkout();
    if !same_checkout(repo, &checkout) {
        return None;
    }
    let lag = match git(&checkout, &["rev-parse", "--verify", "origin/main^{commit}"]) {
        Ok(main) => measure(&checkout, own_build(), main.trim()),
        Err(error) => own_build().map_or(Lag::Unpromoted, |build| Lag::Unknown { build: build.to_owned(), why: error }),
    };
    alarm_once(&state_home().join("north"), &lag, &|text| after::send(PROXY, text));
    Some(lag.note())
}

fn same_checkout(repo: &Path, north: &Path) -> bool {
    matches!((repo.canonicalize(), north.canonicalize()), (Ok(repo), Ok(north)) if repo == north)
}

#[cfg(test)]
#[path = "build_lag_tests.rs"]
mod tests;
