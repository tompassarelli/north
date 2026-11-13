use std::cmp::Reverse;
use std::collections::BinaryHeap;

use north_base::foundation::offload::LAND_LINE_DEADLINE_SECONDS;

const BACKGROUND_DEADLINE_SECONDS: u64 = 3600;
const BACKGROUND_RANK: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineClass {
    LandCheck,
    Background,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineJob {
    pub rank: u8,
    pub check: String,
    pub class: LineClass,
    pub queued_at: u64,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub deadline: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub slots: u32,
    pub jobs: Vec<LineJob>,
}

pub fn deadline(rank: u8) -> u64 {
    if rank >= BACKGROUND_RANK { BACKGROUND_DEADLINE_SECONDS } else { LAND_LINE_DEADLINE_SECONDS }
}

pub fn refuses_at_once(rank: u8) -> bool {
    matches!(rank, 2 | 3)
}

pub fn expected_start(line: &Line, rank: u8, now: u64) -> u64 {
    let slots = line.slots as usize;
    if slots == 0 {
        return u64::MAX;
    }
    let runs = Runs::of(&line.jobs);
    let live = line.jobs.iter().filter(|job| job.ended_at.is_none());
    let mut running: Vec<u64> = live.clone().filter_map(|job| job.started_at.map(|started| runs.expected(job).saturating_sub(now.saturating_sub(started)))).collect();
    running.sort_unstable();
    let busy = running.len().min(slots);
    let mut frees: BinaryHeap<Reverse<u64>> = running.split_off(running.len() - busy).into_iter().chain(std::iter::repeat_n(0, slots - busy)).map(Reverse).collect();
    let mut ahead: Vec<&LineJob> = live.filter(|job| job.started_at.is_none() && (job.rank, job.queued_at) <= (rank, now)).collect();
    ahead.sort_by_key(|job| (job.rank, job.queued_at));
    for job in ahead {
        let Some(Reverse(free)) = frees.pop() else { break };
        frees.push(Reverse(free.saturating_add(runs.expected(job))));
    }
    frees.peek().map_or(0, |Reverse(free)| *free)
}

struct Runs<'a> {
    ended: Vec<(&'a LineJob, u64)>,
}

impl<'a> Runs<'a> {
    fn of(jobs: &'a [LineJob]) -> Self {
        Self { ended: jobs.iter().filter_map(|job| Some((job, job.ended_at?.checked_sub(job.started_at?)?))).collect() }
    }

    fn expected(&self, job: &LineJob) -> u64 {
        let same_check = self.median(|ended| ended.check == job.check && ended.class == job.class);
        if let Some(deadline) = job.deadline {
            return same_check.map_or(deadline, |seconds| seconds.min(deadline));
        }
        let same_rank = || self.median(|ended| ended.rank == job.rank && ended.class == job.class);
        same_check.or_else(same_rank).or_else(|| self.median(|_| true)).unwrap_or(0)
    }

    fn median(&self, keep: impl Fn(&LineJob) -> bool) -> Option<u64> {
        let mut seconds: Vec<u64> = self.ended.iter().filter(|(job, _)| keep(job)).map(|(_, seconds)| *seconds).collect();
        seconds.sort_unstable();
        seconds.get(seconds.len().checked_sub(1)? / 2).copied()
    }
}

#[cfg(test)]
#[path = "line_wait_tests.rs"]
mod tests;
