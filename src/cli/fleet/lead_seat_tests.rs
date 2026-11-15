use super::seat_words;

#[test]
fn a_launch_onto_the_lead_seat_from_any_caller_but_the_operator_is_refused_and_the_operators_keeps_its_parent() {
    let words = ["--name", "gates-lead", "--role", "lead", "--cwd", "nexus:/home/tom/code/north/main", "brief"];
    let refused = seat_words("nexus", &words, 4, "/home/tom/code/north/main", false, Some("a-worker")).unwrap_err();
    assert!(refused.starts_with("--cwd nexus:/home/tom/code/north/main: only the operator seats a lead on nexus"), "{refused}");
    let seated = seat_words("nexus", &words, 4, "/home/tom/code/north/main", true, Some("proxy")).unwrap();
    assert_eq!(seated, ["--parent", "proxy", "--name", "gates-lead", "--role", "lead", "--cwd", "/home/tom/code/north/main", "brief"]);
}
