use super::*;

const BULLETS: &str = "  · --max-jobs N\n\n    Cap parallel builds.\n\n  · --no-update-lock-file\n\n    Never touch the lock.\n\n  · --offline\n\n    Use only what is local,\n    fetch nothing.\n\n  · --option name value\n\n    Set a setting.\n";
const MAN: &str = "OPTIONS\n       --no-update-lock-file\n           Never touch the lock.\n\n       --offline\n           Use only what is local.\n       --option name value\n           Set a setting.\n";

fn host(text: &'static str) -> impl Fn(&str) -> Option<String> {
    move |subcommand| (subcommand == "develop").then(|| text.to_owned())
}

#[test]
fn develop_prints_the_offline_card_and_both_option_paragraphs() {
    for text in [BULLETS, MAN] {
        let out = explain("develop", &host(text)).unwrap();
        let n5 = cards().into_iter().find(|card| card.class == "N5").unwrap();
        assert!(out.starts_with(&n5.render()), "{out}");
        assert!(out.contains("--offline") && out.contains("--no-update-lock-file"), "{out}");
        assert!(out.contains("Never touch the lock."), "{out}");
        assert!(out.contains("Use only what is local"), "{out}");
        assert!(!out.contains("Set a setting."), "{out}");
    }
}

#[test]
fn a_bullet_section_keeps_its_wrapped_body() {
    let found = section(BULLETS, "--offline").unwrap();
    assert!(found.contains("fetch nothing."), "{found}");
    assert!(!found.contains("--option"), "{found}");
}

#[test]
fn class_and_incident_names_find_the_same_card() {
    let by_class = explain("n5", &host(BULLETS)).unwrap();
    let by_incident = explain("nix-offline", &host(BULLETS)).unwrap();
    assert_eq!(by_class, explain("develop", &host(BULLETS)).unwrap());
    assert_eq!(by_class, by_incident);
}

#[test]
fn a_class_without_options_prints_its_card_alone() {
    let out = explain("N1", &|_| panic!("no manual lookup")).unwrap();
    assert!(out.starts_with("nix-activity: N1 "), "{out}");
    assert!(!out.contains("--help on this host"), "{out}");
}

#[test]
fn a_missing_manual_is_said_plainly() {
    let out = explain("develop", &|_| None).unwrap();
    assert!(out.contains("printed nothing on this host"), "{out}");
}

#[test]
fn an_unknown_target_lists_what_exists() {
    let error = explain("frobnicate", &|_| None).unwrap_err();
    assert!(error.contains("classes: N1 N2") && error.contains("subcommands:") && error.contains("develop"), "{error}");
}

#[test]
fn only_the_explain_verb_is_accepted() {
    assert!(run(&["explain".into()]).is_err());
    assert!(run(&["gc".into(), "x".into()]).is_err());
}
