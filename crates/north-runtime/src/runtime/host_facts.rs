use super::bounded;
use super::layout::Layout;
use super::remote::ssh;
use crate::land_offload;
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
    let mut command = Command::new(north);
    command.args(["config", "agents", "live-refs", "--record"]).stdin(Stdio::null());
    if land_offload::publishes_host_facts_here() {
        let Some(repo) = private_checkout() else {
            if announce_missing_checkout {
                println!("north runtime: no ~/{PRIVATE_CHECKOUT} checkout; this host's config facts stay unpublished");
            }
            return;
        };
        command.arg("--repo").arg(&repo);
    }
    match bounded::status(&mut command, RECORD_LIMIT) {
        Ok(status) if status.success() => {}
        Ok(status) => println!("north runtime: recording this host's config facts exited {status}"),
        Err(error) => println!("north runtime: recording this host's config facts failed: {error}"),
    }
}

pub(super) fn relay(north: &Path, destination: &str, host: &str) {
    let Some(repo) = private_checkout() else {
        println!("north runtime deploy: {destination}: no ~/{PRIVATE_CHECKOUT} checkout here, so {host}'s config facts stay unpublished");
        return;
    };
    let path = format!(".local/state/north/{FACTS_DIR}/{host}.json");
    let mut read = match ssh(destination).args(["cat", "--", &path]).stdin(Stdio::null()).stdout(Stdio::piped()).spawn() {
        Ok(read) => read,
        Err(error) => return println!("north runtime deploy: {destination}: cannot read {host}'s config facts: {error}"),
    };
    let mut publish = Command::new(north);
    publish.args(["config", "agents", "live-refs", "--relay", host, "--repo"]).arg(&repo);
    if let Some(facts) = read.stdout.take() {
        publish.stdin(Stdio::from(facts));
    }
    let published = bounded::status(&mut publish, RECORD_LIMIT);
    let read = read.wait();
    match (read, published) {
        (Ok(read), _) if !read.success() => println!("north runtime deploy: {destination}: reading {path} exited {read}, so {host}'s config facts stay unpublished"),
        (_, Ok(status)) if status.success() => {}
        (_, Ok(status)) => println!("north runtime deploy: {destination}: publishing {host}'s config facts exited {status}"),
        (_, Err(error)) => println!("north runtime deploy: {destination}: publishing {host}'s config facts failed: {error}"),
    }
}

fn private_checkout() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
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
