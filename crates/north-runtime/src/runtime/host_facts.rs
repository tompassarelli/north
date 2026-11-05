use super::bounded;
use super::layout::Layout;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const RECORD_LIMIT: Duration = Duration::from_secs(120);
const REFRESH_AFTER: Duration = Duration::from_secs(15 * 60);
const PRIVATE_CHECKOUT: &str = "code/south/main";
const FACTS_DIR: &str = "host-facts";

pub(super) fn record_after_promotion(layout: &Layout, target: &str) {
    record(&layout.runtime(target).join("north"), true);
}

pub(super) fn record_if_stale(layout: &Layout) {
    if newest_age(&layout.state.join(FACTS_DIR)).is_none_or(|age| age >= REFRESH_AFTER) {
        record(&layout.current().join("north"), false);
    }
}

fn record(north: &Path, announce_missing_checkout: bool) {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    let Some(repo) = private_checkout(&home) else {
        if announce_missing_checkout {
            println!("north runtime: no {PRIVATE_CHECKOUT} checkout under {}; this host's config facts stay unpublished", home.display());
        }
        return;
    };
    let mut command = Command::new(north);
    command.args(["config", "agents", "live-refs", "--record", "--repo"]).arg(&repo).stdin(Stdio::null());
    match bounded::status(&mut command, RECORD_LIMIT) {
        Ok(status) if status.success() => {}
        Ok(status) => println!("north runtime: recording this host's config facts exited {status}"),
        Err(error) => println!("north runtime: recording this host's config facts failed: {error}"),
    }
}

fn private_checkout(home: &Path) -> Option<PathBuf> {
    Some(home.join(PRIVATE_CHECKOUT)).filter(|repo| repo.join(".git").exists())
}

fn newest_age(directory: &Path) -> Option<Duration> {
    std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .filter_map(|entry| entry.metadata().and_then(|metadata| metadata.modified()).ok())
        .filter_map(|modified| SystemTime::now().duration_since(modified).ok())
        .min()
}

#[cfg(test)]
mod tests {
    use super::{newest_age, private_checkout};
    use std::time::{Duration, SystemTime};

    #[test]
    fn facts_are_published_only_through_an_existing_south_checkout() {
        let home = tempfile::tempdir().expect("home");
        assert_eq!(private_checkout(home.path()), None);
        std::fs::create_dir_all(home.path().join("code/south/main/.git")).expect("checkout");
        assert_eq!(private_checkout(home.path()), Some(home.path().join("code/south/main")));
    }

    #[test]
    fn a_host_with_no_facts_file_is_stale_and_one_with_a_fresh_file_reports_its_age() {
        let directory = tempfile::tempdir().expect("facts");
        assert_eq!(newest_age(&directory.path().join("absent")), None);
        assert_eq!(newest_age(directory.path()), None);
        let file = std::fs::File::create(directory.path().join("gex45.json")).expect("facts file");
        file.set_modified(SystemTime::now() - Duration::from_secs(600)).expect("age it");
        let age = newest_age(directory.path()).expect("age");
        assert!(age >= Duration::from_secs(600) && age < Duration::from_secs(660), "{age:?}");
    }
}
