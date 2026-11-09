use super::cells;

#[test]
fn the_arm_table_keeps_a_reason_per_exclusion_and_arms_every_other_cell_but_low_and_max() {
    assert!(cells::exclusions().iter().all(|row| !row.reason.trim().is_empty() && (row.model.is_some() || row.effort.is_some())));
    let feature = cells::allowed("feature");
    assert_eq!(feature.len(), 9, "{feature:?}");
    assert!(!feature.iter().any(|cell| ["low", "max"].contains(&cell.effort)));
    assert!(cells::excluded("feature", "haiku", "max").is_some() && cells::excluded("feature", "sonnet", "max").is_some());
    assert_eq!(cells::allowed(""), feature);
    for category in ["planning", "review", "performance", "netcode-determinism", "debugging-unknown-cause"] {
        assert!(!cells::allowed(category).iter().any(|cell| cell.model == "haiku"), "{category}");
    }
    assert!(cells::excluded("planning", "claude-haiku-5-5", "high").is_some());
    assert!(cells::excluded("mechanical", "haiku", "high").is_none());
    let line = cells::table_line();
    assert!(cells::allowed("").iter().all(|cell| line.contains(&cell.name())) && !line.contains("low") && !line.contains("max"), "{line}");
}

#[test]
fn every_category_routes_to_a_cell_the_arm_table_keeps() {
    use super::facets;
    use super::tier::{History, route_for};
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let unread = BTreeMap::new();
    for category in crate::router::brief::CATEGORIES {
        let facet = |key: &str| if key == "Category" { category.to_owned() } else { facets::default(key).to_owned() };
        let route = route_for("north#939", &facet, &History::default(), &unread).unwrap();
        let (model, effort) = route.model_effort(&cfg).unwrap();
        assert!(cells::excluded(category, &model, &effort).is_none(), "{category} routes to {model} at {effort}, which the arm table excludes");
    }
}

#[test]
fn a_model_and_effort_name_the_cell_and_a_hand_launch_records_it() {
    use super::manual::{Ask, start};
    use super::tier::History;
    use std::collections::BTreeMap;
    assert_eq!(cells::named("claude-sonnet-5-5", "high").as_deref(), Some("sonnet-high"));
    assert_eq!(cells::named("claude-sonnet-5-5", "max").as_deref(), Some("sonnet-max"));
    assert_eq!(cells::named("claude-sonnet-5-5", "impossible"), None);
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let brief = "Item: north#939\nCategory: feature";
    let ask = Ask { item: "north#939", brief, prompt: &format!("{brief}\n\nDo the box."), model: None, reason: None, worker: true, places: false };
    let started = start(&ask, "", &cfg, &History::default(), &BTreeMap::new(), &|_| super::grounds::Grounds::default()).unwrap();
    assert_eq!(started.entry("gex45", "claude:b", "n939-d0000").unwrap().cell.as_deref(), Some("sonnet-high"));
}
