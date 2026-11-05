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
}

#[test]
fn a_session_without_an_app_token_is_refused_rather_than_given_a_personal_login() {
    let ledger = scratch("notoken");
    let refusal = prepare("/bin/gh", &words(&["issue", "list"]), "worker-1", 10, &ledger, 1000, || Err("could not decrypt".to_owned())).unwrap_err();
    assert_eq!(refusal, Refusal::Token("could not decrypt".to_owned()));
    assert!(fresh_spend(&ledger, 1000).is_empty());
}

#[test]
fn a_call_past_the_hourly_share_is_refused_with_the_spend_and_when_it_frees() {
    let ledger = scratch("budget");
    let unminted = || -> Result<String, String> { panic!("minted for a refused call") };
    for second in 0..3 {
        prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 3, &ledger, 1000 + second, token).unwrap();
    }
    let Refusal::Budget(message) = prepare("gh", &words(&["issue", "view", "2"]), "worker-1", 3, &ledger, 1100, unminted).unwrap_err() else { panic!("not a budget refusal") };
    assert!(message.contains("worker-1") && message.contains("spent 3 of its 3") && message.contains("frees in 59 min"), "{message}");
}

#[test]
fn spend_older_than_an_hour_no_longer_counts() {
    let ledger = scratch("window");
    prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 1, &ledger, 1000, token).unwrap();
    assert!(prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 1, &ledger, 1000 + WINDOW_SECONDS - 1, token).is_err());
    assert!(prepare("gh", &words(&["issue", "view", "1"]), "worker-1", 1, &ledger, 1000 + WINDOW_SECONDS, token).is_ok());
    assert_eq!(fs::read_to_string(&ledger).unwrap(), format!("{} 1\n", 1000 + WINDOW_SECONDS));
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
