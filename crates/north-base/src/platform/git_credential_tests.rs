
use super::*;

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn get_for_github_over_https_answers_with_the_minted_app_token() {
    let reply = answer(&words(&["get"]), "protocol=https\nhost=github.com\n\n".as_bytes(), || Ok("ghs_agent".to_owned()));
    assert_eq!(reply, Ok(Some("username=x-access-token\npassword=ghs_agent\n".to_owned())));
}

#[test]
fn other_hosts_store_and_erase_get_no_answer_and_mint_nothing() {
    let unminted = || -> Result<String, String> { panic!("minted a token it did not need") };
    assert_eq!(answer(&words(&["get"]), "protocol=https\nhost=gist.github.com\n".as_bytes(), unminted), Ok(None));
    assert_eq!(answer(&words(&["get"]), "protocol=http\nhost=github.com\n".as_bytes(), unminted), Ok(None));
    assert_eq!(answer(&words(&["store"]), "protocol=https\nhost=github.com\n".as_bytes(), unminted), Ok(None));
    assert_eq!(answer(&words(&["erase"]), "protocol=https\nhost=github.com\n".as_bytes(), unminted), Ok(None));
    assert!(answer(&words(&[]), "".as_bytes(), unminted).is_err());
}

