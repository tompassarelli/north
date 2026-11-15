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
    let command = prepare("/bin/gh", &words(&["run", "list"]), "worker-1", Share { budget: 10, ledger: &ledger, pool: &open() }, 1000, &command_origin(), || Ok("ghs_agent".to_owned())).unwrap();
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
    let refusal = prepare("/bin/gh", &words(&["issue", "list"]), "worker-1", Share { budget: 10, ledger: &ledger, pool: &open() }, 1000, &command_origin(), || Err("could not decrypt".to_owned())).unwrap_err();
    assert_eq!(refusal, Refusal::Token("could not decrypt".to_owned()));
    assert!(fresh_spend(&ledger, 1000).is_empty());
}

fn command_origin() -> gh_origin::Origin {
    gh_origin::Origin { kind: Kind::Command, caller: "shell".to_owned() }
}

fn open() -> Pool {
    Pool { remaining: Some(8513), resets_at: 1000 + WINDOW_SECONDS, others: vec![30, 12] }
}

