use crate::spend::operational;
use crate::spend::{Account, Rule, Starter, View, Window};

const NOW: f64 = 1_794_300_000.0;
const HOUR: f64 = 3600.0;

fn account(name: &str, five_hour: f64, week: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + 2.0 * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + 40.0 * HOUR) }), counted: true }
}

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The 2026-11-10 08:40 judges: accounts a, c and d were at their weekly limit and e at its five-hour
/// limit, so a judge that asked the shared accounts first met an empty pool. The reserve answers first.
#[test]
fn operational_callers_try_the_reserve_before_shared_capacity() {
    let policy: toml_edit::DocumentMut = "[accounts.claude]\nreserve = [\"r\"]\n".parse().unwrap();
    let reserve = operational::declared(&policy);
    assert_eq!(reserve, labels(&["r"]));
    assert_eq!(operational::order(&reserve, labels(&["a", "c", "d", "e", "r"])), labels(&["r", "a", "c", "d", "e"]), "the reserve is tried before every shared account");
    assert_eq!(operational::order(&[], labels(&["a", "c"])), labels(&["a", "c"]), "no reserve declared leaves the shared order");

    let view = View { at: NOW, accounts: vec![account("claude:a", 20.0, 100.0), account("claude:c", 20.0, 100.0), account("claude:d", 20.0, 100.0), account("claude:e", 100.0, 40.0), account("claude:r", 0.0, 10.0)], ..View::default() };
    for shared in ["claude:a", "claude:c", "claude:d", "claude:e"] {
        assert_eq!(view.admit(shared, Starter::Reserved).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit), "{shared} had nothing left for the judge");
    }
    assert_eq!(view.admit("claude:r", Starter::Reserved), Ok(()), "the judge is answered on the reserve");
}

/// 2026-11-12 (south#124): with every API organization meant to be held, the judge still fell back to the API pool,
/// a canary could start on the reserve organization from its own unit, and the proxy was never refused there.
#[test]
fn every_caller_is_refused_on_a_held_api_organization_before_a_request_leaves_and_a_plan_account_serves_it() {
    use crate::fleet::model::{Account as Login, ApiKey, ProviderName, Role};
    use crate::fleet::registry::reserve;
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
    for (role, caller, who) in [(Role::Worker, canary, "a canary"), (Role::Worker, None, "a spare fill"), (Role::Proxy, None, "the proxy"), (Role::Lead, None, "an owner session")] {
        let line = reserve::refusal(&rules, &login("api", true), role, None, caller);
        assert!(line.as_deref().is_some_and(|line| line.starts_with("api:fixture-org is held")), "{who} is refused on the held organization: {line:?}");
        assert_eq!(reserve::refusal(&rules, &login("a", false), role, None, caller), None, "{who} starts on a plan account");
    }

    let basis = Basis::new(&policy, NOW);
    assert!(basis.operational("claude:api:fixture-org").is_some_and(|why| why.contains("is held")), "placement offers the canary no API grant");
    assert_eq!(basis.operational("claude:a"), None, "placement offers the plan account");
    let view = crate::spend::view_from_config(&policy, dir.path(), &crate::spend::Rules { charge: 0.0, reading_max_h: 1.0 }, 0, NOW, |_| true);
    assert!(view.admit("claude:api:fixture-org", Starter::Reserved).is_err_and(|refusal| refusal.line.contains("is held")), "an operational caller is refused too");
    assert_eq!(view.admit("claude:a", Starter::Reserved), Ok(()));
}
