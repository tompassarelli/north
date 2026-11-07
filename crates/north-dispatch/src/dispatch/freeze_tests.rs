use super::Outcome;
use super::freeze::{frozen, step};
use super::issues::IssueRef;

#[test]
fn the_tripwire_freeze_of_2026_11_08_tells_the_proxy_once_on_the_first_dispatch_run_fifteen_minutes_in_naming_the_tripwire() {
    let tripwire = "spend tripwire: north-bars-lead spent 18 points of claude:b's five-hour window in 10 min, over the 10-point session tripwire on b".to_owned();
    let item = IssueRef { repo: "north".into(), number: 838 };
    let outcome = Outcome { ready: vec![item.clone()], free: 8, skipped: vec![(item, format!("ready, not placed: nexus holds the box: {tripwire}"))], ..Outcome::default() };
    let basis = frozen(&outcome, &[], std::slice::from_ref(&tripwire));
    let start = 1_794_077_716.0;
    let mut stretch = None;
    let mut told = Vec::new();
    for minute in [0.0, 5.0, 10.0, 15.0, 20.0, 25.0] {
        let (next, text) = step(stretch, basis.clone(), start + minute * 60.0, 15.0);
        stretch = next;
        told.extend(text.map(|text| (minute, text)));
    }
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].0, 15.0);
    assert!(told[0].1.contains("15 min") && told[0].1.contains("refusing guards: spend tripwire") && told[0].1.contains("8 free slots"), "{}", told[0].1);
    let launched = Outcome { launches: Vec::new(), ready: Vec::new(), ..Outcome::default() };
    assert_eq!(step(stretch, frozen(&launched, &[], &[]), start + 30.0 * 60.0, 15.0), (None, None));
}
