use super::issues::IssueRef;
use super::plan::Cloud;
use super::tests::{CONFIG, fleet, issue, survey};
use super::decide;

#[test]
fn north_issues_are_candidates_held_by_their_session_names_and_run_on_hosts_not_the_cloud_lane() {
    let listing = serde_json::json!([{"name": "north-254-d1435", "provider": "claude", "account": "c", "status": "working", "task_ref": null, "machine": null}]);
    let sessions = super::fleets::running_sessions(&listing);
    assert_eq!(sessions[0].items.iter().map(ToString::to_string).collect::<Vec<_>>(), ["north#254"]);

    let (hosts, accounts) = fleet();
    let boxes = vec![issue("north", 7, "- [ ] a\nSurface: rust", &[]), issue("wisp", 8, "- [ ] a\nSurface: ts", &[])];
    let outcome = decide(&survey(boxes, Cloud::Reachable("cloud credit: unread".into()), hosts, accounts), "test");
    let placed: Vec<(String, bool)> = outcome.launches.iter().map(|launch| (launch.item.to_string(), launch.cloud)).collect();
    assert_eq!(placed, [("north#7".to_owned(), false), ("wisp#8".to_owned(), true)], "north has no courier, so only Autoland repositories take the cloud lane");
    assert_eq!(super::effects::session_name(&outcome.launches[0], 0.0), "north-7-d0000");
}

#[test]
fn the_staffed_repos_are_the_projects_in_priority_order_less_private_ones_and_a_paused_project_skips_with_its_time() {
    let projects = super::projects::Projects::read(&CONFIG.parse().unwrap()).unwrap();
    assert_eq!(projects.repos(), ["north", "smashcraft", "wisp"], "galileo is private, south and firn are narrowed out by dispatch_repos, and priority 1 comes first");

    let paused = CONFIG.replace("dispatch_repos = [\"smashcraft\"]", "dispatch_repos = [\"smashcraft\"]\ndispatch_paused_until = \"2026-10-27T14:50:00Z\"");
    let cfg: toml_edit::DocumentMut = paused.parse().unwrap();
    let projects = super::projects::Projects::read(&cfg).unwrap();
    let until = 1_793_112_600.0;
    assert_eq!(projects.paused("smashcraft", until - 60.0).as_deref(), Some("project smashcraft paused until 2026-10-27T14:50:00Z (dispatch_paused_until)"));
    assert_eq!(projects.paused("smashcraft", until), None, "the pause ends at its time");
    assert_eq!(projects.paused("wisp", until - 60.0), None, "only the paused project waits");

    let (hosts, accounts) = fleet();
    let mut held = survey(vec![issue("smashcraft", 3, "- [ ] a", &[]), issue("wisp", 4, "- [ ] a", &[])], Cloud::Closed(String::new()), hosts, accounts);
    (held.projects, held.at) = (projects, until - 60.0);
    let outcome = decide(&held, "test");
    assert_eq!(outcome.launches.iter().map(|launch| launch.item.to_string()).collect::<Vec<_>>(), ["wisp#4"]);
    assert_eq!(outcome.skipped, [(IssueRef::new("smashcraft", 3), "project smashcraft paused until 2026-10-27T14:50:00Z (dispatch_paused_until)".to_owned())]);
    assert_eq!(outcome.ready, 1, "a paused box is not ready work, so it raises no alert");

    let bad: toml_edit::DocumentMut = CONFIG.replace("repos = [\"wisp\"]", "repos = [\"wisp\"]\ndispatch_paused_until = \"tomorrow\"").parse().unwrap();
    assert!(super::projects::Projects::read(&bad).unwrap_err().contains("not an RFC 3339 time"), "a pause that cannot be read fails dispatch rather than being ignored");
    assert!(super::projects::Projects::read(&"[placement]\n".parse().unwrap()).unwrap_err().contains("no [projects.NAME] repos"));
}

#[test]
fn dispatch_staffs_the_oldest_ready_issue_first_across_projects_within_a_priority() {
    let projects = super::projects::Projects::read(&CONFIG.parse().unwrap()).unwrap();
    let opened = |repo: &str, number: u64, created: f64, labels: &[&str]| super::issues::Issue { created, ..issue(repo, number, "- [ ] a", labels) };
    let issues = [
        opened("north", 340, 3_000.0, &[]),
        opened("wisp", 12, 1_000.0, &[]),
        opened("smashcraft", 700, 2_000.0, &[]),
        opened("north", 339, 2_000.0, &[]),
        opened("smashcraft", 738, 4_000.0, &["priority:now"]),
    ];
    let order: Vec<String> = super::ordered(&issues, &projects).iter().map(|issue| issue.reference().to_string()).collect();
    assert_eq!(order, ["smashcraft#738", "wisp#12", "north#339", "smashcraft#700", "north#340"], "priority:now first, then oldest; a tie goes to project order");
}
