use super::tom_class_refusal;

// KEEP(mutant): an agent asking for the tom class is refused and told which class to use; mutant tom-class-any-owner (a8eed22a5, north#460)
#[test]
fn an_agent_asking_for_the_tom_class_is_refused_and_told_which_class_to_use() {
    for owner in [Some("wisp-fx-style"), Some("perf-scale-185"), Some("land-check"), None] {
        let refusal = tom_class_refusal(owner).unwrap_or_else(|| panic!("{owner:?} admitted to the tom class"));
        assert!(refusal.contains("heavy") && refusal.contains("--priority tom-ask"), "{refusal}");
    }
}
