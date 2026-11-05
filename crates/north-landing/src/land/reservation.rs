use super::candidate::Pick;

pub const KEPT_SECS: u64 = 24 * 60 * 60;

pub const REPLACED: &str = "replaced by a newer ticket for this lane";

pub struct Decided<'a> {
    pub queued: u128,
    pub decided_secs: u64,
    pub verdict: &'a str,
}

fn sent_back(verdict: &str) -> bool {
    verdict.starts_with("conflict ")
}

pub fn run<'a, 'b>(newest_first: &'b [Decided<'a>]) -> &'b [Decided<'a>] {
    let length = newest_first.iter().take_while(|decided| sent_back(decided.verdict) || decided.verdict == REPLACED).count();
    &newest_first[..length]
}

pub fn position(queued: u128, newest_first: &[Decided], now_secs: u64) -> u128 {
    let run = run(newest_first);
    let kept = run.iter().find(|decided| sent_back(decided.verdict)).is_some_and(|last| now_secs.saturating_sub(last.decided_secs) < KEPT_SECS);
    if !kept {
        return queued;
    }
    run.iter().map(|decided| decided.queued).fold(queued, u128::min)
}

pub enum Attempt<'a> {
    Clean,
    Conflict { files: &'a str, with_main: bool },
}

pub struct Judged<'a> {
    pub branch: &'a str,
    pub red_fix: bool,
    pub bare_main: bool,
    pub queued_ahead: Option<&'a str>,
}

pub fn judge(ticket: &Judged, attempt: &Attempt) -> Pick {
    let Attempt::Conflict { files, with_main } = attempt else { return Pick::Included };
    let branch = ticket.branch;
    if !ticket.bare_main && !with_main {
        return Pick::Deferred(format!("deferred: {branch} conflicts only with lanes ahead of it in this candidate ({files}); it stays queued for the next candidate"));
    }
    match ticket.queued_ahead {
        Some(ahead) if !ticket.red_fix => Pick::Deferred(format!("deferred: {branch} conflicts on {files} while {ahead}, ahead of it on those files, is still queued; it is judged on main once {ahead} is decided")),
        _ => Pick::Decided(format!("conflict {files}")),
    }
}
