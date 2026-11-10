use super::*;

#[test]
fn a_host_whose_coverage_map_lags_shows_on_the_round_line() {
    let lander = "1111111111";
    let line = note(lander, &[Held { host: "gex45".to_owned(), commit: Some("2222222222".to_owned()) }, Held { host: "cpx62".to_owned(), commit: Some(lander.to_owned()) }]).unwrap();
    assert!(line.contains("gex45") && line.contains("2222222222") && line.contains("1111111111"), "the host that lags and both commits are named: {line}");
    assert!(!line.contains("cpx62"), "a host holding the lander's map is not named: {line}");
    assert!(note(lander, &[Held { host: "cpx62".to_owned(), commit: None }]).unwrap().contains("cpx62 holds no coverage map"), "a host whose graph holds no map is named too");
    assert_eq!(note(lander, &[Held { host: "cpx62".to_owned(), commit: Some(lander.to_owned()) }]), None, "every host holding the lander's map needs no line");
    assert_eq!(commit_of(r#"{"triple":["triple",["string","coverage:north"],["string","commit"],["string","abc123"]],"occurrences":1}"#).as_deref(), Some("abc123"), "the round line reads the commit from the graph's JSON");
}
