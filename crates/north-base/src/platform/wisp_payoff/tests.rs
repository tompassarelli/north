use super::*;
use serde_json::json;

#[test]
fn setup_failures_do_not_retire_checks_or_file_parity_fixes() {
    let rows = [
        ("PASS", 1.0, 1.0, Some(true)),
        ("FAIL", 1.0, 1.0, Some(false)),
        ("INVALID", 1.0, 1.0, None),
        ("PASS", 0.0, 1.0, None),
        ("FAIL", 1.0, 0.0, None),
    ];
    for (verdict, compare, headless, expected) in rows {
        let input = json!({"fresh_each":false,"hot":false,"pairs":["lan-0"],"scripts":[{"pair":"lan-0","script":"test/native/pads/448/a.pad", "verdict":verdict, "compare":compare,"headless":headless,"game":30,"run":60,"reset":15,"reload":15,"waited":900}]});
        let runs = evidence::decode(Path::new("batch.json"), &input, 100);
        assert_eq!(runs[0].agreement, expected);
        assert_eq!(runs[0].client_minutes, 4.0);
        assert_eq!(runs[0].kind, "pad/448");
        let mut unmarked = input.clone();
        unmarked.as_object_mut().unwrap().remove("fresh_each");
        assert!(evidence::decode(Path::new("report.json"), &unmarked, 100).is_empty());
        let mut headless = input;
        headless["headless_session"] = json!(true);
        assert!(evidence::decode(Path::new("batch.json"), &headless, 100).is_empty());
    }
}

#[test]
fn a_recent_disagreement_blocks_retirement_even_outside_the_display_filter() {
    let now = 1_792_100_000;
    let make = |at, agreement| Run {
        path: "batch.json".into(),
        kind: "pad/448".into(),
        script: Some("test/native/pads/448/a.pad".into()),
        at,
        client_minutes: 1.0,
        estimated: false,
        agreement,
        invalid: false,
    };
    let agreeing: Vec<_> = (0..10).map(|i| make(now - i, Some(true))).collect();
    assert_eq!(summarize(agreeing.clone(), None, now).retire, ["pad/448"]);
    let mut runs = agreeing.clone();
    runs.push(make(now - 86400 * 13, Some(false)));
    let since = Some(OffsetDateTime::from_unix_timestamp(now).unwrap().date());
    assert!(summarize(runs, since, now).retire.is_empty());
    let mut runs = agreeing;
    runs.push(make(now - 86400 * 15, Some(false)));
    let report = summarize(runs, None, now);
    assert_eq!(report.retire, ["pad/448"]);
    assert_eq!(report.families[0].verdict, "wisp-only");
    assert_eq!(report.families[0].match_rule.kind, "parent-directory");
    assert_eq!(
        report.families[0].match_rule.values,
        ["test/native/pads/448"]
    );
    assert_eq!(report.families[0].native_sample_rate, 0.1);
    assert_eq!(report.sampler.example_bucket, 3);
    assert_eq!(report.fixes.len(), 1);
}

#[test]
fn allocating_accept_report_time_preserves_total_and_never_implies_parity() {
    let input = json!({"started":1_791_000_000_000_i64,"finished":1_791_000_120_000_i64,"results":[{"id":"a","verdict":"pass"},{"id":"b","verdict":"fail"},{"id":"never-ran","verdict":"skipped"}]});
    let runs = evidence::decode(Path::new("report.json"), &input, 0);
    assert_eq!(runs.len(), 2);
    assert_eq!(runs.iter().map(|r| r.client_minutes).sum::<f64>(), 4.0);
    assert!(runs.iter().all(|r| r.agreement.is_none() && r.estimated));
    let report = summarize(runs, None, 1_791_000_130);
    assert_eq!(report.weeks.len(), 1);
    assert!(report.weeks[0].wisp_only_share.is_none());
    assert!(report.retire.is_empty());
    assert!(report.fixes.is_empty());
}
