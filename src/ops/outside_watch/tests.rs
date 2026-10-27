use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;

use serde_json::{Value, json};

use super::scan::{Post, domains, is_spam, summary};
use super::{Config, Failure, Gh, check, status};
use crate::foundation::clock::Timestamp;

const FIXTURES: &str = include_str!("fixtures/gh.json");
const RECORDED: &str = include_str!("fixtures/recorded.json");
const NOW: &str = "2026-10-14T09:30:00Z";

struct FakeGh {
    fixtures: Value,
    calls: RefCell<Vec<Vec<String>>>,
    fail_everything: bool,
}

impl FakeGh {
    fn new() -> Self {
        Self { fixtures: serde_json::from_str(FIXTURES).unwrap(), calls: RefCell::new(Vec::new()), fail_everything: false }
    }
}

impl Gh for FakeGh {
    fn api(&self, args: &[String], ok_missing: bool) -> Result<Option<Value>, Failure> {
        self.calls.borrow_mut().push(args.to_vec());
        if self.fail_everything {
            return if ok_missing { Ok(None) } else { Err("outside-watch: gh api failed: rate limited".to_owned()) };
        }
        let (mut method, mut fields, mut endpoint, mut index) = ("GET", Vec::new(), None, 0);
        while index < args.len() {
            match args[index].as_str() {
                "-X" => {
                    method = args[index + 1].as_str();
                    index += 1;
                }
                "-f" => {
                    fields.push(args[index + 1].as_str());
                    index += 1;
                }
                other if !other.starts_with('-') && endpoint.is_none() => endpoint = Some(other),
                _ => {}
            }
            index += 1;
        }
        let mut endpoint = endpoint.unwrap().trim_start_matches("https://api.github.com/").split('?').next().unwrap().to_owned();
        if endpoint == "graphql" {
            endpoint += if fields.iter().any(|field| field.contains("minimizeComment")) { ":minimize" } else { ":search" };
        }
        let body = match self.fixtures.get(format!("{method} {endpoint}")) {
            Some(body) => body.clone(),
            None if method == "GET" && endpoint.starts_with("user/blocks/") => return if ok_missing { Ok(None) } else { Err("not found".to_owned()) },
            None if args.iter().any(|arg| arg == "--paginate") => json!([]),
            None => json!({}),
        };
        Ok(Some(if args.iter().any(|arg| arg == "--slurp") { json!([body]) } else { body }))
    }
}

struct Step {
    stdout: String,
    calls: Vec<Vec<String>>,
    inbox: String,
    state: Value,
    log: Value,
}

fn config(root: &std::path::Path) -> Config {
    Config {
        owner: "tompassarelli".to_owned(),
        own: ["tompassarelli", "github-actions[bot]", "dependabot[bot]"].map(str::to_owned).into(),
        state_dir: root.join("state"),
        inbox: root.join("proxy-inbox.md"),
        now: Timestamp::parse(NOW).unwrap(),
    }
}

fn replay() -> Vec<Step> {
    let scratch = tempfile::tempdir().unwrap();
    let config = config(scratch.path());
    fs::write(&config.inbox, "earlier line\n").unwrap();
    let gh = FakeGh::new();
    let mut steps = Vec::new();
    let mut record = |gh: &FakeGh, stdout: Vec<u8>| {
        let read = |name: &str| fs::read_to_string(config.state_dir.join(name)).ok();
        steps.push(Step {
            stdout: String::from_utf8(stdout).unwrap(),
            calls: gh.calls.replace(Vec::new()),
            inbox: fs::read_to_string(&config.inbox).unwrap(),
            state: read("state.json").map_or(Value::Null, |text| serde_json::from_str(&text).unwrap()),
            log: read("log").map_or(Value::Null, Value::String),
        });
    };
    let mut out = Vec::new();
    status(&config, &mut out).unwrap();
    record(&gh, out);
    for (dry_run, days) in [(true, Some(7.0)), (false, None), (false, None)] {
        let mut out = Vec::new();
        check(&config, &gh, dry_run, days, &mut out).unwrap();
        record(&gh, out);
    }
    let mut out = Vec::new();
    status(&config, &mut out).unwrap();
    record(&gh, out);
    steps
}

fn assert_step(new: &Step, old: &Value) {
    assert_eq!(new.stdout, old["stdout"].as_str().unwrap());
    assert_eq!(json!(new.calls), old["calls"]);
    assert_eq!(new.inbox, old["inbox"].as_str().unwrap());
    assert_eq!(new.state, old["state"]);
    assert_eq!(new.log, old["log"]);
}

#[test]
fn the_five_recorded_steps_match_the_python_tool_and_keep_its_guarantees() {
    let recorded: Value = serde_json::from_str(RECORDED).unwrap();
    let steps = replay();
    assert_eq!(steps.len(), 5);
    for (new, label) in steps.iter().zip(["status-empty", "dry", "live", "again", "status"]) {
        let old = recorded.as_array().unwrap().iter().find(|step| step["label"] == label).unwrap();
        assert_step(new, old);
    }
    let (dry, live, again) = (&steps[1], &steps[2], &steps[3]);
    assert_eq!(steps[0].stdout, "last run: never\n");
    assert!(dry.stdout.lines().any(|line| line.starts_with("SPAM smashcraft#422 AILIFE1:")));
    assert!(dry.stdout.lines().any(|line| line.starts_with("EXTERNAL smashcraft#600 newbie:")));
    assert!(dry.calls.iter().all(|call| !call.iter().any(|arg| arg == "PUT" || arg == "PATCH" || arg.contains("minimizeComment"))));
    assert_eq!((dry.inbox.as_str(), &dry.state), ("earlier line\n", &Value::Null));
    assert!(live.calls.iter().any(|call| call.iter().any(|arg| arg == "id=IC_1")));
    assert_eq!(live.calls.iter().filter(|call| call.iter().any(|arg| arg == "PUT")).count(), 1);
    let marked: BTreeSet<&str> = live.calls.iter().filter(|call| call.iter().any(|arg| arg == "PATCH")).map(|call| call.last().unwrap().as_str()).collect();
    assert_eq!(marked, BTreeSet::from(["notifications/threads/n-ci", "notifications/threads/n-ours"]));
    assert_eq!(again.inbox, live.inbox);
    assert!(again.calls.iter().all(|call| !call.iter().any(|arg| arg == "PUT" || arg.contains("minimizeComment"))));
}

fn user_post(body: &str) -> Post {
    Post {
        kind: "comment",
        repo: "smashcraft".into(),
        number: 1,
        id: "IC_9".into(),
        actor: "someone".into(),
        is_user: true,
        body: body.into(),
        association: "NONE".into(),
        created: NOW.into(),
        url: "u".into(),
    }
}

#[test]
fn domains_spam_summaries_and_a_failing_gh_call_follow_the_python_rules() {
    let found = domains("see https://www.Example.COM/docs, api.github.com, foo.community, raw.githubusercontent.com/x and docs.stormlib.org.");
    assert_eq!(found, BTreeSet::from(["example.com".to_owned(), "docs.stormlib.org".to_owned()]));

    let at = Timestamp::parse(NOW).unwrap();
    let young = json!({"created_at": "2026-10-01T00:00:00Z", "blog": ""});
    let old_linker = json!({"created_at": "2020-01-01T00:00:00Z", "blog": "https://www.promo.io"});
    let old_other = json!({"created_at": "2020-01-01T00:00:00Z", "blog": "https://carol.dev"});
    let post = user_post("try promo.io today");
    assert!(is_spam(&post, &young, "", at));
    assert!(is_spam(&post, &old_linker, "", at));
    assert!(!is_spam(&post, &old_other, "", at));
    assert!(!is_spam(&post, &young, "the thread already links promo.io", at));
    assert!(!is_spam(&Post { is_user: false, ..user_post("try promo.io today") }, &young, "", at));
    assert!(!is_spam(&Post { association: "MEMBER".into(), ..user_post("try promo.io today") }, &young, "", at));

    assert_eq!(summary(&user_post("a  \"b\"\n c")), "a 'b' c");
    let long = summary(&user_post(&"x".repeat(150)));
    assert_eq!((long.chars().count(), long.ends_with('…')), (141, true));

    let scratch = tempfile::tempdir().unwrap();
    let config = config(scratch.path());
    let gh = FakeGh { fail_everything: true, ..FakeGh::new() };
    let error = check(&config, &gh, false, None, &mut Vec::new()).unwrap_err();
    assert!(error.starts_with("outside-watch: gh api"), "{error}");
    assert!(!config.state_dir.exists());
}
