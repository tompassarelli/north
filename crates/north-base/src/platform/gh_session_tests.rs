use std::ffi::OsStr;

use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn env_of(command: &Command, name: &str) -> Option<Option<String>> {
    command.get_envs().find(|(key, _)| *key == OsStr::new(name)).map(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("north-gh-session-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    directory.join("budget").join("session")
}

fn token() -> Result<String, String> {
    Ok("t".to_owned())
}

#[test]
fn a_session_gh_call_runs_with_the_app_token_and_never_a_personal_one() {
    let ledger = scratch("token");
    let command = prepare("/bin/gh", &words(&["run", "list"]), "worker-1", Share { budget: 10, ledger: &ledger }, 1000, &command_origin(), || Ok("ghs_agent".to_owned())).unwrap();
    assert_eq!(command.get_program(), "/bin/gh");
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["run", "list"]);
    assert_eq!(env_of(&command, "GH_TOKEN"), Some(Some("ghs_agent".to_owned())));
    for name in SCRUBBED.iter().skip(1) {
        assert_eq!(env_of(&command, name), Some(None), "{name} must be removed");
    }
}

#[test]
fn a_session_without_an_app_token_is_refused_rather_than_given_a_personal_login() {
    let ledger = scratch("notoken");
    let refusal = prepare("/bin/gh", &words(&["issue", "list"]), "worker-1", Share { budget: 10, ledger: &ledger }, 1000, &command_origin(), || Err("could not decrypt".to_owned())).unwrap_err();
    assert_eq!(refusal, Refusal::Token("could not decrypt".to_owned()));
    assert!(fresh_spend(&ledger, 1000).is_empty());
}

#[test]
fn the_hourly_share_refuses_a_call_with_the_spend_and_when_it_frees_counts_a_rolling_hour_and_alarms_at_50_and_80_percent() {
    let ledger = scratch("budget");
    let unminted = || -> Result<String, String> { panic!("minted for a refused call") };
    for second in 0..3 {
        prepare("gh", &words(&["issue", "view", "1"]), "worker-1", Share { budget: 3, ledger: &ledger }, 1000 + second, &command_origin(), token).unwrap();
    }
    let Refusal::Budget(message) = prepare("gh", &words(&["issue", "view", "2"]), "worker-1", Share { budget: 3, ledger: &ledger }, 1100, &command_origin(), unminted).unwrap_err() else { panic!("not a budget refusal") };
    assert!(message.contains("worker-1") && message.contains("spent 3 of its 3") && message.contains("frees in 59 min"), "{message}");

    let window = scratch("window");
    prepare("gh", &words(&["issue", "view", "1"]), "worker-1", Share { budget: 1, ledger: &window }, 1000, &command_origin(), token).unwrap();
    assert!(prepare("gh", &words(&["issue", "view", "1"]), "worker-1", Share { budget: 1, ledger: &window }, 1000 + WINDOW_SECONDS - 1, &command_origin(), token).is_err());
    assert!(prepare("gh", &words(&["issue", "view", "1"]), "worker-1", Share { budget: 1, ledger: &window }, 1000 + WINDOW_SECONDS, &command_origin(), token).is_ok(), "spend older than an hour no longer counts");

    let hooks = scratch("hooks");
    let hook = gh_origin::Origin { kind: Kind::Hook, caller: "north-hook".to_owned() };
    prepare("gh", &words(&["issue", "view", "1"]), "proxy", Share { budget: 20, ledger: &hooks }, 1000, &hook, token).unwrap();
    prepare("gh", &words(&["api", "-H", "x:y", "repos/o/r/pulls?state=open"]), "proxy", Share { budget: 20, ledger: &hooks }, 1001, &hook, token).unwrap();
    let Refusal::Budget(message) = prepare("gh", &words(&["issue", "view", "2"]), "proxy", Share { budget: 20, ledger: &hooks }, 1002, &hook, token).unwrap_err() else { panic!("not a budget refusal") };
    assert!(message.contains("hooks have spent 2 of their 2"), "a hook is refused past a tenth of the share: {message}");
    prepare("gh", &words(&["issue", "view", "3"]), "proxy", Share { budget: 20, ledger: &hooks }, 1003, &command_origin(), token).unwrap();
    let seen: Vec<_> = fresh_spend(&hooks, 1004).into_iter().map(|spend| (spend.kind, spend.caller, spend.endpoint)).collect();
    let named = |kind, caller: &str, endpoint: &str| (kind, caller.to_owned(), endpoint.to_owned());
    assert_eq!(seen, [named(Kind::Hook, "north-hook", "issue:view"), named(Kind::Hook, "north-hook", "api:repos/o/r/pulls"), named(Kind::Command, "shell", "issue:view")]);
    let chain = |lines: &[&str]| lines.iter().map(|line| words(&line.split(' ').collect::<Vec<_>>())).collect::<Vec<_>>();
    assert_eq!(gh_origin::classify(&chain(&["/bin/north threads claim", "/bin/north hook", "bash north-hook.sh"])).kind, Kind::Hook, "a hook's nested north verbs still count as the hook");
    assert_eq!(gh_origin::classify(&chain(&["north dispatch", "north fleet tick"])).kind, Kind::Tick);
    assert_eq!(gh_origin::classify(&chain(&["bash -c gh", "north threads claim x"])).caller, "north-threads");

    let notices = scratch("notices");
    let at = |before, units, now| budget_notices("worker-1", &notices, before, units, 10, now).into_iter().map(|(recipient, _)| recipient).collect::<Vec<_>>();
    let to = |before, units| at(before, units, 2000);
    assert!(to(0, 1).is_empty());
    assert_eq!(to(4, 1), ["worker-1"], "50% tells the spending session, not the proxy");
    assert!(to(5, 1).is_empty());
    assert_eq!(to(7, 1), ["worker-1"], "80% tells the spending session, not the proxy");
    assert!(budget_notices("worker-1", &notices, 4, 5, 10, 2000)[0].1.contains("80%"), "one call over both lines names the higher one");
    assert!(to(8, 1).is_empty());
    assert_eq!(to(10, 1), ["proxy"], "only a refusal at 100% reaches the proxy");
    assert!(to(10, 1).is_empty(), "a second refusal in the same window sends the proxy nothing");
    assert!(to(9, 1).is_empty(), "an admitted call sends nothing");
    assert!(at(10, 1, 2000 + WINDOW_SECONDS - 1).is_empty(), "a refusal after an admitted small call within the hour of the first sends the proxy nothing");
    assert_eq!(at(10, 1, 2000 + WINDOW_SECONDS), ["proxy"], "a refusal an hour after the first tells the proxy once more");
    assert!(at(10, 1, 2000 + WINDOW_SECONDS + 1).is_empty(), "the hour restarts from that notice");
    assert!(to(10, 0).is_empty(), "a free call is never refused");
}

#[test]
fn paginated_and_long_listings_cost_more_and_help_costs_nothing() {
    assert_eq!(cost(&words(&["api", "--paginate", "repos/x/y/actions/runs"])), PAGINATE_COST);
    assert_eq!(cost(&words(&["run", "list", "--limit", "1000"])), 10);
    assert_eq!(cost(&words(&["run", "list", "-L", "250"])), 3);
    assert_eq!(cost(&words(&["run", "list", "--limit=30"])), 1);
    assert_eq!(cost(&words(&["issue", "view", "5"])), 1);
    assert_eq!(cost(&words(&["--version"])), 0);
    assert_eq!(cost(&words(&["issue", "--help"])), 0);
    assert_eq!(cost(&words(&["config", "get", "editor"])), 0);
    assert_eq!(cost(&[]), 0);
}

fn command_origin() -> gh_origin::Origin {
    gh_origin::Origin { kind: Kind::Command, caller: "shell".to_owned() }
}

