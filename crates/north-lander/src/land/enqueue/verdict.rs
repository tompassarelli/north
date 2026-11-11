use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use north_base::land_ticket::{self, Phase};
use north_base::platform::records_mirror;

use super::super::queue;
use super::{Queued, git};

const LOCAL_STEP: Duration = Duration::from_millis(250);
const ORIGIN_STEP: Duration = Duration::from_secs(3);

pub(super) enum Verdict {
    Landed(String),
    Refused(String),
    Undecided,
}

/// Where a queued lane's verdict shows: its ticket in a decided area of the lander's queue, here or in this host's
/// mirror of the lander's records, and for a lane queued from another host, the destination branch on origin.
pub(super) struct Hand<'a> {
    pub queued: &'a Queued,
    pub state_home: &'a Path,
    pub git_dir: PathBuf,
    pub destination: &'a str,
}

impl Hand<'_> {
    pub(super) fn wait(&self, bound: Duration) -> Verdict {
        let Some(name) = self.queued.path.file_name() else { return Verdict::Undecided };
        let origin = &self.queued.ticket.origin;
        let queues = [queue::directory(self.state_home, origin), queue::directory(records_mirror::Records::here().home(), origin)];
        let step = if self.queued.outbox { ORIGIN_STEP } else { LOCAL_STEP };
        let until = Instant::now() + bound;
        loop {
            if let Some(verdict) = decided(&queues, name).or_else(|| self.queued.outbox.then(|| self.on_origin()).flatten()) {
                return verdict;
            }
            if Instant::now() >= until {
                return Verdict::Undecided;
            }
            std::thread::sleep(step);
        }
    }

    fn on_origin(&self) -> Option<Verdict> {
        let remote_ref = format!("refs/remotes/origin/{}", self.destination);
        git(&self.git_dir, &["fetch", "-q", "origin", &format!("+refs/heads/{}:{remote_ref}", self.destination)]).ok()?;
        let head = &self.queued.ticket.head;
        let unlanded = git(&self.git_dir, &["cherry", &remote_ref, head]).ok()?.lines().any(|line| line.starts_with('+'));
        if unlanded {
            return None;
        }
        let carried = git(&self.git_dir, &["log", "--cherry-mark", "--left-only", "--no-merges", "--format=%m%H", &format!("{remote_ref}...{head}")]).ok()?;
        let carrier = carried.lines().find_map(|line| line.strip_prefix('='))?;
        Some(Verdict::Landed(carrier.to_owned()))
    }
}

fn decided(queues: &[PathBuf], name: &OsStr) -> Option<Verdict> {
    let verdict = queues.iter().flat_map(|queue| land_ticket::directories(queue).into_iter().skip(1)).find_map(|area| queue::read(&area.join(name)).ok()?.verdict)?;
    Some(match (Phase::decided(&verdict), verdict.split_whitespace().nth(1)) {
        (Phase::Landed, Some(commit)) => Verdict::Landed(commit.to_owned()),
        _ => Verdict::Refused(verdict),
    })
}
