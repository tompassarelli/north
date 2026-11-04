use super::VERB_KINDS;
use crate::cli::tools::{defines, verbs};
use crate::proxy::Kind;

#[test]
fn every_dispatched_verb_declares_a_kind() {
    assert_eq!(VERB_KINDS.undeclared(verbs()), Vec::<&str>::new());
}

#[test]
fn every_declared_verb_is_dispatched() {
    let dispatched: Vec<&str> = verbs().collect();
    assert_eq!(VERB_KINDS.strays(&dispatched), Vec::<&str>::new());
    assert!(dispatched.iter().all(|verb| defines(verb)));
}

#[test]
fn subcommands_override_their_verb() {
    assert_eq!(VERB_KINDS.kind(&["fleet", "launch"]), Some(Kind::Delegate));
    assert_eq!(VERB_KINDS.kind(&["fleet", "show"]), Some(Kind::Status));
    assert_eq!(VERB_KINDS.kind(&["land", "enqueue"]), Some(Kind::Landing));
    assert_eq!(VERB_KINDS.kind(&["land", "scan"]), Some(Kind::Diagnosis));
    assert_eq!(VERB_KINDS.kind(&[]), Some(Kind::Status));
}
