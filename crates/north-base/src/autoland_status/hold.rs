use super::{commit_status, text};
use crate::platform::process;
use std::process::Command;
use std::time::{Duration, Instant};
use crate::foundation::clock::Timestamp;
use serde_json::Value;

const QUEUE_PREFIX: &str = "claude/land-";
const HOOK_BUDGET: Duration = Duration::from_secs(8);
const PENDING_SETTLE_MS: i64 = 24 * 3_600_000;
const UNKNOWN_SETTLE_MS: i64 = 4 * 3_600_000;
const DISPATCHED: [&str; 10] = ["run", "list", "--workflow", "autoland.yml", "--event", "workflow_dispatch", "--limit", "100", "--json", "databaseId,displayTitle,event,status,conclusion,url,createdAt"];
const OWN: [&str; 10] = ["run", "list", "--workflow", "autoland.yml", "--branch", "", "--limit", "20", "--json", "status,createdAt,headBranch,url"];

fn unreadable(branch: &str) -> Option<String> {
    held(branch, "its Autoland runs could not be read")
}

fn held(branch: &str, why: &str) -> Option<String> {
    Some(format!("{branch}: {why}; deleting it would drop the lane unlanded, so wait for Autoland to land or hand it back"))
}

fn started_ms(run: &Value) -> Option<i64> {
    text(run, "createdAt").and_then(Timestamp::parse_rfc3339).map(Timestamp::unix_millis)
}

fn unfinished(run: &Value) -> bool {
    text(run, "status") != Some("completed")
}

pub fn deletion_hold(branch: &str, tip: &str, now_ms: i64, mut gh: impl FnMut(&[&str]) -> Option<String>) -> Option<String> {
    if !branch.starts_with(QUEUE_PREFIX) {
        return None;
    }
    let Some(outcome) = commit_status(tip, &mut gh) else {
        return unreadable(branch);
    };
    let state = outcome.split('\t').next().unwrap_or("").trim();
    if matches!(state, "success" | "failure" | "error") {
        return None;
    }
    let Some(dispatched) = gh(&DISPATCHED).and_then(|payload| serde_json::from_str::<Value>(&payload).ok()) else {
        return unreadable(branch);
    };
    let mut own = OWN;
    own[5] = branch;
    let Some(pushes) = gh(&own).and_then(|payload| serde_json::from_str::<Value>(&payload).ok()) else {
        return unreadable(branch);
    };
    let (Some(dispatched), Some(pushes)) = (dispatched.as_array(), pushes.as_array()) else {
        return unreadable(branch);
    };
    let suffix = format!(" {branch} {tip}");
    let title = |run: &Value| text(run, "displayTitle").unwrap_or_default().to_owned();
    let tests: Vec<&Value> = dispatched.iter().filter(|run| title(run).starts_with("Autoland test ") && title(run).ends_with(&suffix)).collect();
    let pushes: Vec<&Value> = pushes.iter().filter(|run| text(run, "headBranch") == Some(branch)).collect();
    let shared = dispatched.iter().filter(|run| title(run) == "Autoland train" || title(run).starts_with("Autoland plan"));
    if let Some(live) = tests.iter().chain(&pushes).copied().chain(shared).find(|run| unfinished(run)) {
        return held(branch, &format!("an Autoland run {} is {}", text(live, "url").unwrap_or("(no url)"), text(live, "status").unwrap_or("unfinished")));
    }
    let newest = tests.iter().chain(&pushes).filter_map(|run| started_ms(run)).max()?;
    let settle = if state == "pending" { PENDING_SETTLE_MS } else { UNKNOWN_SETTLE_MS };
    (now_ms - newest < settle).then(|| format!("{branch}: its newest Autoland run started under {} hours ago and Autoland has not landed or handed it back; deleting it would drop the lane unlanded, so wait for Autoland to land or hand it back", settle / 3_600_000))
}

fn within(deadline: Instant, program: &str, arguments: &[&str], repository: Option<&str>) -> Option<String> {
    let mut command = Command::new(program);
    command.args(arguments);
    if let Some(repository) = repository {
        command.env("GH_REPO", repository);
    }
    let left = deadline.checked_duration_since(Instant::now()).filter(|left| !left.is_zero())?;
    process::run_command_within(command, None, left)
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
}

fn repository_slug(url: &str) -> Option<String> {
    let tail = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let mut parts = tail.rsplit(['/', ':']);
    let (repo, owner) = (parts.next()?, parts.next()?);
    (!repo.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{repo}"))
}

pub fn hook_deadline() -> Instant {
    Instant::now() + HOOK_BUDGET
}

pub fn origin_branches_hold(deadline: Instant, directory: &str, remote: &str, branches: &[String]) -> Option<String> {
    branches.iter().find_map(|branch| origin_branch_hold(deadline, directory, remote, branch))
}

fn origin_branch_hold(deadline: Instant, directory: &str, remote: &str, branch: &str) -> Option<String> {
    if !branch.starts_with(QUEUE_PREFIX) {
        return None;
    }
    let git = |arguments: &[&str]| within(deadline, "git", arguments, None);
    let Some(listed) = git(&["-C", directory, "ls-remote", remote, &format!("refs/heads/{branch}")]) else {
        return held(branch, "origin's tip could not be read in time, so a queued Autoland run cannot be ruled out");
    };
    let tip = listed.split_whitespace().next()?;
    let url = git(&["-C", directory, "remote", "get-url", "--push", remote]).unwrap_or_else(|| remote.to_owned());
    let slug = repository_slug(&url);
    deletion_hold(branch, tip, Timestamp::now().unix_millis(), |arguments| within(deadline, "gh", arguments, slug.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::{deletion_hold, repository_slug};
    use serde_json::{Value, json};

    const BRANCH: &str = "claude/land-window-lighting-ea547387c6ab";
    const NOW: i64 = 1_793_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn at(ms: i64) -> String {
        crate::foundation::clock::Timestamp::from_unix_millis(ms).iso_millis()
    }

    struct Github {
        commit_status: &'static str,
        dispatched: Value,
        branch_runs: Option<Value>,
    }

    impl Github {
        fn new(commit_status: &'static str) -> Self {
            Github { commit_status, dispatched: json!([]), branch_runs: Some(json!([])) }
        }

        fn hold(&self, branch: &str) -> Option<String> {
            deletion_hold(branch, "tipsha", NOW, |arguments| match arguments[0] {
                "api" => Some(self.commit_status.to_owned()),
                _ if arguments.contains(&"--branch") => self.branch_runs.as_ref().map(ToString::to_string),
                _ => Some(self.dispatched.to_string()),
            })
        }
    }

    const PENDING: &str = "pending\twaiting for the train\turl";

    fn test_run(status: &str, started: i64) -> Value {
        json!({"displayTitle": format!("Autoland test 3 {BRANCH} tipsha"), "event": "workflow_dispatch", "status": status, "conclusion": null, "url": "https://example.test/test", "createdAt": at(started)})
    }

    fn shared_run(title: &str, status: &str) -> Value {
        json!({"displayTitle": title, "event": "workflow_dispatch", "status": status, "conclusion": null, "url": "https://example.test/shared", "createdAt": at(NOW)})
    }

    fn push_run(status: &str, started: i64) -> Value {
        json!({"status": status, "createdAt": at(started), "headBranch": BRANCH, "url": "https://example.test/push"})
    }

    #[test]
    fn any_unfinished_run_of_the_branch_or_the_shared_plan_and_train_holds_it() {
        for state in ["queued", "in_progress", "waiting"] {
            let test = Github { dispatched: json!([test_run(state, NOW - 30 * HOUR)]), ..Github::new("\t\t") };
            assert!(test.hold(BRANCH).unwrap().contains("https://example.test/test"), "{state}");
            let push = Github { branch_runs: Some(json!([push_run(state, NOW - 30 * HOUR)])), ..Github::new(PENDING) };
            assert!(push.hold(BRANCH).unwrap().contains("https://example.test/push"), "{state}");
        }
        for title in ["Autoland train", "Autoland plan", "Autoland plan claude/land-x-aaaaaaaaaaaa"] {
            let shared = Github { dispatched: json!([test_run("completed", NOW - 30 * HOUR), shared_run(title, "queued")]), ..Github::new(PENDING) };
            assert!(shared.hold(BRANCH).unwrap().contains("https://example.test/shared"), "{title}");
        }
    }

    #[test]
    fn a_passed_test_waiting_for_the_train_stays_held_for_a_day_and_an_unknown_outcome_for_four_hours() {
        let waiting = |status, started| Github { dispatched: json!([test_run("completed", started)]), ..Github::new(status) };
        assert!(waiting(PENDING, NOW - 20 * HOUR).hold(BRANCH).is_some());
        assert_eq!(waiting(PENDING, NOW - 25 * HOUR).hold(BRANCH), None);
        assert!(waiting("\t\t", NOW - 3 * HOUR).hold(BRANCH).is_some());
        assert_eq!(waiting("\t\t", NOW - 5 * HOUR).hold(BRANCH), None);
        let late_push = Github { branch_runs: Some(json!([push_run("completed", NOW - 2 * HOUR)])), ..Github::new("\t\t") };
        assert!(late_push.hold(BRANCH).is_some());
    }

    #[test]
    fn a_landed_handed_back_or_untouched_branch_is_free_to_delete() {
        for terminal in ["success\tlanded abc\turl", "failure\tred\turl", "error\thanded back\turl"] {
            let github = Github { branch_runs: Some(json!([push_run("queued", NOW)])), ..Github::new(terminal) };
            assert_eq!(github.hold(BRANCH), None, "{terminal}");
        }
        assert_eq!(Github::new("\t\t").hold(BRANCH), None);
        assert_eq!(Github::new(PENDING).hold(BRANCH), None);
        let other = json!([{"status": "queued", "createdAt": at(NOW), "headBranch": "claude/land-other-aaaaaaaaaaaa", "url": "u"}]);
        assert_eq!(Github { branch_runs: Some(other), ..Github::new(PENDING) }.hold(BRANCH), None);
    }

    #[test]
    fn unreadable_runs_hold_a_queue_branch_and_other_branches_never_read_github() {
        let unreadable = Github { branch_runs: None, ..Github::new(PENDING) };
        assert!(unreadable.hold(BRANCH).unwrap().contains("could not be read"));
        assert_eq!(deletion_hold("scratch/x", "tipsha", NOW, |_| panic!("no gh for a non-queue branch")), None);
        let no_status = deletion_hold(BRANCH, "tipsha", NOW, |arguments| (arguments[0] != "api").then(|| "[]".to_owned()));
        assert!(no_status.unwrap().contains("could not be read"));
    }

    #[test]
    fn the_repository_comes_from_the_remote_the_push_names() {
        assert_eq!(repository_slug("git@github.com:tompassarelli/smashcraft.git").as_deref(), Some("tompassarelli/smashcraft"));
        assert_eq!(repository_slug("https://github.com/tompassarelli/wisp").as_deref(), Some("tompassarelli/wisp"));
    }
}
