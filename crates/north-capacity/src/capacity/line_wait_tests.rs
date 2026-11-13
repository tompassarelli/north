use super::{Line, LineClass, LineJob, deadline, expected_start, refuses_at_once};

const NOW: u64 = 1_763_000_000;

fn job(rank: u8, queued_at: u64, started_at: Option<u64>, ended_at: Option<u64>) -> LineJob {
    LineJob { rank, check: "cargo test".to_owned(), class: LineClass::LandCheck, queued_at, started_at, ended_at, deadline: None }
}

fn line(slots: u32, running: u32, queued: impl Fn(u64) -> LineJob) -> Line {
    let history = (0..10).map(|index| job(3, NOW - 4000 - index * 400, Some(NOW - 3900 - index * 400), Some(NOW - 3600 - index * 400)));
    let held = (0..running).map(|_| job(3, NOW - 90, Some(NOW - 60), None));
    let waiting = (0..40).map(|index| queued(NOW - 50 + index));
    Line { slots, jobs: history.chain(held).chain(waiting).collect() }
}

#[test]
fn line_wait_refuses_a_landing_behind_a_full_line_at_once_and_never_on_a_free_slot() {
    let full = line(18, 18, |queued_at| job(2, queued_at, None, None));
    let wait = expected_start(&full, 2, NOW);
    assert_eq!(wait, 840, "18 slots end their 300 s checks in 240 s, then 40 landings ahead take three rounds of slots");
    assert!(refuses_at_once(2) && wait > deadline(2));
    let free = line(18, 17, |queued_at| LineJob { class: LineClass::Background, ..job(4, queued_at, None, None) });
    assert!((0..4).all(|rank| expected_start(&free, rank, NOW) == 0), "a free slot with only background jobs behind starts every land-check rank at once");
    let held = LineJob { check: "north-offload-hold".to_owned(), deadline: Some(1000), ..job(2, NOW - 110, Some(NOW - 100), None) };
    let ended = line(1, 0, |queued_at| job(3, queued_at, None, None)).jobs.into_iter().filter(|job| job.ended_at.is_some());
    let behind_a_hold = Line { slots: 1, jobs: ended.chain([held]).collect() };
    assert_eq!(expected_start(&behind_a_hold, 2, NOW), 900, "a job behind a hold with no ended hold on record waits out the hold's remaining deadline, not the median of the host's other checks");
}
