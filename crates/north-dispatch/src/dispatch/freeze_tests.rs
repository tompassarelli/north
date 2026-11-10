use super::Outcome;
use super::freeze::{frozen, step};
use super::issues::IssueRef;
use std::collections::BTreeMap;

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

    let failed = |at: f64, name: &str, detail: &str| super::record::Record {
        at,
        item: "north#1".into(),
        host: "nexus".into(),
        account: "claude:a".into(),
        name: name.into(),
        cloud: false,
        launched: false,
        detail: detail.into(),
        tier: None,
        category: None,
        box_text: None,
        hand: false,
        effort_arm: None,
        signal: None,
        lift: None,
        effort: None,
        cell: None,
        cell_role: None,
    };
    let records = vec![
        failed(start + 60.0, "north-1-d0101", "error: launch refused: no capacity on nexus"),
        failed(start + 120.0, "north-1-d0102", "error: launch refused: no capacity on nexus"),
        failed(start + 180.0, "north-1-d0103", "error: launch refused: no capacity on nexus"),
    ];
    let pages = super::record::failure_pages(&records, start + 240.0);
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].count, 3);
    assert_eq!(pages[0].cause, "error: launch refused: no capacity on nexus");
    assert!(pages[0].stderr.ends_with("north-1-d0101.log"), "{}", pages[0].stderr);
    assert!(pages[0].line().contains("first stderr:"));
    let mut told = BTreeMap::new();
    let mut paged = Vec::new();
    assert!(super::effects::watch_failures(&pages, start + 240.0, &mut told, &mut |line| {
        paged.push(line.to_owned());
        true
    }));
    assert_eq!(paged, [pages[0].line()]);
    assert!(super::effects::watch_failures(&pages, start + 300.0, &mut told, &mut |line| {
        paged.push(line.to_owned());
        true
    }));
    assert_eq!(paged.len(), 1, "one page per cause in ten minutes");

    let mut alerts = Vec::new();
    let delivery = super::effects::deliver("dispatch alert", &mut |_| Err("no token".to_owned()), &mut |line| {
        alerts.push(line.to_owned());
        Ok(())
    });
    assert_eq!(delivery, Ok(super::effects::Delivery::Page), "a failed nexus-alert is itself a page");
    assert_eq!(alerts, ["dispatch alert"]);
    assert!(super::effects::deliver("dispatch alert", &mut |_| Err("no token".to_owned()), &mut |_| Err("no proxy".to_owned())).is_err());
}
