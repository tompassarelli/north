use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn at(home: &str) -> impl Fn() -> Option<String> + '_ {
    move || Some(home.to_owned())
}

fn session_trust(path: Option<&Path>) -> bool {
    path.is_some_and(super::sealed)
}

#[test]
fn guards_stay_on_without_trusted_proof() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().display().to_string();
    let get = |name: &str| (name == "HOME").then(|| home.clone());
    assert!(!super::guards_off_in("north-hook", &get), "a session with no proof turned the pre-bash guards off");

    let current = Path::new(&home).join(".local/state/north/agents/current");
    std::fs::create_dir_all(&current).unwrap();
    let digest = format!("sha256:{}", "a".repeat(64));
    let record = serde_json::json!({
        "schema": "north.agent-activation/v1",
        "catalogDigest": digest,
        "generationId": digest,
        "units": [{"kind": "hook", "id": "north-hook", "permission": "off", "active": false}],
    });
    std::fs::write(current.join("activation.json"), record.to_string()).unwrap();
    assert!(!super::guards_off("north-hook", &get, &session_trust, &at(&home)), "proof the session could write turned the pre-bash guards off");

    let switched = |name: &str| match name {
        "HOME" => Some(home.clone()),
        "AGENT_NO_AUTHORING_HOOKS" => Some("1".to_owned()),
        _ => None,
    };
    assert!(!super::guards_off("north-hook", &switched, &session_trust, &at(&home)), "a setting the session could make turned the pre-bash guards off");
    std::fs::write(current.join("activation.active"), "").unwrap();
    assert!(!super::guards_off("north-hook", &get, &session_trust, &at(&home)), "a list the session could write turned the pre-bash guards off");

    let sealed_elsewhere = |path: Option<&Path>| path.is_some();
    let unwritable = Path::new(&home).join("unwritable.active");
    std::fs::write(&unwritable, "hook other-hook\n").unwrap();
    let bare = tempfile::tempdir().unwrap();
    let bare = bare.path().display().to_string();
    let moved = |name: &str| match name {
        "HOME" => Some(bare.clone()),
        "NORTH_AGENT_ACTIVE" => Some(unwritable.display().to_string()),
        _ => None,
    };
    assert!(!super::guards_off("north-hook", &moved, &sealed_elsewhere, &at(&bare)), "a proof location chosen by the session turned the pre-bash guards off");

    let foreign = tempfile::tempdir().unwrap();
    let foreign = foreign.path().display().to_string();
    let foreign_current = Path::new(&foreign).join(".local/state/north/agents/current");
    std::fs::create_dir_all(&foreign_current).unwrap();
    std::fs::write(foreign_current.join("activation.json"), serde_json::json!({
        "schema": "north.agent-activation/v1",
        "catalogDigest": digest,
        "generationId": digest,
        "units": [{"kind": "hook", "id": "north-hook", "permission": "off", "active": false}],
    }).to_string()).unwrap();
    let pointed = |name: &str| (name == "HOME").then(|| foreign.clone());
    assert!(!super::guards_off("north-hook", &pointed, &sealed_elsewhere, &at(&bare)), "a home chosen by the session turned the pre-bash guards off");

    let other = tempfile::tempdir().unwrap();
    let other = other.path().display().to_string();
    let current = Path::new(&other).join(".local/state/north/agents/current");
    std::fs::create_dir_all(&current).unwrap();
    let record = serde_json::json!({
        "schema": "north.agent-activation/v1",
        "catalogDigest": digest,
        "generationId": digest,
        "units": [{"kind": "hook", "id": "north-hook", "permission": "on", "active": true}],
    });
    std::fs::write(current.join("activation.json"), record.to_string()).unwrap();
    let renamed = |name: &str| match name {
        "HOME" => Some(other.clone()),
        "NORTH_HOOK_ID" => Some("unlisted-hook".to_owned()),
        _ => None,
    };
    assert!(!super::guards_off("north-hook", &renamed, &sealed_elsewhere, &at(&other)), "a hook name chosen by the session turned the pre-bash guards off");

    let own = tempfile::tempdir().unwrap();
    let own_home = own.path().display().to_string();
    let chain = Path::new(&own_home).join(".local/state/north/agents/current");
    std::fs::create_dir_all(&chain).unwrap();
    std::fs::write(chain.join("activation.json"), serde_json::json!({
        "schema": "north.agent-activation/v1",
        "catalogDigest": digest,
        "generationId": digest,
        "units": [{"kind": "hook", "id": "north-hook", "permission": "off", "active": false}],
    }).to_string()).unwrap();
    let steps: Vec<_> = chain.ancestors().take_while(|step| step.starts_with(&own_home)).map(Path::to_path_buf).collect();
    for step in &steps {
        std::fs::set_permissions(step, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    let read_only_and_ours = |path: Option<&Path>| path.is_some_and(|path| path.ancestors().take_while(|step| step.starts_with(&own_home)).all(super::foreign));
    let owned = |name: &str| (name == "HOME").then(|| own_home.clone());
    let answer = super::guards_off("north-hook", &owned, &read_only_and_ours, &at(&own_home));
    for step in &steps {
        std::fs::set_permissions(step, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    assert!(!answer, "a record the session owns and made read-only turned the pre-bash guards off");
}
