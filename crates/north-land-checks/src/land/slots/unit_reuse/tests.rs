use north_base::verdict_cache::reuse_log::Record;

#[test]
fn a_logged_record_parses_back_to_itself() {
    let unit = Record::Unit { probe: "abc".into(), unit: "north-base a::b".into(), set: Some("ff00".into()), passed: false, seconds: 1.5 };
    let differs = Record::SetDiffers { probe: "abc".into(), unit: "north-base a::b".into(), reused: "ff00".into(), real: "ee11".into(), path: "content harness/x".into() };
    assert_eq!(Record::parse(&unit.render()), Some(unit));
    assert_eq!(Record::parse(&differs.render()), Some(differs));
}
