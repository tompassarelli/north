use north_pace::spend::Starter;

const NOW: f64 = 1_794_300_000.0;

/// 2026-11-12 (south#124): with every API organization meant to be held, the judge still fell back to the API pool,
/// a canary could start on the reserve organization from its own unit, and the coordinator was never refused there;
/// then an unreadable policy read as no hold, and a fleet session's own plan profile settings went unchecked.
#[test]
fn every_caller_is_refused_on_a_held_api_organization_before_a_request_leaves_and_a_plan_account_serves_it() {
    use north_fleet_supervisor::fleet::model::{Account as Login, ApiKey, ProviderName, Role};
    use north_fleet_supervisor::fleet::registry::reserve;
    use crate::pacing::placement::room::Basis;
    use north_providers::providers::claude::api_pool;

    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("fixture-key");
    std::fs::write(&key, "fixture-secret").unwrap();
    let text = format!(
        "[accounts.claude]\nreserve = [\"api:fixture-org\"]\napi_hold = {{ since = \"2026-11-12\", reason = \"fixture hold\" }}\n\n[accounts.claude.api_orgs.fixture-org]\nid = \"fixture-id\"\nkeys = {{ fixture-host = [\"{}\"] }}\n",
        key.display()
    );
    let path = dir.path().join("orchestration.toml");
    std::fs::write(&path, &text).unwrap();
    let policy: toml_edit::DocumentMut = text.parse().unwrap();

    assert!(api_pool::next_key(&path, dir.path(), &dir.path().join("state"), "fixture-host", &[]).is_none(), "the judge's API fallback and every one-shot ask get no key");

    let login = |label: &str, api: bool| Login { provider: ProviderName::new("claude"), label: label.to_owned(), home: dir.path().to_owned(), token_file: None, api: api.then(|| ApiKey { org: "fixture-org".to_owned(), file: key.clone() }) };
    let rules = reserve::declared(&path);
    let canary = Some("north-runtime-follow.service");
    for (role, caller, who) in [(Role::Worker, canary, "a canary"), (Role::Worker, None, "a spare fill"), (Role::Coordinator, None, "the coordinator"), (Role::Lead, None, "an owner session")] {
        let line = reserve::refusal(&rules, &login("api", true), role, None, caller);
        assert!(line.as_deref().is_some_and(|line| line.starts_with("api:fixture-org is held")), "{who} is refused on the held organization: {line:?}");
        assert_eq!(reserve::refusal(&rules, &login("a", false), role, None, caller), None, "{who} starts on a plan account");
    }

    let basis = Basis::new(&policy, NOW);
    assert!(basis.operational("claude:api:fixture-org").is_some_and(|why| why.contains("is held")), "placement offers the canary no API grant");
    assert_eq!(basis.operational("claude:a"), None, "placement offers the plan account");
    let view = north_pace::spend::view_from_config(&policy, dir.path(), &north_pace::spend::Rules { charge: 0.0, reading_max_h: 1.0, money_ceiling: 100.0, claude_sessions: 0 }, 0, NOW, |_| true);
    assert!(view.admit("claude:api:fixture-org", Starter::Reserved).is_err_and(|refusal| refusal.line.contains("is held")), "an operational caller is refused too");
    assert_eq!(view.admit("claude:a", Starter::Reserved), Ok(()));

    let unparseable = dir.path().join("unparseable.toml");
    std::fs::write(&unparseable, "[accounts.claude\n").unwrap();
    for broken in [unparseable, dir.path().join("missing.toml")] {
        let line = reserve::refusal(&reserve::declared(&broken), &login("api", true), Role::Coordinator, None, None);
        assert!(line.as_deref().is_some_and(|line| line.contains("cannot be read")), "a policy that cannot be read holds every API organization: {line:?}");
        let spawn = north_pace::spend::view_from_policy(&broken, dir.path(), &north_pace::spend::Rules { charge: 0.0, reading_max_h: 1.0, money_ceiling: 100.0, claude_sessions: 0 }, 0, NOW, |_| true);
        assert!(spawn.admit("claude:api:fixture-org", Starter::Worker).is_err_and(|refusal| refusal.line.contains("cannot be read")), "the subagent spawn view holds it too");
    }

    let billed = tempfile::tempdir().unwrap();
    std::fs::write(billed.path().join("settings.json"), r#"{"env":{"ANTHROPIC_BASE_URL":"https://fixture.invalid"}}"#).unwrap();
    let plan = Login { home: billed.path().to_owned(), ..login("b", false) };
    let line = reserve::refusal(&rules, &plan, Role::Worker, None, None);
    assert!(line.as_deref().is_some_and(|line| line.contains("ANTHROPIC_BASE_URL")), "a fleet session's own plan profile settings are checked like a one-shot ask's: {line:?}");
}
