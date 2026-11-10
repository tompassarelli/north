use super::{Better, Number, Record};
use super::super::{Weekly, render};

/// The graph holds a recording as one reading per number and week, and the fleet tick rebuilds the week-on-week comparison from them: this week must stay this week and the week before the week before.
#[test]
fn a_recording_read_back_from_its_graph_readings_compares_the_same_two_weeks() {
    let recorded_at = 1_794_100_000;
    let number = |name: &str, better, now, before| Number { name: name.to_owned(), better, now, before };
    let record = Record {
        recorded_at,
        numbers: vec![
            number("cost per landing", Better::Lower, Some((16.5, 581)), Some((9.9, 1116))),
            number("first-try success", Better::Higher, Some((0.9, 40)), Some((0.8, 3))),
            number("waits", Better::Lower, Some((3.0, 12)), None),
        ],
    };
    let told = north_graph_loops::scoreboard::Told { readings: record.readings(), at: Some(recorded_at) };
    let mut rebuilt = Record::told(&told).unwrap();
    rebuilt.numbers.sort_by(|left, right| left.name.cmp(&right.name));
    let mut expected = record;
    expected.numbers.sort_by(|left, right| left.name.cmp(&right.name));
    assert_eq!(rebuilt, expected);
    assert_eq!(rebuilt.worsened(), ["cost per landing"]);
}

/// The weekly report puts the live infra total beside the model spend it is read against, and says when the read model could not price it instead of hiding the spend line.
#[test]
fn the_weekly_money_line_sets_the_infra_total_beside_model_spend() {
    let report = Weekly { measures: Vec::new(), spend: [12.34, 9.87] };
    let text = render(&report, Ok("€152.99 + $304.52".to_owned()), 1_794_100_000);
    assert!(text.contains("money: model spend $12.34 this week, $9.87 the week before | infra €152.99 + $304.52 a month"), "{text}");
    let unread = render(&report, Err("manifest: no such file".to_owned()), 1_794_100_000);
    assert!(unread.contains("money: model spend $12.34 this week, $9.87 the week before | infra unread: manifest: no such file"), "{unread}");
}
