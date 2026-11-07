use super::held_back;
use crate::foundation::clock::Timestamp;
use super::week::Week;

fn week(resets: i64, readings: &[(i64, f64)]) -> Week {
    Week { resets, readings: readings.to_vec() }
}

/// Two usage sources read the same window a point apart (north#793): a lower readout after a higher one is not a drop and refill, so each flap is not spent again.
#[test]
fn a_lower_readout_from_another_source_is_not_spent_twice() {
    let week = week(1_000_000, &[(100, 90.0), (160, 91.0), (170, 90.0), (220, 91.0), (280, 93.0)]);
    assert_eq!(week.spent(150, 300), 3.0);
}

/// North#793: the provider refused an org for credit while the ledger counted less spend than the provider billed (north#792), so its credit was reported held back; a refusal closes the org, one with credit and no refusal still counts.
#[test]
fn credit_on_an_org_the_provider_refused_is_not_held_back() {
    let dir = tempfile::tempdir().unwrap();
    let policy = dir.path().join("orchestration.toml");
    std::fs::write(&policy, "[accounts.claude.api_orgs.proton]\nid = \"p\"\n[accounts.claude.api_orgs.gmail]\nid = \"g\"\n").unwrap();
    std::fs::write(dir.path().join("claude-accounts.toml"), "[[account]]\n[account.api_org]\nid = \"p\"\ngrants = [{ usd = 100.0, expires = \"2099-12-31\" }]\n[[account]]\n[account.api_org]\nid = \"g\"\ngrants = [{ usd = 40.0, expires = \"2099-12-31\" }]\n").unwrap();
    let at = Timestamp::from_unix_seconds(1_794_000_000);
    assert_eq!(held_back(&policy, dir.path(), at), 140.0);
    north_pace::credit::marks::mark(&dir.path().join("north/api-exhausted"), "p", "north-1", &at.iso_date(), None).unwrap();
    assert_eq!(held_back(&policy, dir.path(), at), 40.0);
}
