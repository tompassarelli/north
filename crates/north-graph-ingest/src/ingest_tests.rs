use crate::trusted_writers::refusal;

#[test]
fn a_fleet_session_cannot_tell_under_a_trusted_name() {
    let session = Some("0::/user.slice/north-session-7.scope/inner\n");
    let broker = Some("0::/user.slice/north-worker-outside-1.service\n");
    for writer in ["checker", "mirror", "lander", "git"] {
        assert!(refusal("tell", writer, session).is_some() && refusal("retract", writer, broker).is_some(), "{writer}");
    }
    assert!(refusal("tell", "checker", Some("0::/user.slice/north-lander.service\n")).is_none() && refusal("tell", "checker", None).is_some());
    let held = Some("0::/user.slice/user-1000.slice/user@1000.service/agent.slice/north-fleet-north-1547-d1752-graded-3730d742.service\n");
    assert!(refusal("tell", "checker", held).is_some() && refusal("retract", "lander", held).is_some(), "a held session's unit is a fleet session's scope");
    assert!(refusal("show", "checker", session).is_none() && refusal("tell", "selection-miss", session).is_none());
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("graph.db").display().to_string();
    let words = ["tell", "--db", &database, "--writer", "checker", "--file", "--writer", "S", "check-passed"].map(String::from);
    let code = north_graph::run_guarded(&words, &|verb, writer| refusal(verb, writer, session));
    assert_eq!(code, 1, "the writer the parser resolved is the one refused, since --file takes the later --writer as its value");
    assert!(!directory.path().join("graph.db").exists());
    let seen = std::cell::RefCell::new(Vec::new());
    let words = ["tell", "--db", &database, "--writer", "S", "--file", "--writer", "checker"].map(String::from);
    let code = north_graph::run_guarded(&words, &|verb, writer| {
        seen.borrow_mut().push(writer.to_owned());
        refusal(verb, writer, session)
    });
    assert_eq!(*seen.borrow(), ["S"], "the parser resolves S, so a reading of the raw words that refuses checker here is the wrong one");
    assert_eq!(code, 2, "the guard let it through and the write failed on its arguments");
}
