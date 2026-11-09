use super::*;

const POLICY: &str = r#"
[[caller]]
uid = 1001
[caller.github.agent]
repositories = ["north", "south"]
permissions = { contents = "write", issues = "write" }
[caller.proxy]
digitalocean = ["GET /v2/droplets", "GET /v2/droplets/*", "POST /v2/droplets/*/actions", "GET /v2/files/**"]
"#;

fn ceiling() -> Ceiling {
    Policy::parse(POLICY).unwrap().caller(1001).unwrap().github["agent"].clone()
}

fn asked(pairs: &[(&str, &str)]) -> Permissions {
    pairs.iter().map(|(name, level)| ((*name).to_owned(), (*level).to_owned())).collect()
}

#[test]
fn a_permission_above_the_ceiling_is_refused_with_the_permission_named() {
    let error = ceiling().scope("agent", &["north".to_owned()], &asked(&[("contents", "write"), ("issues", "admin")])).unwrap_err();
    assert!(error.contains("issues:admin") && error.contains("issues:write"), "{error}");
    let error = ceiling().scope("agent", &["north".to_owned()], &asked(&[("actions", "read")])).unwrap_err();
    assert!(error.contains("actions"), "{error}");
}

#[test]
fn a_scoped_read_token_always_carries_metadata_read_and_a_repository_outside_the_ceiling_is_refused() {
    let scope = ceiling().scope("agent", &["north".to_owned()], &asked(&[("contents", "read")])).unwrap();
    assert_eq!(scope.permissions, asked(&[("contents", "read"), ("metadata", "read")]));
    assert!(ceiling().scope("agent", &["other".to_owned()], &Permissions::new()).unwrap_err().contains("other"));
    assert!(ceiling().scope("agent", &[], &Permissions::new()).unwrap_err().contains("must name its repositories"));
}

#[test]
fn a_request_without_permissions_gets_the_whole_ceiling_and_workflows_is_never_granted_by_default() {
    let scope = ceiling().scope("agent", &["south".to_owned()], &Permissions::new()).unwrap();
    assert_eq!(scope.permissions, asked(&[("contents", "write"), ("issues", "write"), ("metadata", "read")]));
    let open = Ceiling::default();
    assert!(open.scope("agent", &[], &asked(&[("workflows", "write")])).unwrap_err().contains("workflows"));
    assert_eq!(open.scope("agent", &[], &Permissions::new()).unwrap().permissions, Permissions::new());
}

#[test]
fn the_allowlist_matches_one_segment_per_star_and_refuses_traversal() {
    let rules = Rules::new(&Policy::parse(POLICY).unwrap().caller(1001).unwrap().proxy["digitalocean"]).unwrap();
    assert!(rules.allows("GET", "/v2/droplets") && rules.allows("GET", "/v2/droplets/42") && rules.allows("POST", "/v2/droplets/42/actions"));
    assert!(rules.allows("GET", "/v2/files/a/b"));
    assert!(!rules.allows("DELETE", "/v2/droplets/42") && !rules.allows("GET", "/v2/account") && !rules.allows("POST", "/v2/droplets"));
    for sneaky in ["/v2/droplets/../account", "/v2/droplets//42/actions", "/v2/droplets/%2e%2e/account", "/v2/droplets/a%2Fb", "/v2/files", "/v2/droplets\\x"] {
        assert!(!rules.allows("GET", sneaky), "{sneaky}");
    }
}
