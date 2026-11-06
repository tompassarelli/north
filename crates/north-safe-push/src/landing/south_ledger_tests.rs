use super::{Host, route};

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

fn client() -> Host {
    Host { ledger_client: true, ledger_host: Some("nexus".to_owned()) }
}

#[test]
fn remote_land_leaves_north_other_branches_tags_and_full_hosts_to_git_push() {
    let full = Host { ledger_client: false, ledger_host: Some("nexus".to_owned()) };
    assert_eq!(route(&words("--to main"), "git@github.com:tompassarelli/north.git", &client()), None);
    assert_eq!(route(&words("--to main"), "/fixture/southern.git", &client()), None);
    assert_eq!(route(&words("--to other"), "/fixture/south.git", &client()), None);
    assert_eq!(route(&words("--tag v1"), "/fixture/south.git", &client()), None);
    assert_eq!(route(&words("--help"), "/fixture/south.git", &client()), None);
    assert_eq!(route(&words("--to main --force"), "/fixture/south.git", &client()), None);
    assert_eq!(route(&words("--to main"), "/fixture/south.git", &full), None);
}
