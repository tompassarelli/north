use crate::spend::operational;
use crate::spend::{Account, Placed, Rule, Rules, Starter, View, Window};

const NOW: f64 = 1_794_300_000.0;
const HOUR: f64 = 3600.0;

fn account(name: &str, five_hour: f64, week: f64) -> Account {
    Account { name: name.into(), plan_week: 1.0, five_hour: Some(Window { used_pct: five_hour, resets_at: Some(NOW + 2.0 * HOUR) }), week: Some(Window { used_pct: week, resets_at: Some(NOW + 40.0 * HOUR) }), counted: true }
}

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The 2026-11-10 08:40 judges: accounts a, c and d were at their weekly limit and e at its five-hour
/// limit, so a judge that asked the shared accounts first met an empty pool. The reserve answers first,
/// and no worker may start inside the floor that keeps the shared accounts usable for the judge.
#[test]
fn operational_callers_try_the_reserve_before_shared_capacity() {
    let policy: toml_edit::DocumentMut = "[accounts.claude]\nreserve = [\"r\"]\n".parse().unwrap();
    let reserve = operational::declared(&policy);
    assert_eq!(reserve, labels(&["r"]));
    assert_eq!(operational::order(&reserve, labels(&["a", "c", "d", "e", "r"])), labels(&["r", "a", "c", "d", "e"]), "the reserve is tried before every shared account");
    assert_eq!(operational::order(&[], labels(&["a", "c"])), labels(&["a", "c"]), "no reserve declared leaves the shared order");

    let rules = Rules::configured();
    let view = View { at: NOW, accounts: vec![account("claude:a", 20.0, 100.0), account("claude:c", 20.0, 100.0), account("claude:d", 20.0, 100.0), account("claude:e", 100.0, 40.0), account("claude:r", 0.0, 10.0), account("claude:f", 0.0, 96.0)], ..View::default() };
    for shared in ["claude:a", "claude:c", "claude:d", "claude:e"] {
        assert_eq!(view.admit(&rules, shared, Starter::Reserved, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::ProviderLimit), "{shared} had nothing left for the judge");
    }
    assert_eq!(view.admit(&rules, "claude:r", Starter::Reserved, Placed::default()), Ok(()), "the judge is answered on the reserve");
    assert_eq!(view.admit(&rules, "claude:f", Starter::Worker, Placed::default()).map_err(|refusal| refusal.rule), Err(Rule::Floor), "a worker stops at the floor of an account the judge may still use");
    assert_eq!(view.admit(&rules, "claude:f", Starter::Reserved, Placed::default()), Ok(()));
}
