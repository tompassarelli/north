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
    planned.deepseek = Seen { settings: settings(), tried: BTreeSet::from([key(&IssueRef::new("north", 4), Some("retry it"))]), review: super::review::Found::default(), running: 0 };

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
    assert_eq!(super::tried(&[Record { at: 0.0, item: "north#5".into(), host: "whiterabbit".into(), account: "opencode:go".into(), name: "north-5-d0101".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: Some("another column".into()), hand: false, effort_arm: None, signal: None, lift: None, effort: None, cell: None, cell_role: None, route: None }]), BTreeSet::from([key(&IssueRef::new("north", 5), Some("another column"))]), "a DeepSeek launch's record makes its box tried");
}

fn verdict(category: &str, kind: Kind, sha: &str) -> Verdict {
    Verdict { at: 0.0, session: "ds".into(), category: category.into(), sha: sha.into(), kind, class: None, effort: None }
}

#[test]
fn opencode_landings_get_full_review_for_ten_runs_then_one_in_five_and_a_defect_resets_it() {
    let shas: Vec<String> = (0..12u32).map(|number| format!("{:08x}", number * 7 + 1)).collect();
    let mut verdicts: Vec<Verdict> = Vec::new();
    let mut requested = Vec::new();
    for sha in &shas {
        let asked = sampling::reviews(&verdicts, "feature", false, false, sha);
        requested.push(asked);
        verdicts.push(verdict("feature", if asked { Kind::Clean } else { Kind::Unsampled }, sha));
    }
    assert!(requested[..FULL_REVIEWS].iter().all(|asked| *asked), "the first 10 runs of a category are all reviewed: {requested:?}");
    let sampled = |sha: &String| u64::from_str_radix(sha, 16).unwrap().is_multiple_of(sampling::SAMPLE_EVERY);
    assert!(requested[FULL_REVIEWS..].iter().zip(&shas[FULL_REVIEWS..]).all(|(asked, sha)| *asked == sampled(sha)), "after 10, a run is reviewed only when its sha is sampled: {requested:?}");
    let unsampled = "00000001";
    assert!(!sampling::reviews(&verdicts, "feature", false, false, unsampled));
    let mut defect = verdict("feature", Kind::Defect, unsampled);
    defect.class = Some("out-of-scope".into());
    verdicts.push(defect);
    assert!(sampling::reviews(&verdicts, "feature", false, false, unsampled), "a defect finding puts the category back on full review");
}

fn a_category_is_reviewed_in_full_until_ten_clean_then_one_in_five_while_a_defect_resets_it_and_landing_boxes_are_always_reviewed() {
    let unsampled = "00000001";
    let sampled = "00000005";
    let mut verdicts: Vec<Verdict> = Vec::new();
    for _ in 0..FULL_REVIEWS {
        assert!(sampling::reviews(&verdicts, "feature", false, false, unsampled), "a category under 10 clean reviews is reviewed in full");
        verdicts.push(verdict("feature", Kind::Clean, unsampled));
    }
    verdicts.push(verdict("tooling", Kind::Defect, unsampled));
    assert!(!sampling::reviews(&verdicts, "feature", false, false, unsampled), "after 10 clean, an unsampled sha goes straight to the lander");
    assert!(sampling::reviews(&verdicts, "feature", false, false, sampled), "after 10 clean, one sha in 5 is still reviewed");
    assert!(sampling::reviews(&verdicts, "feature", true, false, unsampled), "a landing box is always reviewed");
    let shown = (0..1000u32).filter(|number| sampling::reviews(&verdicts, "feature", false, false, &format!("{number:08x}"))).count();
    assert_eq!(shown, 200, "one sha in five is sampled");

    verdicts.push(verdict("feature", Kind::Defect, unsampled));
    assert!(sampling::reviews(&verdicts, "feature", false, false, unsampled), "a defect puts the category back on full review");
    verdicts.extend((0..FULL_REVIEWS).map(|_| verdict("feature", Kind::Clean, unsampled)));
    verdicts.push(verdict("feature", Kind::Refused, unsampled));
    assert!(sampling::reviews(&verdicts, "feature", false, false, unsampled), "the lander refusing an unreviewed commit resets it too");
}

fn an_idle_deepseek_session_whose_last_report_is_done_queues_one_review_job_and_no_other_session_does() {
    let session = |name: &str, provider: &str, status: &str| json!({"name": name, "provider": provider, "status": status, "tasks": ["north#7"], "box": "the box"});
    let lists = vec![("whiterabbit".to_owned(), json!([session("ds-done", "opencode", "idle"), session("ds-busy", "opencode", "working"), session("ds-stuck", "opencode", "idle"), session("cl-done", "claude", "idle"), session("ds-queued", "opencode", "idle"), session("ds-parked", "opencode", "parked")]))];
    let said = |text: &str| json!({"entries": [{"kind": "tool", "text": "Bash cargo test"}, {"kind": "assistant", "text": text}, {"kind": "turn_end", "text": "completed"}]});
    let reports = vec![("whiterabbit".to_owned(), json!({"ds-done": said("Done: the box landed in the lane."), "ds-busy": said("Done: earlier"), "ds-stuck": said("Not done: tests fail.\nStop: reasoning"), "cl-done": said("Done: x"), "ds-queued": said("Done: y"), "ds-parked": said("Done: z")}))];
    let landing = issue("north", 7, "- [ ] the box", &["area:landing"]);
    let queued = review::Marked { recent: BTreeSet::from(["ds-queued".to_owned()]), ever: BTreeSet::from(["ds-queued".to_owned()]) };

    let found = review::done(&lists, &reports, &[], &[landing], &queued).done;

    let resting = |name: &str| Done { name: name.into(), host: "whiterabbit".into(), item: IssueRef::new("north", 7), box_text: Some("the box".into()), category: "feature".into(), landing: true, oracle: Vec::new(), at_ms: 0 };
    assert_eq!(found, [resting("ds-done"), resting("ds-parked")]);
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
fn a_private_lane_opencode_launch_past_the_zero_retention_date_is_refused_and_a_public_one_admitted() {
    use crate::fleet::model::{Account, LaunchRequest};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtime::{Opening, Runtime};
    use north_providers::providers::opencode::harness::OPENCODE;
    use north_providers::providers::opencode::route;
    use north_providers::providers::opencode::{Deepseek, Visibility};
    use std::sync::Arc;

    let configured = crate::platform::harness::document("harness/opencode.toml")["go"]["zero_retention_until"]
        .as_str()
        .expect("harness/opencode.toml [go] zero_retention_until is a YYYY-MM-DD date");
    let until = route::date(configured).unwrap();
    let after = until.next_day().unwrap();

    let home = tempfile::tempdir().unwrap();
    let account = Account { provider: Deepseek::opencode().provider(), label: "go".into(), home: home.path().into(), token_file: Some(home.path().join("go.key")), api: None };
    let request = LaunchRequest {
        verification: false,
        gates_aside: false,
        name: "worker".into(),
        provider: Deepseek::opencode().provider(),
        account: "go".into(),
        cwd: home.path().into(),
        model: None,
        effort: None,
        prompt: "Item: north#1\nBox: add a.txt".into(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: PromptMode::North,
        idle_exit_minutes: None,
        parent: None,
        task_ref: None,
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: None,
        resume: None,
        compact_window: None,
        route: None,
    };
    let launch = |visibility: Visibility, today: time::Date| {
        Deepseek::reading(&OPENCODE, Arc::new(move |_| visibility), Arc::new(move || today)).host(&account, &request, Opening::Fresh(Some("ses_x")), "")
    };

    let private = launch(Visibility::Private, after);
    assert!(private.is_err_and(|error| error.user_message().contains("zero-retention agreement")), "a private lane after {configured} is refused");
    assert!(launch(Visibility::Public, after).is_ok(), "a public lane runs on the free model, which the zero-retention agreement does not bind");
    assert!(launch(Visibility::Private, until).is_ok(), "the agreement's last day still admits a private lane");
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
fn the_deepseek_brief_and_the_review_agree_on_who_lands() {
    let order = review::worker_order();
    let full: Vec<Verdict> = (0..FULL_REVIEWS).map(|_| verdict("feature", Kind::Clean, "00000001")).collect();
    assert_eq!(review::Lander::of(&[], "feature", false, false, "00000001"), review::Lander::Reviewer, "a category under full review goes to a reviewer");
    assert_eq!(review::Lander::of(&full, "feature", false, false, "00000001"), review::Lander::ReviewJob, "an unsampled commit is landed by the review job");
    assert!(order.contains("never run safe-push"), "{order}");
    assert!(order.contains("Sonnet reviewer, who lands it") && order.contains("review job lands any other commit through safe-push itself"), "the worker is told both landings the review code takes: {order}");
    assert!(order.contains("the lander ticks the box") && !order.contains("reviewer lands it and ticks"), "nobody but the lander ticks, on either path: {order}");
    let reviewer = review::brief("ds", "north#1020", "feature", None, std::path::Path::new("/lane"), "abc", "medium");
    assert!(reviewer.contains("land it with `safe-push` from the lane"), "the reviewer brief lands the sampled commit: {reviewer}");
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

#[test]
fn a_resting_session_on_a_build_host_that_reported_done_gets_a_review_job_and_one_that_has_not_gets_none() {
    let session = |name: &str, status: &str| json!({"name": name, "provider": "opencode", "status": status, "tasks": ["north#7"], "box": "the box"});
    let held = |name: &str, status: &str, past: bool, item: &str| json!({"name": name, "provider": "opencode", "status": status, "machine": "cpx62b", "past": past, "ended": if past { "retired" } else { "" }, "tasks": [item], "box": "the box"});
    let said = |text: &str, ended_ms: u64| json!({"entries": [{"kind": "assistant", "text": text}], "turns": [{"ended_ms": ended_ms}]});
    let lists = vec![
        ("nexus".to_owned(), json!([held("held-done", "parked", false, "north#7"), held("held-later", "idle", false, "north#7"), held("held-last", "idle", false, "north#7"), held("retired-done", "exited", true, "north#7"), held("retired-reviewed", "exited", true, "north#7"), held("retired-closed", "exited", true, "north#8"), held("retired-ticked", "exited", true, "north#9")])),
        ("gex45".to_owned(), json!([session("on-here", "working")])),
        ("cpx62".to_owned(), json!([session("remote-done", "idle"), session("remote-busy", "idle")])),
    ];
    let read = |host: Option<&str>| match host {
        None => Ok(json!({"held-done": said("Done: h", 300), "held-later": said("Done: l", 400), "held-last": said("Done: z", 500), "retired-done": said("Done: r", 100), "retired-reviewed": said("Done: q", 50), "retired-closed": said("Done: c", 60), "retired-ticked": said("Done: t", 70)})),
        Some("cpx62") => Ok(json!({"remote-done": said("Done: x", 200), "remote-busy": said("Not done: y", 10)})),
        _ => Err("unreached".to_owned()),
    };
    let record = tempfile::tempdir().unwrap();
    std::fs::write(record.path().join("deepseek-reviews.jsonl"), "{\"at\":9990,\"name\":\"retired-reviewed\"}\n{\"at\":7300,\"name\":\"held-done\",\"failed\":\"refused\"}\n{\"at\":2800,\"name\":\"retired-done\",\"failed\":\"refused\"}\n").unwrap();
    let marked = review::queued(record.path(), 10_000.0);
    let mut notes = Vec::new();

    let reports = super::reports("nexus", None, &json!(null), &lists, &marked, &read, &mut notes);
    let found = review::done(&lists, &reports, &[], &[issue("north", 7, "- [ ] the box", &[]), issue("north", 9, "- [x] the box", &[])], &marked);

    assert_eq!(found.done.iter().map(|done| (done.name.as_str(), done.host.as_str())).collect::<Vec<_>>(), [("retired-done", "nexus"), ("remote-done", "cpx62"), ("held-later", "nexus"), ("held-last", "nexus")], "a session the control host holds for a build host is reviewed where it is held, once even after the fleet retired it, oldest Done first, and one whose job failed within the hour sits out that hour");
    assert_eq!(found.left.iter().map(|left| (left.name.as_str(), left.why.as_str(), left.retired)).collect::<Vec<_>>(), [("retired-closed", "north#8: its issue is not among the open issues dispatch reads", true), ("retired-ticked", "north#9: its box is ticked", true)]);
    assert!(notes.is_empty(), "{notes:?}");
    let lines = review::queue(&found, "nexus", None, &|_, _| Err("no remote run in a dry run".to_owned()), true);
    assert_eq!(
        lines,
        [
            "would leave retired-closed on nexus: north#8: its issue is not among the open issues dispatch reads",
            "would leave retired-ticked on nexus: north#9: its box is ticked",
            "would review retired-done on nexus (north#7, feature)",
            "would review remote-done on cpx62 (north#7, feature)",
            "would review held-later on nexus (north#7, feature)",
            "review: would queue 3 of 4 waiting, 1 left for later runs at 3 a run; would leave 2",
        ],
        "one run queues the three oldest and names the rest"
    );
}

#[test]
fn a_resting_session_stops_counting_against_the_provider_cap_once_its_review_job_is_queued() {
    let session = |name: &str, status: &str| json!({"name": name, "provider": "opencode", "status": status, "tasks": ["north#7"]});
    let lists = vec![("cpx62".to_owned(), json!([session("busy", "working"), session("waiting", "idle"), session("queued", "idle"), session("queued-parked", "parked")]))];
    let queued = BTreeSet::from(["queued".to_owned(), "queued-parked".to_owned()]);

    assert_eq!(super::counted(&lists, &BTreeSet::new()), 4);
    assert_eq!(super::counted(&lists, &queued), 2);
}
