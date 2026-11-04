use std::cell::RefCell;

use serde_json::json;

use super::sampler::sampler_alert;
use super::tests::{Fake, places};

const GOOD: f64 = 1_793_800_000.0;

#[test]
fn a_sampler_failing_past_ten_minutes_alerts_the_proxy_once_naming_the_provider_and_its_last_good_reading() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let pace = places.state.join("agents/pace-control.json");
    std::fs::create_dir_all(pace.parent().unwrap()).unwrap();
    let write = |claude: serde_json::Value| std::fs::write(&pace, json!({"providers": {"claude": claude, "gemini": {"code": 0, "sampled_at": GOOD}}}).to_string()).unwrap();
    let sends = RefCell::new(Vec::<String>::new());
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "list", "--json"] => Some("[]".into()),
        ["north", "fleet", "send", node, text] => {
            sends.borrow_mut().push(format!("{node}: {text}"));
            Some(String::new())
        }
        _ => None,
    };
    let host = Fake(&reply);
    write(json!({"code": 2, "throttled": false, "sampled_at": GOOD, "error": "north usage --rows claude: no such adapter"}));
    sampler_alert(&places, &host, GOOD + 540.0).unwrap();
    assert!(sends.borrow().is_empty(), "nine minutes is not yet stale");
    for minute in [10.0, 11.0, 30.0, 300.0] {
        sampler_alert(&places, &host, GOOD + minute * 60.0).unwrap();
    }
    assert_eq!(sends.borrow().len(), 1, "{:?}", sends.borrow());
    let text = sends.borrow()[0].clone();
    assert!(text.starts_with("proxy: Usage sampler for claude has not read for 10m (exit 2: north usage --rows claude: no such adapter); last good reading "), "{text}");
    assert!(!text.contains("gemini"), "{text}");
    write(json!({"code": 0, "throttled": false, "sampled_at": GOOD + 18_000.0}));
    sampler_alert(&places, &host, GOOD + 18_000.0).unwrap();
    write(json!({"code": 2, "throttled": false, "sampled_at": GOOD + 18_000.0}));
    sampler_alert(&places, &host, GOOD + 18_700.0).unwrap();
    assert_eq!(sends.borrow().len(), 2, "a good read ends the episode, so the next failure alerts again");
}

#[test]
fn a_sampler_with_no_good_reading_alerts_ten_minutes_after_flow_first_sees_it() {
    let directory = tempfile::tempdir().unwrap();
    let places = places(directory.path());
    let pace = places.state.join("agents/pace-control.json");
    std::fs::create_dir_all(pace.parent().unwrap()).unwrap();
    std::fs::write(&pace, json!({"providers": {"claude": {"code": 2, "interval_s": 30720.0, "sampled_at": null}}}).to_string()).unwrap();
    let sends = RefCell::new(Vec::<String>::new());
    let reply = |argv: &[&str]| match argv {
        ["north", "fleet", "send", _, text] => {
            sends.borrow_mut().push((*text).to_owned());
            Some(String::new())
        }
        _ => None,
    };
    let host = Fake(&reply);
    sampler_alert(&places, &host, GOOD).unwrap();
    sampler_alert(&places, &host, GOOD + 600.0).unwrap();
    assert_eq!(sends.borrow().len(), 1);
    assert!(sends.borrow()[0].contains("(exit 2); last good reading none recorded"), "{}", sends.borrow()[0]);
}
