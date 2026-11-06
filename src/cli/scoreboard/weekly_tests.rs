use super::*;
use crate::cli::scoreboard::{Figure, Measure};

fn share(name: &str, better: Better, now: (usize, usize), before: (usize, usize)) -> Measure {
    let figure = |(hits, of)| Figure::Share { hits, of, pending: 0, pending_label: "" };
    Measure { name: name.into(), better, source: "test", weeks: [figure(now), figure(before)] }
}

#[test]
fn worse_follows_the_direction_each_number_improves() {
    let record = Record::of(
        &[
            share("rework", Better::Lower, (30, 100), (20, 100)),
            share("first try", Better::Higher, (40, 100), (60, 100)),
            share("steady", Better::Higher, (59, 100), (60, 100)),
            share("better", Better::Lower, (10, 100), (20, 100)),
        ],
        1,
    );
    assert_eq!(record.worsened(), ["rework", "first try"]);
}

#[test]
fn a_week_with_too_few_observations_is_not_compared() {
    let record = Record::of(&[share("rare", Better::Higher, (0, 4), (4, 4)), share("absent", Better::Lower, (1, 10), (0, 0))], 1);
    assert!(record.worsened().is_empty());
    assert_eq!(record.score(), "scoreboard: none of 0 compared numbers is worse than the week before");
}

#[test]
fn the_score_line_names_what_got_worse() {
    let record = Record::of(&[share("rework", Better::Lower, (30, 100), (20, 100)), share("first try", Better::Higher, (60, 100), (60, 100))], 1);
    assert_eq!(record.score(), "scoreboard: 1 of 2 compared numbers worse than the week before: rework");
}
