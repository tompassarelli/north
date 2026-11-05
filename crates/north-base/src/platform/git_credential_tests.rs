use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn get_for_github_over_https_answers_with_the_minted_app_token() {
    let reply = answer(&words(&["get"]), "protocol=https\nhost=github.com\npath=tompassarelli/north.git\n\n".as_bytes(), |path| Ok(format!("ghs_agent for {path}")));
    assert_eq!(reply, Ok(Some("username=x-access-token\npassword=ghs_agent for tompassarelli/north.git\n".to_owned())));
}

#[test]
fn other_hosts_store_and_erase_get_no_answer_and_mint_nothing() {
    let unminted = |_: &str| -> Result<String, String> { panic!("minted a token it did not need") };
    assert_eq!(answer(&words(&["get"]), "protocol=https\nhost=gist.github.com\n".as_bytes(), unminted), Ok(None));
    assert_eq!(answer(&words(&["get"]), "protocol=http\nhost=github.com\n".as_bytes(), unminted), Ok(None));
    assert_eq!(answer(&words(&["store"]), "protocol=https\nhost=github.com\n".as_bytes(), unminted), Ok(None));
    assert_eq!(answer(&words(&["erase"]), "protocol=https\nhost=github.com\n".as_bytes(), unminted), Ok(None));
    assert!(answer(&words(&[]), "".as_bytes(), unminted).is_err());
}

#[test]
fn a_host_that_cannot_mint_falls_back_to_the_installation_token_the_session_was_launched_with() {
    assert_eq!(session_token(Some("ghs_launched".to_owned())), Some("ghs_launched".to_owned()));
    assert_eq!(session_token(Some("ghp_personal".to_owned())), None);
    assert_eq!(session_token(None), None);
}

#[test]
fn a_host_that_cannot_mint_uses_its_pushed_token_only_while_it_is_fresh_and_covers_the_path() {
    let home = tempfile::tempdir().unwrap();
    let state = home.path().join(".local/state/north");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("gh-write-token"), "ghs_pushed\n").unwrap();
    std::fs::write(state.join("gh-write-token.repos"), "tompassarelli/north\ntompassarelli/south\n").unwrap();
    let written = std::fs::metadata(state.join("gh-write-token")).unwrap().modified().unwrap();
    let at = |path: &str, minutes: u64| pushed_token(home.path(), path, written + Duration::from_secs(minutes * 60));
    assert_eq!(at("TompAssarelli/south.git", 1), Some("ghs_pushed".to_owned()));
    assert_eq!(at("tompassarelli/south.git", 56), None);
    assert_eq!(at("tompassarelli/wisp.git", 1), None);
    assert_eq!(at("", 1), None);
}
