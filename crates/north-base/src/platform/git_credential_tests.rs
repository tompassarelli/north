use std::process::{Command, Stdio};

use super::*;
use crate::foundation::shim::write_script;

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

#[test]
fn a_mint_failure_is_reported_not_answered() {
    let reply = answer(&words(&["get"]), "protocol=https\nhost=github.com\n".as_bytes(), || Err("could not decrypt github-app.yaml".to_owned()));
    assert_eq!(reply, Err("could not decrypt github-app.yaml".to_owned()));
}

#[test]
fn the_session_env_replaces_the_inherited_github_helper_and_leaves_other_hosts_alone() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let bin = scratch.path().join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        home.join(".gitconfig"),
        "[credential \"https://github.com\"]\n\thelper = \"!f() { echo username=admin; echo password=admin-token; }; f\"\n[credential \"https://gist.github.com\"]\n\thelper = \"!f() { echo username=admin; echo password=admin-token; }; f\"\n",
    )
    .unwrap();
    write_script(&bin.join("north"), "test \"$1 $2\" = 'git-credential get' && printf 'username=x-access-token\\npassword=ghs_agent\\n'\n");
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let fill = |host: &str| {
        let mut command = Command::new("git");
        command.args(["credential", "fill"]).env_clear().env("HOME", &home).env("PATH", &path).env("GIT_TERMINAL_PROMPT", "0");
        command.envs(session_env()).stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(format!("protocol=https\nhost={host}\n\n").as_bytes()).unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    };
    assert!(fill("github.com").contains("password=ghs_agent\n"), "{}", fill("github.com"));
    assert!(fill("gist.github.com").contains("password=admin-token\n"), "{}", fill("gist.github.com"));
}
