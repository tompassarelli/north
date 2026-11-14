use super::Follow;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

fn link(directory: &Path, name: &str, runtime: &str) -> PathBuf {
    let path = directory.join(name);
    symlink(directory.join("runtimes").join(runtime), &path).unwrap();
    path
}

#[test]
fn a_service_a_canary_started_follows_the_host_link_not_the_judged_one() {
    let directory = std::env::temp_dir().join(format!("follow-service-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let judged = link(&directory, "0123abc", "0123abc");
    let current = link(&directory, "current", "4567def");
    assert_eq!(Follow::service_on(judged, current.clone(), None).link(), current);
    let other = directory.join("elsewhere");
    assert_eq!(Follow::service_on(current.clone(), other, None).link(), current);
    std::fs::remove_dir_all(&directory).unwrap();
}
