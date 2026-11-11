use super::Outcome;
use super::freeze::{frozen, step};
use super::issues::IssueRef;

#[test]
fn the_spend_freeze_of_2026_11_08_tells_the_proxy_once_on_the_first_dispatch_run_fifteen_minutes_in_naming_the_guard() {
    let guard = "burn governor: claude:b would reach 104% of its five-hour window by its reset in 1.0 h".to_owned();
    let item = IssueRef { repo: "north".into(), number: 838 };
    let outcome = Outcome { ready: vec![item.clone()], free: 8, skipped: vec![(item, format!("ready, not placed: nexus holds the box: {guard}"))], ..Outcome::default() };
    let basis = frozen(&outcome, &[], std::slice::from_ref(&guard));
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
    assert!(told[0].1.contains("15 min") && told[0].1.contains("refusing guards: burn governor") && told[0].1.contains("8 free slots"), "{}", told[0].1);
    let launched = Outcome { launches: Vec::new(), ready: Vec::new(), ..Outcome::default() };
    assert_eq!(step(stretch, frozen(&launched, &[], &[]), start + 30.0 * 60.0, 15.0), (None, None));

    let mut alerts = Vec::new();
    let delivery = super::effects::deliver("dispatch alert", &mut |_| Err("no token".to_owned()), &mut |line| {
        alerts.push(line.to_owned());
        Ok(())
    });
    assert_eq!(delivery, Ok(super::effects::Delivery::Page), "a failed nexus-alert is itself a page");
    assert_eq!(alerts, ["dispatch alert"]);
    assert!(super::effects::deliver("dispatch alert", &mut |_| Err("no token".to_owned()), &mut |_| Err("no proxy".to_owned())).is_err());
}

#[test]
fn a_failed_nexus_alert_pages_the_proxy_and_still_makes_the_dispatch_run_exit_non_zero() {
    let line = format!("north fleet dispatch on whiterabbit: the failed alert sink of test {} is a page", std::process::id());
    let mut pages = Vec::new();
    let told = super::effects::tell_tom_with(&line, false, 1_700_000_000.0, &mut |_| Err("exited 1".to_owned()), &mut |paged| {
        pages.push(paged.to_owned());
        Ok(())
    }, &mut |_, _| {});
    assert_eq!(pages, std::slice::from_ref(&line), "a failed nexus-alert is itself a page carrying the whole line");
    assert!(told.is_err(), "a delivered page still returns Err, so the run does not look successful");
    assert_eq!(super::exit(told, true, true), 1, "a dispatch run whose alert sink failed exits non-zero");
    assert_eq!(super::exit(Ok(()), true, true), 0, "a run whose alerts all went out exits zero");
    assert_eq!(super::exit(Ok(()), false, true), 1, "a failed launch page fails the run");
    assert_eq!(super::exit(Ok(()), true, false), 1, "a failed freeze page fails the run");
    let both = super::effects::tell_tom_with(&line, false, 1_700_000_000.0, &mut |_| Err("no token".to_owned()), &mut |_| Err("no proxy".to_owned()), &mut |_, _| {});
    let why = both.expect_err("both sinks failed, so telling Tom failed");
    assert!(why.contains("no token") && why.contains("no proxy"), "{why}");
}

#[test]
fn three_launches_failing_with_the_same_cause_within_ten_minutes_page_once() {
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
    let start = 1_794_077_716.0;
    let capacity = "error: launch refused: no capacity on nexus";
    let spread = "error: claude exited before its first turn";
    let failures = [
        failed(start + 60.0, "north-1-d0101", capacity),
        failed(start + 120.0, "north-1-d0102", capacity),
        failed(start + 150.0, "north-1-d0103", spread),
        failed(start + 180.0, "north-1-d0104", capacity),
        failed(start + 420.0, "north-1-d0105", capacity),
        failed(start + 840.0, "north-1-d0106", spread),
        failed(start + 1200.0, "north-1-d0107", spread),
    ];
    let store = tempfile::tempdir().unwrap();
    let dispatch = north_fleet_store::sender::Sender::Service("north-dispatch".into());
    let mut delivered = Vec::new();
    for run in 0..=10 {
        let at = start + f64::from(run) * 120.0;
        let seen: Vec<_> = failures.iter().filter(|record| record.at <= at).cloned().collect();
        let pages = super::record::failure_pages(&seen, at);
        assert!(super::effects::watch_failures(&pages, &mut |key, line| {
            let condition = north_fleet_store::fold::Condition { key: key.to_owned(), cleared: false };
            let admitted = north_fleet_store::fold::admit(store.path(), &dispatch, north_fleet_store::runtime::PROXY, line, Some(&condition), (at * 1000.0) as u64);
            if admitted == north_fleet_store::fold::Admit::Send {
                delivered.push(line.to_owned());
            }
            !matches!(admitted, north_fleet_store::fold::Admit::Drop)
        }));
    }
    assert_eq!(delivered.len(), 1, "one page for the cause however many dispatch runs still see it, none for a cause failing three times over more than ten minutes: {delivered:?}");
    assert!(delivered[0].contains("3 launches failed in 10 min") && delivered[0].contains(capacity), "{}", delivered[0]);
    assert!(delivered[0].contains(&format!("first stderr: {}", super::record::stderr_path("north-1-d0101").display())), "{}", delivered[0]);
}
