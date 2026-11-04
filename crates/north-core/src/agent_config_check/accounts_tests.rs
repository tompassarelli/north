use super::accounts::{Profiles, problems};

struct Hosts {
    missing: &'static [&'static str],
    tier: Option<&'static str>,
}

impl Profiles for Hosts {
    fn has_directory(&self, host: &str, path: &str) -> Option<bool> {
        Some(!self.missing.contains(&format!("{host}:{path}").as_str()))
    }

    fn tier(&self, _: &str, _: &str) -> Option<String> {
        self.tier.map(str::to_owned)
    }
}

const ORCHESTRATION: &str = r#"
[accounts.claude]
plans = { a = "max-20x", b = "max-5x" }
[accounts.claude.profiles.a]
nexus = { path = "~/p/claude-a" }
gex45 = { path = "~/p/claude-a" }
[accounts.claude.profiles.b]
nexus = { path = "~/p/claude-b" }
"#;

const COSTS: &str = "[[service]]\nid = \"claude-a\"\n[[service]]\nid = \"claude-b\"\n";

fn found(orchestration: &str, costs: &str, hosts: &Hosts) -> Vec<String> {
    problems(orchestration, costs, hosts)
}

const WHOLE: Hosts = Hosts { missing: &[], tier: None };

#[test]
fn a_complete_account_set_passes() {
    assert_eq!(found(ORCHESTRATION, COSTS, &WHOLE), Vec::<String>::new());
}

#[test]
fn an_account_without_a_costs_service_fails_naming_it() {
    let costs = "[[service]]\nid = \"claude-a\"\n";
    assert_eq!(found(ORCHESTRATION, costs, &WHOLE), ["accounts.claude.b has no costs.toml service claude-b"]);
}

#[test]
fn an_account_without_a_plans_entry_fails_naming_it() {
    let orchestration = ORCHESTRATION.replace(", b = \"max-5x\"", "");
    assert_eq!(found(&orchestration, COSTS, &WHOLE), ["accounts.claude.b has no accounts.claude.plans entry"]);
}

#[test]
fn a_plans_entry_that_differs_from_the_profiles_rate_limit_tier_fails() {
    let hosts = Hosts { missing: &[], tier: Some("max-5x") };
    let found = found(ORCHESTRATION, COSTS, &hosts);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().all(|line| line.starts_with("accounts.claude.a plans entry max-20x differs from the max-5x")), "{found:?}");
}

#[test]
fn a_declared_host_without_the_profile_directory_fails_naming_the_host() {
    let hosts = Hosts { missing: &["gex45:~/p/claude-a"], tier: None };
    assert_eq!(found(ORCHESTRATION, COSTS, &hosts), ["accounts.claude.a has no profile directory ~/p/claude-a on gex45"]);
}

#[test]
fn the_real_south_config_passes_when_it_is_present() {
    let Some(operator) = std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join("code/south/main/agents")) else { return };
    let (Ok(orchestration), Ok(costs)) = (std::fs::read_to_string(operator.join("orchestration.toml")), std::fs::read_to_string(operator.join("costs.toml"))) else { return };
    assert_eq!(found(&orchestration, &costs, &WHOLE), Vec::<String>::new());
}
