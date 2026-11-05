use super::*;

const HOUR: u64 = 60 * 60;
const NOW: u64 = 1_000 * HOUR;

type Row<'a> = (bool, bool, Option<&'a str>, Option<bool>, &'a str);

fn verdict(pick: &Pick) -> String {
    match pick {
        Pick::Included => "include".to_owned(),
        Pick::Decided(verdict) => format!("send back: {verdict}"),
        Pick::Undecided(reason) => format!("undecided: {reason}"),
        Pick::Deferred(reason) => format!("defer: {reason}"),
    }
}

#[test]
fn judge_truth_table() {
    let rows: [Row; 12] = [
        (false, true, None, None, "include"),
        (true, false, Some("lane-a"), None, "include"),
        (false, false, None, Some(false), "defer: deferred: lane-b conflicts only with lanes ahead of it in this candidate (shared.txt)"),
        (true, false, None, Some(false), "defer: deferred: lane-b conflicts only with lanes ahead of it in this candidate (shared.txt)"),
        (false, false, None, Some(true), "send back: conflict shared.txt"),
        (false, false, Some("lane-a"), Some(true), "defer: deferred: lane-b conflicts on shared.txt while lane-a"),
        (false, true, None, Some(true), "send back: conflict shared.txt"),
        (false, true, Some("lane-a"), Some(true), "defer: deferred: lane-b conflicts on shared.txt while lane-a"),
        (true, true, Some("lane-a"), Some(true), "send back: conflict shared.txt"),
        (true, true, None, Some(true), "send back: conflict shared.txt"),
        (true, false, Some("lane-a"), Some(true), "send back: conflict shared.txt"),
        (false, false, Some("lane-a"), Some(false), "defer: deferred: lane-b conflicts only with lanes ahead of it in this candidate (shared.txt)"),
    ];
    for (red_fix, bare_main, queued_ahead, conflict, expected) in rows {
        let attempt = match conflict {
            None => Attempt::Clean,
            Some(with_main) => Attempt::Conflict { files: "shared.txt", with_main },
        };
        let judged = verdict(&judge(&Judged { branch: "lane-b", red_fix, bare_main, queued_ahead }, &attempt));
        assert!(judged.starts_with(expected), "red_fix={red_fix} bare_main={bare_main} queued_ahead={queued_ahead:?} conflict={conflict:?}: {judged}");
    }
}

fn decided(queued: u128, hours_ago: u64, verdict: &str) -> Decided<'_> {
    Decided { queued, decided_secs: NOW - hours_ago * HOUR, verdict }
}

#[test]
fn a_sent_back_lane_keeps_the_position_of_its_first_ticket_in_the_run() {
    let run = [decided(30, 1, REPLACED), decided(20, 2, "conflict shared.txt"), decided(10, 3, "conflict shared.txt")];
    assert_eq!(position(40, &run, NOW), 10);
}

#[test]
fn a_run_stops_at_the_first_other_verdict() {
    let run = [decided(30, 1, "conflict shared.txt"), decided(20, 2, "failed https://example.invalid/run"), decided(10, 3, "conflict shared.txt")];
    assert_eq!(position(40, &run, NOW), 30);
    assert_eq!(position(40, &[decided(30, 1, "landed abc")], NOW), 40);
}

#[test]
fn a_run_whose_last_send_back_is_25_hours_old_keeps_no_position() {
    let run = [decided(20, 25, "conflict shared.txt"), decided(10, 26, "conflict shared.txt")];
    assert_eq!(position(40, &run, NOW), 40);
    assert_eq!(position(40, &[decided(20, 23, "conflict shared.txt")], NOW), 20);
}

#[test]
fn replacements_alone_keep_no_position() {
    assert_eq!(position(40, &[decided(20, 1, REPLACED)], NOW), 40);
    assert_eq!(position(40, &[], NOW), 40);
}
