use super::{Org, declared, open};
use crate::credit::{Grant, with_credit_left};
use std::collections::BTreeMap;
use std::path::Path;

const POLICY: &str = "[accounts.claude.api_orgs.proton]\nid = \"p\"\nkeys = { gex45 = [\"/k/p\"] }\n[accounts.claude.api_orgs.gmail]\nid = \"g\"\nkeys = { gex45 = [\"/k/g\"] }\n";

fn grant(usd: f64, expires: &str) -> Grant {
    Grant { usd, expires: expires.to_owned() }
}

// KEEP(bug): an api org whose first grant is spent still ranked by that grant's date, so a later grant was drawn before an untouched sooner one (north#737)
#[test]
fn the_pool_opens_the_org_whose_soonest_grant_with_credit_left_expires_first() {
    let proton = [grant(199.99, "2026-11-08"), grant(200.0, "2026-12-08")];
    let spent = [("2026-11-06".to_owned(), 150.0), ("2026-11-07".to_owned(), 66.23)];
    let today = "2026-11-07";
    let open_grants: BTreeMap<String, Vec<Grant>> = [("p".to_owned(), with_credit_left(&proton, &spent, today)), ("g".to_owned(), with_credit_left(&[grant(200.0, "2026-11-12")], &[], today))].into();
    let orgs = declared(&POLICY.parse().unwrap(), &open_grants, Path::new("/home"), "gex45");
    let first = |orgs: &[Org]| open(orgs, today, |_| false, |_| true).map(|(org, _)| org.name.clone());
    assert_eq!(first(&orgs), Some("gmail".to_owned()));
    let all_spent: BTreeMap<String, Vec<Grant>> = [("p".to_owned(), Vec::new()), ("g".to_owned(), Vec::new())].into();
    assert_eq!(first(&declared(&POLICY.parse().unwrap(), &all_spent, Path::new("/home"), "gex45")), None);
}
