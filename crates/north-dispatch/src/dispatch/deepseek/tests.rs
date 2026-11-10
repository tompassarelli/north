use super::review::{self, Done};
use super::sampling::{self, FULL_REVIEWS, Kind, Verdict};
use super::{Seen, Settings, key};
use crate::dispatch::decide;
use crate::dispatch::issues::IssueRef;
use crate::dispatch::plan::Cloud;
use crate::dispatch::record::Record;
use crate::dispatch::tests::{MAX_5X, host, issue, sized, survey};
use crate::dispatch::slots::Room;
use serde_json::json;
use std::collections::BTreeSet;

fn settings() -> Settings {
    let document: toml_edit::DocumentMut = "[dispatch]\nhosts = [\"whiterabbit\"]\nslot_cap = 4\nclaude_only = ['\\bsandbox', '\\block(s|ed|ing)?\\b']\n".parse().unwrap();
    Settings::parse(&document)
}

#[test]
fn a_deepseek_box_is_staffed_sampled_for_review_and_queued_from_its_done_report_without_the_proxy() {
    active_workers_counts_claude_workers_alone();
    a_category_is_reviewed_in_full_until_ten_clean_then_one_in_five_while_a_defect_resets_it_and_landing_boxes_are_always_reviewed();
    an_idle_deepseek_session_whose_last_report_is_done_queues_one_review_job_and_no_other_session_does();
    let hosts = vec![host("nexus", 6, Ok(0)), host("whiterabbit", 8, Ok(0))];
    let accounts = vec![sized("claude:a", &["nexus"], MAX_5X, Room { five_hour: 80.0, week: 80.0, week_reset_h: 100.0 })];
    let boxes = vec![
        issue("north", 1, "- [ ] add a column", &[]),
        issue("north", 2, "- [ ] narrow the sandbox reads", &[]),
        issue("north", 3, "- [ ] take the session lock first", &[]),
        issue("north", 4, "- [ ] retry it", &[]),
        issue("north", 5, "- [ ] another column", &[]),
        issue("north", 6, "- [ ] a third column", &[]),
    ];
    let mut planned = survey(boxes, Cloud::Closed("no credit".into()), hosts, accounts);
    planned.deepseek = Seen { settings: settings(), tried: BTreeSet::from([key(&IssueRef::new("north", 4), Some("retry it"))]), done: Vec::new(), running: 0 };

    let outcome = decide(&planned, "test");

    let placed: Vec<(String, &str, &str)> = outcome.launches.iter().map(|launch| (launch.item.to_string(), launch.account.as_str(), launch.host.as_str())).collect();
    let on = |item: &str, account: &'static str, host: &'static str| (item.to_owned(), account, host);
    assert_eq!(
        placed,
        [
            on("north#1", "opencode:go", "whiterabbit"),
            on("north#2", "claude:a", "nexus"),
            on("north#3", "claude:a", "nexus"),
            on("north#4", "claude:a", "nexus"),
            on("north#5", "opencode:go", "whiterabbit"),
            on("north#6", "opencode:go", "whiterabbit"),
        ],
        "DeepSeek takes every box that may leave Claude and it has not tried; sandbox and locking boxes go to Claude"
    );
    assert_eq!(super::tried(&[Record { at: 0.0, item: "north#5".into(), host: "whiterabbit".into(), account: "opencode:go".into(), name: "north-5-d0101".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: Some("another column".into()), hand: false, effort_arm: None, signal: None, lift: None, effort: None, cell: None, cell_role: None }]), BTreeSet::from([key(&IssueRef::new("north", 5), Some("another column"))]), "a DeepSeek launch's record makes its box tried");
}

fn verdict(category: &str, kind: Kind, sha: &str) -> Verdict {
    Verdict { at: 0.0, session: "ds".into(), category: category.into(), sha: sha.into(), kind, class: None, effort: None }
}

fn a_category_is_reviewed_in_full_until_ten_clean_then_one_in_five_while_a_defect_resets_it_and_landing_boxes_are_always_reviewed() {
    let unsampled = "00000001";
    let sampled = "00000005";
    let mut verdicts: Vec<Verdict> = Vec::new();
    for _ in 0..FULL_REVIEWS {
        assert!(sampling::reviews(&verdicts, "feature", false, unsampled), "a category under 10 clean reviews is reviewed in full");
        verdicts.push(verdict("feature", Kind::Clean, unsampled));
    }
    verdicts.push(verdict("tooling", Kind::Defect, unsampled));
    assert!(!sampling::reviews(&verdicts, "feature", false, unsampled), "after 10 clean, an unsampled sha goes straight to the lander");
    assert!(sampling::reviews(&verdicts, "feature", false, sampled), "after 10 clean, one sha in 5 is still reviewed");
    assert!(sampling::reviews(&verdicts, "feature", true, unsampled), "a landing box is always reviewed");
    let shown = (0..1000u32).filter(|number| sampling::reviews(&verdicts, "feature", false, &format!("{number:08x}"))).count();
    assert_eq!(shown, 200, "one sha in five is sampled");

    verdicts.push(verdict("feature", Kind::Defect, unsampled));
    assert!(sampling::reviews(&verdicts, "feature", false, unsampled), "a defect puts the category back on full review");
    verdicts.extend((0..FULL_REVIEWS).map(|_| verdict("feature", Kind::Clean, unsampled)));
    verdicts.push(verdict("feature", Kind::Refused, unsampled));
    assert!(sampling::reviews(&verdicts, "feature", false, unsampled), "the lander refusing an unreviewed commit resets it too");
}

fn an_idle_deepseek_session_whose_last_report_is_done_queues_one_review_job_and_no_other_session_does() {
    let session = |name: &str, provider: &str, status: &str| json!({"name": name, "provider": provider, "status": status, "tasks": ["north#7"], "box": "the box"});
    let lists = vec![("whiterabbit".to_owned(), json!([session("ds-done", "opencode", "idle"), session("ds-busy", "opencode", "working"), session("ds-stuck", "opencode", "idle"), session("cl-done", "claude", "idle"), session("ds-queued", "opencode", "idle")]))];
    let said = |text: &str| json!({"entries": [{"kind": "tool", "text": "Bash cargo test"}, {"kind": "assistant", "text": text}, {"kind": "turn_end", "text": "completed"}]});
    let reports = vec![("whiterabbit".to_owned(), json!({"ds-done": said("Done: the box landed in the lane."), "ds-busy": said("Done: earlier"), "ds-stuck": said("Not done: tests fail.\nStop: reasoning"), "cl-done": said("Done: x"), "ds-queued": said("Done: y")}))];
    let landing = issue("north", 7, "- [ ] the box", &["area:landing"]);
    let queued = BTreeSet::from(["ds-queued".to_owned()]);

    let found = review::done(&lists, &reports, &[], &[landing], &queued);

    assert_eq!(found, [Done { name: "ds-done".into(), host: "whiterabbit".into(), item: IssueRef::new("north", 7), box_text: Some("the box".into()), category: "feature".into(), landing: true }]);
    assert_eq!(review::job(&found[0]), ["fleet", "dispatch", "review", "ds-done", "--item", "north#7", "--category", "feature", "--box", "the box", "--landing"]);
}

fn active_workers_counts_claude_workers_alone() {
    let listing = json!([
        {"name": "w1", "status": "working", "provider": "claude", "account": "a", "role": "worker"},
        {"name": "w2", "status": "working", "provider": "claude", "account": "b", "role": "worker"},
        {"name": "proxy", "status": "working", "provider": "claude", "account": "c", "role": "proxy"},
        {"name": "ds-1", "status": "working", "provider": "opencode", "account": "go", "role": "worker"},
        {"name": "ds-2", "status": "working", "provider": "opencode", "account": "go", "role": "worker"},
        {"name": "ds-3", "status": "starting", "provider": "pi", "account": "go", "role": "worker"},
    ]);
    let sessions: Vec<_> = crate::dispatch::fleets::running_sessions(&listing).into_iter().map(|session| (session, "whiterabbit".to_owned())).collect();

    assert_eq!(crate::dispatch::fleets::active_workers(&sessions), 2, "two Claude workers are working while a proxy and three DeepSeek sessions are not Claude workers");
    assert_eq!(crate::dispatch::fleets::working_by_account(&sessions).get("opencode"), Some(&2));
}

#[test]
fn the_sonnet_reviewer_draws_medium_or_high_by_seed_and_records_it() {
    let drawn: BTreeSet<&str> = (0..40).map(|number| sampling::reviewer_effort(&format!("north-1020-d{number:04x}"))).collect();
    assert_eq!(drawn, BTreeSet::from(["medium", "high"]), "both efforts are drawn across sessions");
    assert_eq!(sampling::reviewer_effort("north-1020-d0001"), sampling::reviewer_effort("north-1020-d0001"), "a session draws the same effort every time");
    let session = "north-1020-d0001";
    let effort = sampling::reviewer_effort(session);
    let text = review::brief(session, "north#1020", "feature", None, std::path::Path::new("/lane"), "abc", effort);
    assert!(text.contains(&format!("--sha abc --effort {effort} clean")), "the brief tells the reviewer to record its drawn effort");
    let directory = std::env::temp_dir().join(format!("deepseek-effort-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let row = Verdict { effort: Some(effort.into()), ..verdict("feature", Kind::Clean, "abc") };
    sampling::append(&directory, &row).unwrap();
    assert_eq!(sampling::read(&directory), vec![row], "the verdict row keeps the effort");
    std::fs::remove_dir_all(&directory).unwrap();
}


#[test]
fn a_deepseek_run_whose_lane_the_broker_landed_before_its_report_is_recorded_landed() {
    let state = tempfile::tempdir().unwrap();
    let queues = state.path().join("north/land-queue");
    let done = queues.join("repo/done");
    std::fs::create_dir_all(&done).unwrap();
    std::fs::write(done.join("1-a.json"), r#"{"session":"ds-landed","branch":"lane","verdict":"landed abc123"}"#).unwrap();

    assert_eq!(review::no_commit_kind(&queues, "ds-landed"), Kind::Landed);
    assert_eq!(review::no_commit_kind(&queues, "ds-nothing"), Kind::Empty);
    let verdicts = [Verdict { at: 0.0, session: "ds-landed".into(), category: "feature".into(), sha: "abc".into(), kind: Kind::Landed, class: None, effort: None }];
    assert_eq!(sampling::clean_run(&verdicts, "feature"), 0);
}
