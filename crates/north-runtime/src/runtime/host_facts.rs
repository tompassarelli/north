use super::bounded;
use super::layout::Layout;
use super::remote::ssh;
use crate::land_offload;
use crate::platform::host;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const RECORD_LIMIT: Duration = Duration::from_secs(120);
const REFRESH_AFTER: Duration = Duration::from_secs(15 * 60);
const PRIVATE_CHECKOUT: &str = "code/south/main";
const FACTS_DIR: &str = "host-facts";
const UNPUBLISHED: &str = "unpublished";

pub(super) fn record_after_promotion(layout: &Layout, target: &str) {
    let published = record(&layout.runtime(target).join("north"), true);
    note(layout, &host::name(), published);
}

pub(super) fn record_if_stale(layout: &Layout) {
    let north = layout.current().join("north");
    let here = host::name();
    if newest_age(&layout.state.join(FACTS_DIR)).is_none_or(|age| age >= REFRESH_AFTER) || unpublished(layout, &here) {
        if unpublished(layout, &here) {
            println!("north runtime: retrying the publish of this host's config facts that failed earlier");
        }
        note(layout, &here, record(&north, false));
    }
    for relayed in land_offload::deployment().relays_facts.into_iter().filter(|relayed| unpublished(layout, &relayed.host)) {
        println!("north runtime: retrying the publish of {}'s config facts that failed earlier", relayed.host);
        relay(layout, &north, &relayed.destination, &relayed.host);
    }
}

fn record(north: &Path, announce_missing_checkout: bool) -> bool {
    let mut command = Command::new(north);
    command.args(["config", "agents", "live-refs", "--record"]).stdin(Stdio::null());
    if land_offload::publishes_host_facts_here() {
        let Some(repo) = private_checkout() else {
            if announce_missing_checkout {
                println!("north runtime: no ~/{PRIVATE_CHECKOUT} checkout; this host's config facts stay unpublished");
            }
            return true;
        };
        command.arg("--repo").arg(&repo);
    }
    match bounded::status(&mut command, RECORD_LIMIT) {
        Ok(status) if status.success() => true,
        Ok(status) => {
            println!("north runtime: recording this host's config facts exited {status}; retrying on the next tick");
            false
        }
        Err(error) => {
            println!("north runtime: recording this host's config facts failed: {error}; retrying on the next tick");
            false
        }
    }
}

pub(super) fn relay(layout: &Layout, north: &Path, destination: &str, host: &str) {
    let Some(repo) = private_checkout() else {
        println!("north runtime deploy: {destination}: no ~/{PRIVATE_CHECKOUT} checkout here, so {host}'s config facts stay unpublished");
        return;
    };
    let path = format!(".local/state/north/{FACTS_DIR}/{host}.json");
    let mut read = match ssh(destination).args(["cat", "--", &path]).stdin(Stdio::null()).stdout(Stdio::piped()).spawn() {
        Ok(read) => read,
        Err(error) => {
            println!("north runtime deploy: {destination}: cannot read {host}'s config facts: {error}; retrying on the next tick");
            return note(layout, host, false);
        }
    };
    let mut publish = Command::new(north);
    publish.args(["config", "agents", "live-refs", "--relay", host, "--repo"]).arg(&repo);
    if let Some(facts) = read.stdout.take() {
        publish.stdin(Stdio::from(facts));
    }
    let published = bounded::status(&mut publish, RECORD_LIMIT);
    let read = read.wait();
    let failure = match (read, published) {
        (Ok(read), _) if !read.success() => Some(format!("reading {path} exited {read}")),
        (_, Ok(status)) if status.success() => None,
        (_, Ok(status)) => Some(format!("publishing {host}'s config facts exited {status}")),
        (_, Err(error)) => Some(format!("publishing {host}'s config facts failed: {error}")),
    };
    if let Some(failure) = &failure {
        println!("north runtime deploy: {destination}: {failure}; retrying on the next tick");
    }
    note(layout, host, failure.is_none());
}

fn marker(layout: &Layout, host: &str) -> PathBuf {
    layout.state.join(FACTS_DIR).join(format!("{host}.{UNPUBLISHED}"))
}

fn unpublished(layout: &Layout, host: &str) -> bool {
    marker(layout, host).exists()
}

fn note(layout: &Layout, host: &str, published: bool) {
    let marker = marker(layout, host);
    let _ = match published {
        true => fs::remove_file(&marker),
        false => fs::create_dir_all(layout.state.join(FACTS_DIR)).and_then(|()| fs::write(&marker, "")),
    };
}

fn private_checkout() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(PRIVATE_CHECKOUT)).filter(|repo| repo.join(".git").exists())
}

fn newest_age(directory: &Path) -> Option<Duration> {
    fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .filter_map(|entry| entry.metadata().and_then(|metadata| metadata.modified()).ok())
        .filter_map(|modified| SystemTime::now().duration_since(modified).ok())
        .min()
}
