use super::*;
use crate::foundation::clock::Timestamp;
use crate::threads::fake::Recorded;

fn ledger(directory: &std::path::Path) -> std::path::PathBuf {
    let database = directory.join("threads.db");
    Connection::open(&database).unwrap();
    database
}

fn call(verb: crate::threads::verb::Verb, database: &std::path::Path, replica: bool, argv: &[&str]) -> (i32, String, String) {
    let outside = Recorded::default();
    let context = Context { database, actor: "tester".into(), no_native: None, outside: &outside, now: Timestamp::parse("2026-11-05T10:00:00Z").unwrap(), replica };
    let argv: Vec<String> = argv.iter().map(|word| (*word).to_owned()).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = verb(&argv, &context, &mut std::io::empty(), &mut out, &mut err);
    (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
}

fn finding(database: &std::path::Path, [review, line, kind, lens, tier, severity, verdict]: [&str; 7]) -> (i32, String, String) {
    call(run, database, false, &["review-finding", "record", "--review", review, "--plan", "docs/rationale/proxy-role.md", "--line", line, "--kind", kind, "--lens", lens, "--tier", tier, "--severity", severity, "--verdict", verdict, "--finding", &format!("finding at {line}")])
}

#[test]
fn a_reviews_findings_are_recorded_with_kind_lens_tier_severity_and_verdict_and_read_back_per_kind() {
    let directory = tempfile::tempdir().unwrap();
    let database = ledger(directory.path());
    let recorded = [
        ["r1", "217", "fact", "", "here", "must-fix", "accepted"],
        ["r1", "13", "scenario", "", "sonnet-high", "must-fix", "accepted"],
        ["r1", "19", "scenario", "", "sonnet-high", "nit", "rejected"],
        ["r1", "15", "architecture", "simpler-alternative", "opus-high", "should-fix", "accepted"],
        ["r2", "23", "scenario", "", "opus-medium", "must-fix", "rejected"],
    ];
    for row in recorded {
        let (code, out, err) = finding(&database, row);
        assert_eq!((code, err.as_str()), (0, ""), "{out}");
    }
    let (code, out, _) = finding(&database, ["r1", "13", "scenario", "", "sonnet-high", "must-fix", "accepted"]);
    assert_eq!((code, out.as_str()), (0, "review r1: scenario finding at L13 already recorded\n"));
    let db = Connection::open(&database).unwrap();
    let row: (String, String, String, String, String, i64) = db
        .query_row("SELECT kind, lens, tier, severity, verdict, line FROM review_findings WHERE review = 'r1' AND kind = 'architecture'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
        .unwrap();
    assert_eq!(row, ("architecture".into(), "simpler-alternative".into(), "opus-high".into(), "should-fix".into(), "accepted".into(), 15));
    assert_eq!(db.query_row("SELECT count(*) FROM review_findings", [], |row| row.get::<_, i64>(0)).unwrap(), 5);
    let (code, out, err) = call(crate::threads::calibration::run, &database, true, &["recommend", "Category: review"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(
        out,
        "review findings by kind (kind, reviewer tier and lens: reviews, findings, accepted, rejected, accepted must-fix)\n\
         fact: findings 1  accepted 1  rejected 0  must-fix accepted 1\n\
         \x20 here                                 reviews 1  findings 1  accepted 1  rejected 0  must-fix accepted 1\n\
         scenario: findings 3  accepted 1  rejected 2  must-fix accepted 1\n\
         \x20 opus-medium                          reviews 1  findings 1  accepted 0  rejected 1  must-fix accepted 0\n\
         \x20 sonnet-high                          reviews 1  findings 2  accepted 1  rejected 1  must-fix accepted 1\n\
         architecture: findings 1  accepted 1  rejected 0  must-fix accepted 0\n\
         \x20 opus-high lens simpler-alternative   reviews 1  findings 1  accepted 1  rejected 0  must-fix accepted 0\n"
    );
    let (_, later, _) = call(crate::threads::calibration::run, &database, true, &["recommend", "--since", "2026-11-06", "Category: review"]);
    assert_eq!(later, render(&[]));
}

#[test]
fn a_finding_outside_the_kinds_severities_or_verdicts_or_on_a_replica_is_refused_and_records_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let database = ledger(directory.path());
    for (kind, severity, verdict) in [("style", "nit", "accepted"), ("fact", "major", "accepted"), ("fact", "nit", "maybe")] {
        let (code, _, err) = finding(&database, ["r1", "1", kind, "", "here", severity, verdict]);
        assert_eq!(code, 2, "{err}");
    }
    let (code, _, err) = call(run, &database, true, &["review-finding", "record", "--review", "r1", "--kind", "fact", "--tier", "here", "--severity", "nit", "--verdict", "accepted"]);
    assert_eq!(code, 1, "{err}");
    let (code, _, _) = call(run, &database, false, &["review-finding", "record", "--kind", "fact", "--tier", "here", "--severity", "nit", "--verdict", "accepted"]);
    assert_eq!(code, 2);
    let db = Connection::open(&database).unwrap();
    assert!(!has_table(&db).unwrap());
    let (code, out, _) = call(crate::threads::calibration::run, &database, true, &["recommend", "Category: review"]);
    assert_eq!((code, out.as_str()), (0, "review findings: none recorded; record each with north threads review-finding record\n"));
}
