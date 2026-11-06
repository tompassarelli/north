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
    let command = prepare("/bin/gh", &words(&["run", "list"]), "worker-1", 10, &ledger, 1000, || Ok("ghs_agent".to_owned())).unwrap();
    assert_eq!(command.get_program(), "/bin/gh");
    assert_eq!(command.get_args().collect::<Vec<_>>(), ["run", "list"]);
    assert_eq!(env_of(&command, "GH_TOKEN"), Some(Some("ghs_agent".to_owned())));
    for name in SCRUBBED.iter().skip(1) {
        assert_eq!(env_of(&command, name), Some(None), "{name} must be removed");
    }

    let directory = scratch("pushed").parent().unwrap().parent().unwrap().to_owned();
    fs::create_dir_all(&directory).unwrap();
    let pushed = directory.join("gh-write-token");
    fs::write(&pushed, "ghs_pushed\n").unwrap();
    let written = fs::metadata(&pushed).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let host = Host { secrets: directory, runtime: None, pushed: Some(pushed.clone()), api: String::new(), decrypt: Box::new(|_| panic!("decrypted the App key on a host the laptop pushes to")) };

    let (token, note) = session_token(&host, i64::try_from(written).unwrap() + 60, None).unwrap();
    assert_eq!(token, "ghs_pushed");
    assert!(note.unwrap().contains(&pushed.display().to_string()));
}

#[test]
fn a_session_without_an_app_token_is_refused_rather_than_given_a_personal_login() {
    let ledger = scratch("notoken");
    let refusal = prepare("/bin/gh", &words(&["issue", "list"]), "worker-1", 10, &ledger, 1000, || Err("could not decrypt".to_owned())).unwrap_err();
    assert_eq!(refusal, Refusal::Token("could not decrypt".to_owned()));
    assert!(fresh_spend(&ledger, 1000).is_empty());
}

#[test]
fn the_hourly_share_refuses_a_call_with_the_spend_and_when_it_frees_counts_a_rolling_hour_and_alarms_at_50_and_80_percent() {
    let ledger = scratch("budget");
    let unminted = || -> Result<String, String> { panic!("minted for a refused call") };
    for second in 0..3 {
        prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 3, &ledger, 1000 + second, token).unwrap();
    }
    let Refusal::Budget(message) = prepare("gh", &words(&["issue", "view", "2"]), "worker-1", 3, &ledger, 1100, unminted).unwrap_err() else { panic!("not a budget refusal") };
    assert!(message.contains("worker-1") && message.contains("spent 3 of its 3") && message.contains("frees in 59 min"), "{message}");

    let window = scratch("window");
    prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 1, &window, 1000, token).unwrap();
    assert!(prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 1, &window, 1000 + WINDOW_SECONDS - 1, token).is_err());
    assert!(prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 1, &window, 1000 + WINDOW_SECONDS, token).is_ok(), "spend older than an hour no longer counts");

    assert_eq!(alarm_crossed(0, 1, 10), None);
    assert_eq!(alarm_crossed(4, 1, 10), Some(50));
    assert_eq!(alarm_crossed(5, 1, 10), None);
    assert_eq!(alarm_crossed(7, 1, 10), Some(80));
    assert_eq!(alarm_crossed(4, 5, 10), Some(80), "one call over both lines tells the proxy the higher one");
    assert_eq!(alarm_crossed(8, 1, 10), None);
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
