use super::*;

#[test]
fn every_class_in_the_harness_table_has_a_paraphrase_an_owner_and_a_guard() {
    let all = classes();
    assert!(all.len() >= 11);
    for class in all {
        assert!(class.paraphrase.starts_with("Tom "), "{} paraphrase names Tom as the subject", class.name);
        assert!(class.owner.as_deref().is_some_and(|owner| owner.contains('#')), "{} names an owning issue", class.name);
        assert!(class.guard.as_deref().is_some_and(|guard| !guard.is_empty()), "{} proposes a guard", class.name);
    }
    assert_eq!(class_of("you're working too slowly").map(|class| class.name.as_str()), Some("pace"));
    assert_eq!(class_of("is this deterministically enforced?").map(|class| class.name.as_str()), Some("manual-not-guarded"));
}
