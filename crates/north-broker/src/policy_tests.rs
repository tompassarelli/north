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
fn the_ceiling_names_a_refused_permission_adds_metadata_and_never_grants_workflows_by_default() {
    let error = ceiling().scope("agent", &["north".to_owned()], &asked(&[("contents", "write"), ("issues", "admin")])).unwrap_err();
    assert!(error.contains("issues:admin") && error.contains("issues:write"), "{error}");
    assert!(ceiling().scope("agent", &["north".to_owned()], &asked(&[("actions", "read")])).unwrap_err().contains("actions"));
    let scope = ceiling().scope("agent", &["north".to_owned()], &asked(&[("contents", "read")])).unwrap();
    assert_eq!(scope.permissions, asked(&[("contents", "read"), ("metadata", "read")]));
    assert!(ceiling().scope("agent", &["other".to_owned()], &Permissions::new()).unwrap_err().contains("other"));
    assert!(ceiling().scope("agent", &[], &Permissions::new()).unwrap_err().contains("must name its repositories"));
    let scope = ceiling().scope("agent", &["south".to_owned()], &Permissions::new()).unwrap();
    assert_eq!(scope.permissions, asked(&[("contents", "write"), ("issues", "write"), ("metadata", "read")]));
    let open = Ceiling::default();
    assert!(open.scope("agent", &[], &asked(&[("workflows", "write")])).unwrap_err().contains("workflows"));
    assert_eq!(open.scope("agent", &[], &Permissions::new()).unwrap().permissions, Permissions::new());
    let lander = "[[caller]]\nuid = 7\n[caller.github.lander]\nrepositories = [\"north\"]\n";
    assert!(Policy::parse(lander).unwrap_err().contains("no permissions list"));
    let user = Policy::for_user(7);
    let scope = user.caller(7).unwrap().github["lander"].scope("lander", &[], &Permissions::new()).unwrap();
    assert_eq!(scope.permissions, asked(&[("contents", "write"), ("metadata", "read"), ("workflows", "write")]));
}

#[test]
fn the_allowlist_matches_one_segment_per_star_refuses_traversal_and_takes_a_query_only_where_the_rule_names_it() {
    let rules = Rules::new(&Policy::parse(POLICY).unwrap().caller(1001).unwrap().proxy["digitalocean"]).unwrap();
    assert!(rules.allows("GET", "/v2/droplets", "") && rules.allows("GET", "/v2/droplets/42", "") && rules.allows("POST", "/v2/droplets/42/actions", ""));
    assert!(rules.allows("GET", "/v2/files/a/b", ""));
    assert!(!rules.allows("DELETE", "/v2/droplets/42", "") && !rules.allows("GET", "/v2/account", "") && !rules.allows("POST", "/v2/droplets", ""));
    for sneaky in ["/v2/droplets/../account", "/v2/droplets//42/actions", "/v2/droplets/%2e%2e/account", "/v2/droplets/a%2Fb", "/v2/files", "/v2/droplets\\x", "/v2/droplets/{..}/actions", "/v2/droplets/[1-9]", "/v2/droplets/..;x/actions", "/v2/files/{..}/{..}/v2/account/keys"] {
        assert!(!rules.allows("GET", sneaky, ""), "{sneaky}");
    }
    let rules = Rules::new(&["GET /v2/a".to_owned(), "GET /v2/b?page=1".to_owned(), "GET /v2/c?*".to_owned()]).unwrap();
    assert!(rules.allows("GET", "/v2/a", "") && !rules.allows("GET", "/v2/a", "page=1"));
    assert!(rules.allows("GET", "/v2/b", "page=1") && !rules.allows("GET", "/v2/b", "page=2") && !rules.allows("GET", "/v2/b", ""));
    assert!(rules.allows("GET", "/v2/c", "per_page=5&page=2") && !rules.allows("GET", "/v2/c", "x={1,2}") && !rules.allows("GET", "/v2/c", "x=%zz"));
}
