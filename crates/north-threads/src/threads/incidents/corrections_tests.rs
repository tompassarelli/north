use super::*;

const DAY: f64 = 24.0 * 3600.0;

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

#[test]
fn a_guarded_class_that_recurs_inside_the_week_is_red_and_one_before_its_guard_is_not() {
    let table = "[[class]]\nname = \"pace\"\nparaphrase = \"Tom said the work was moving too slowly.\"\nowner = \"north#636\"\nguarded_at = \"2026-11-06T00:00:00Z\"\n\n[[class]]\nname = \"overbuilt\"\nparaphrase = \"Tom said it was heavier than needed.\"\n";
    let classes = parse(table).unwrap();
    let guarded = Timestamp::parse_loose("2026-11-06T00:00:00Z").unwrap().unix_seconds_f64();
    let now = guarded + DAY;
    let before = vec![("pace".to_owned(), guarded - 60.0), ("overbuilt".to_owned(), guarded + 60.0)];
    assert!(recurrences(&before, &classes, now).is_empty());
    assert_eq!(red(&[]), (String::new(), String::new()));
    let after = [before, vec![("pace".to_owned(), guarded + 3600.0)]].concat();
    let recurred = recurrences(&after, &classes, now);
    assert_eq!(recurred, vec![Recurrence { class: "pace".into(), count: 1, owner: Some("north#636".into()) }]);
    let (set, text) = red(&recurred);
    assert_eq!(set, "pace:1");
    assert_eq!(text, "corrections red (1): pace recurred 1 time since its guard landed (owner north#636)");
    assert!(recurrences(&after, &classes, guarded + 9.0 * DAY).is_empty());
}
