use super::layout::linked_sha;
use super::remote::{limit, ssh};
use crate::ops::land_offload;
use crate::platform::process;
use std::path::Path;
use std::time::Duration;

const ASK: Duration = Duration::from_secs(60);

pub(super) struct Promoted {
    pub(super) host: String,
    pub(super) sha: String,
}

pub(super) fn promoted() -> Vec<Promoted> {
    land_offload::other_destinations()
        .into_iter()
        .filter_map(|host| {
            let mut readlink = ssh(&host);
            readlink.args(["readlink", ".local/state/north/current"]);
            let output = process::run_command_within(readlink, None, limit(ASK)).filter(|output| output.status.success())?;
            let link = String::from_utf8_lossy(&output.stdout);
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

