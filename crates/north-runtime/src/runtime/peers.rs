use super::layout::linked_sha;
use super::remote::{limit, ssh};
use crate::land_offload;
use crate::platform::process;
use std::path::Path;
use std::time::Duration;

const ASK: Duration = Duration::from_secs(60);

pub(super) struct Promoted {
    pub(super) host: String,
    pub(super) sha: String,
}

pub(super) fn promoted() -> Vec<Promoted> {
    promoted_among(land_offload::pull_sources(), |host| {
        let mut readlink = ssh(host);
        readlink.args(["readlink", ".local/state/north/current"]);
        let output = process::run_command_within(readlink, None, limit(ASK)).filter(|output| output.status.success())?;
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

pub(super) fn promoted_among(sources: Vec<String>, read_link: impl Fn(&str) -> Option<String>) -> Vec<Promoted> {
    sources
        .into_iter()
        .filter_map(|host| {
            let link = read_link(&host)?;
            let sha = linked_sha(Path::new(link.trim()))?.to_owned();
            Some(Promoted { host, sha })
        })
        .collect()
}

pub(super) fn newest(promoted: Vec<Promoted>, descends: impl Fn(&str, &str) -> bool) -> Option<Promoted> {
    promoted
        .into_iter()
        .reduce(|newest, next| if next.sha != newest.sha && descends(&next.sha, &newest.sha) { next } else { newest })
}
