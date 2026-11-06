use super::{Host, HostFacts, Route, route, run};

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_owned).collect()
}

fn client() -> Host {
    Host { ledger_client: true, ledger_host: Some("nexus".to_owned()) }
}

#[test]
fn remote_land_routes_a_south_landing_to_main_on_a_ledger_client() {
    let routed = Route { ledger_host: Some("nexus".to_owned()), dry_run: false };
    for origin in ["git@github.com:tompassarelli/south.git", "https://github.com/tompassarelli/south", "/fixture/south.git"] {
        assert_eq!(route(&words("--to main"), origin, &client()), Some(Route { ledger_host: routed.ledger_host.clone(), dry_run: routed.dry_run }), "{origin}");
    }
    assert_eq!(route(&words("--dry-run --to main"), "/fixture/south.git", &client()), Some(Route { ledger_host: Some("nexus".to_owned()), dry_run: true }));
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

#[test]
fn remote_land_answers_its_help_probe_and_refuses_an_unknown_verb() {
    let host = HostFacts { ledger_client: true };
    assert_eq!(run(&words("--help"), &host), 0);
    assert_eq!(run(&words("land"), &host), 2);
    assert_eq!(run(&[], &host), 2);
}
