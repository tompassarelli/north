use super::*;
use serde_json::json;

#[test]
fn killswitch_matches_override_list_and_activation_json() {
    let dir = tempfile::tempdir().unwrap();
    let list = dir.path().join("activation.active");
    let path = dir.path().join("activation.json");
    let mut env =
        std::collections::BTreeMap::from([("NORTH_AGENT_ACTIVE", list.display().to_string())]);
    let off = |env: &std::collections::BTreeMap<&str, String>| {
        guards_off("fixture.sh", &|name| env.get(name).cloned())
    };
    assert!(off(&env));
    for (value, expected) in [
        ("0", false),
        ("false", false),
        ("1", true),
        ("true", true),
        ("FALSE", true),
        (" ", true),
    ] {
        env.insert("AGENT_NO_AUTHORING_HOOKS", value.into());
        assert_eq!(off(&env), expected);
    }
    env.remove("AGENT_NO_AUTHORING_HOOKS");
    std::fs::write(&list, "hook other\nhook fixture").unwrap();
    assert!(!off(&env));
    std::fs::write(&list, "hook fixture\r\n").unwrap();
    assert!(off(&env));
    std::fs::remove_file(&list).unwrap();
    let mut activation = json!({"schema":"north.agent-activation/v1", "catalogDigest":format!("sha256:{}", "a".repeat(64)), "generationId":format!("sha256:{}", "0".repeat(64)), "units":[{"kind":"hook", "id":"fixture", "permission":"on", "active":true}]});
    std::fs::write(&path, activation.to_string()).unwrap();
    assert!(!off(&env));
    activation["units"][0]["active"] = json!(false);
    std::fs::write(&path, activation.to_string()).unwrap();
    assert!(off(&env));
    activation["units"][0]["active"] = json!(true);
    let duplicate = activation["units"][0].clone();
    activation["units"].as_array_mut().unwrap().push(duplicate);
    std::fs::write(&path, activation.to_string()).unwrap();
    assert!(off(&env));
    std::fs::write(&path, "bad").unwrap();
    assert!(off(&env));
}
