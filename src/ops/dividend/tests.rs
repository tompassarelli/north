use super::checks::{evaluate, Check, Cpu, Facts, Standing, DAY_S};
use super::{live, tick, Bet, Board};

const NOW: f64 = 1_793_300_000.0;
const DATE: &str = "2026-10-29";
const BODY: &str = "Why.\n\n**Dividend:** code-only boxes placed in Anthropic cloud per day > 0. Checked by `north ops dividend cloud-lane`.\n**Dividend:** Public 0.1 work items closed per day >= 1. Checked by `north ops dividend release-closed`.\n\nOwner: x";

#[derive(Default)]
struct Fake {
    bets: Vec<Bet>,
    comments: Vec<(u64, String)>,
    alerts: Vec<String>,
}

impl Board for Fake {
    fn bets(&self) -> Result<Vec<Bet>, String> {
        Ok(self.bets.clone())
    }

    fn comment(&mut self, number: u64, text: &str) -> Result<(), String> {
        self.comments.push((number, text.to_owned()));
        Ok(())
    }

    fn alert(&mut self, text: &str) -> Result<(), String> {
        self.alerts.push(text.to_owned());
        Ok(())
    }
}

fn bet(number: u64, body: &str) -> Bet {
    Bet {
        number,
        title: format!("bet {number}"),
        body: body.into(),
        comments: Vec::new(),
    }
}

fn paying() -> Facts {
    Facts {
        at: NOW,
        cloud_launches: vec![NOW - 3600.0],
        credit: (true, "credit live".into()),
        cpu: Ok(Cpu {
            laptop: 3600.0,
            total: 36_000.0,
            unmeasured: Vec::new(),
        }),
        landings: Ok(vec![
            (NOW - 100.0, "smashcraft".into()),
            (NOW - 200.0, "wisp".into()),
            (NOW - 300.0, "north".into()),
        ]),
        release_closed: Ok((1..=7)
            .map(|day| NOW - f64::from(day) * DAY_S + 60.0)
            .collect()),
    }
}

fn decayed() -> Facts {
    Facts {
        cloud_launches: vec![NOW - 3.0 * DAY_S],
        release_closed: Ok(vec![NOW - DAY_S]),
        ..paying()
    }
}

fn standing(check: Check, facts: &Facts) -> Standing {
    evaluate(check, facts).standing
}

#[test]
fn a_paying_bet_comments_paying_and_alerts_no_one() {
    let mut board = Fake {
        bets: vec![bet(7, BODY)],
        ..Fake::default()
    };
    let report = tick(&mut board, &paying(), DATE, false);
    assert!(!report.failed, "{:?}", report.lines);
    assert_eq!(board.comments.len(), 1);
    let (number, text) = &board.comments[0];
    assert_eq!(*number, 7);
    assert!(
        text.starts_with("north ops dividend 2026-10-29: PAYING\n"),
        "{text}"
    );
    assert!(
        text.contains("- cloud-lane PAYING:") && text.contains("- release-closed PAYING:"),
        "{text}"
    );
    assert!(board.alerts.is_empty());
}

#[test]
fn a_decayed_bet_comments_decayed_and_raises_one_alert_to_the_proxy() {
    let mut board = Fake {
        bets: vec![bet(7, BODY)],
        ..Fake::default()
    };
    let report = tick(&mut board, &decayed(), DATE, false);
    assert!(!report.failed, "{:?}", report.lines);
    assert!(
        board.comments[0]
            .1
            .starts_with("north ops dividend 2026-10-29: DECAYED\n"),
        "{}",
        board.comments[0].1
    );
    assert_eq!(board.alerts.len(), 1);
    assert!(
        board.alerts[0].contains("bet #7 bet 7 is DECAYED")
            && board.alerts[0].contains("cloud-lane DECAYED"),
        "{}",
        board.alerts[0]
    );
}

#[test]
fn a_bet_comments_once_a_day_and_again_the_next_day() {
    let mut board = Fake {
        bets: vec![bet(7, BODY)],
        ..Fake::default()
    };
    board.bets[0]
        .comments
        .push("north ops dividend 2026-10-29: DECAYED\n\n- x".into());
    tick(&mut board, &decayed(), DATE, false);
    assert!(
        board.comments.is_empty() && board.alerts.is_empty(),
        "a bet already commented today is left alone"
    );
    tick(&mut board, &decayed(), "2026-10-30", false);
    assert_eq!((board.comments.len(), board.alerts.len()), (1, 1));
}

#[test]
fn a_dry_run_comments_and_alerts_nothing() {
    let mut board = Fake {
        bets: vec![bet(7, BODY)],
        ..Fake::default()
    };
    let report = tick(&mut board, &decayed(), DATE, true);
    assert!(board.comments.is_empty() && board.alerts.is_empty());
    assert!(
        report
            .lines
            .iter()
            .any(|line| line.contains("would comment DECAYED on #7")),
        "{:?}",
        report.lines
    );
}

#[test]
fn a_bet_with_no_check_and_a_bet_whose_checks_are_unread_get_no_comment() {
    let facts = Facts {
        release_closed: Err("gh down".into()),
        ..paying()
    };
    let unread = "**Dividend:** closed per day. Checked by `north ops dividend release-closed`.";
    let mut board = Fake {
        bets: vec![
            bet(1, "**Dividend:** something nobody measures"),
            bet(2, unread),
        ],
        ..Fake::default()
    };
    tick(&mut board, &facts, DATE, false);
    assert!(board.comments.is_empty() && board.alerts.is_empty());
}

#[test]
fn one_decayed_check_decays_a_bet_whose_other_checks_pay() {
    let facts = Facts {
        cloud_launches: Vec::new(),
        ..paying()
    };
    let mut board = Fake {
        bets: vec![bet(7, BODY)],
        ..Fake::default()
    };
    tick(&mut board, &facts, DATE, false);
    assert!(
        board.comments[0]
            .1
            .starts_with("north ops dividend 2026-10-29: DECAYED\n")
            && board.comments[0].1.contains("- release-closed PAYING"),
        "{}",
        board.comments[0].1
    );
}

#[test]
fn the_dividend_section_is_a_bold_line_or_a_heading_and_names_each_check_once() {
    let heading = "## Verification\nnorth ops dividend game-share is not here\n\n## Dividend\nmetric: laptop under 80%. `north ops dividend laptop-share` and `north ops dividend laptop-share`, `north ops dividend nonsense`\n\n## Notes\n`north ops dividend cloud-lane`\n";
    assert_eq!(super::bets::checks(heading), [Check::LaptopShare]);
    assert_eq!(
        super::bets::checks(BODY),
        [Check::CloudLane, Check::ReleaseClosed]
    );
    assert!(super::bets::checks("no section").is_empty());
}

#[test]
fn cloud_lane_pays_with_a_placement_in_the_last_day_and_retires_with_the_credit() {
    assert_eq!(standing(Check::CloudLane, &paying()), Standing::Paying);
    assert_eq!(standing(Check::CloudLane, &decayed()), Standing::Decayed);
    let expired = Facts {
        credit: (false, "cloud credit: expired 2026-11-05T00:00Z".into()),
        ..decayed()
    };
    let verdict = evaluate(Check::CloudLane, &expired);
    assert!(
        verdict.standing == Standing::Paying && verdict.line.starts_with("retired"),
        "{verdict:?}"
    );
}

#[test]
fn laptop_share_decays_at_eighty_percent_and_is_unread_without_measurements() {
    let share = |laptop: f64, total: f64| Facts {
        cpu: Ok(Cpu {
            laptop,
            total,
            unmeasured: Vec::new(),
        }),
        ..paying()
    };
    assert_eq!(
        standing(Check::LaptopShare, &share(79.0, 100.0)),
        Standing::Paying
    );
    assert_eq!(
        standing(Check::LaptopShare, &share(80.0, 100.0)),
        Standing::Decayed
    );
    assert_eq!(
        standing(Check::LaptopShare, &share(0.0, 0.0)),
        Standing::Unread
    );
    assert_eq!(
        standing(
            Check::LaptopShare,
            &Facts {
                cpu: Err("no log".into()),
                ..paying()
            }
        ),
        Standing::Unread
    );
}

#[test]
fn game_share_needs_sixty_percent_of_the_landings_of_the_last_seven_days() {
    let landed = |games: usize, others: usize| {
        let rows = (0..games)
            .map(|i| (NOW - i as f64, "smashcraft".to_owned()))
            .chain((0..others).map(|i| (NOW - i as f64, "north".to_owned())))
            .chain([(NOW - 9.0 * DAY_S, "north".to_owned())]);
        Facts {
            landings: Ok(rows.collect()),
            ..paying()
        }
    };
    assert_eq!(standing(Check::GameShare, &landed(3, 2)), Standing::Paying);
    assert_eq!(standing(Check::GameShare, &landed(5, 5)), Standing::Decayed);
    assert_eq!(
        standing(
            Check::GameShare,
            &Facts {
                landings: Ok(Vec::new()),
                ..paying()
            }
        ),
        Standing::Decayed
    );
}

#[test]
fn release_closed_needs_one_item_a_day_over_the_week() {
    let closed = |count: usize| Facts {
        release_closed: Ok((0..count).map(|i| NOW - i as f64 * 60.0).collect()),
        ..paying()
    };
    assert_eq!(standing(Check::ReleaseClosed, &closed(7)), Standing::Paying);
    assert_eq!(
        standing(Check::ReleaseClosed, &closed(6)),
        Standing::Decayed
    );
}

#[test]
fn cpu_seconds_count_only_the_rows_of_the_last_seven_days() {
    let log = "{\"at\":\"2026-10-29T10:46:14.349Z\",\"cpuSeconds\":100.5}\n{\"at\":\"2026-10-20T10:46:14.349Z\",\"cpuSeconds\":900}\nnot json\n{\"at\":\"2026-10-26T00:00:00Z\",\"cpuSeconds\":10}\n";
    let at = crate::foundation::clock::Timestamp::parse_rfc3339("2026-10-29T12:00:00Z")
        .unwrap()
        .unix_seconds_f64();
    assert_eq!(live::usage_seconds(log, at), 110.5);
}
