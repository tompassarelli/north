use super::*;

#[test]
fn timestamps_round_trip_iso() {
    let time = Timestamp::parse("2026-10-15T15:20:00Z").unwrap();
    assert_eq!(time.iso(), "2026-10-15T15:20:00Z");
    assert_eq!(Timestamp::parse("2026-10-15T17:20:00.5+02:00").unwrap(), time);
    assert_eq!(Timestamp::parse("2024-02-29T00:00:00Z").unwrap().iso(), "2024-02-29T00:00:00Z");
    assert_eq!(Timestamp::parse("1970-01-01T00:00:00Z").unwrap(), Timestamp::from_unix_seconds(0));
    for bad in ["2026-10-15", "2026-02-30T00:00:00Z", "2026-10-15T15:20:00", "ACK 7751 15:20", "2026-10-15T15:20:00+0200"] {
        assert!(Timestamp::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_local_day_starts_at_local_midnight() {
    assert_eq!([offset_seconds("+0800"), offset_seconds("-0530"), offset_seconds("Z")], [Some(28_800), Some(-19_800), None]);
    let now = Timestamp::now();
    let start = now.local_day_start_ms();
    let now_ms = u64::try_from(now.unix_millis()).unwrap();
    assert!(start <= now_ms && now_ms - start < 86_400_000);
    assert_eq!((i64::try_from(start).unwrap() / 1000 + local_offset_seconds()).rem_euclid(86_400), 0);
}
