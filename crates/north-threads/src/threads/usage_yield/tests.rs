use super::week::Week;

fn week(resets: i64, readings: &[(i64, f64)]) -> Week {
    Week { resets, readings: readings.to_vec() }
}

/// Two usage sources read the same window a point apart (north#793): a lower readout after a higher one is not a drop and refill, so each flap is not spent again.
#[test]
fn a_lower_readout_from_another_source_is_not_spent_twice() {
    let week = week(1_000_000, &[(100, 90.0), (160, 91.0), (170, 90.0), (220, 91.0), (280, 93.0)]);
    assert_eq!(week.spent(150, 300), 3.0);
}
