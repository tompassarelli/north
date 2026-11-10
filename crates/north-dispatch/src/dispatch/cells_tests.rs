use super::cells;

#[test]
fn the_arm_table_keeps_a_reason_per_exclusion_and_arms_every_other_cell_but_low_and_max() {
    assert!(cells::exclusions().iter().all(|row| !row.reason.trim().is_empty() && (row.model.is_some() || row.effort.is_some())));
    let feature = cells::allowed("feature");
    assert_eq!(feature.len(), 7, "{feature:?}");
    assert!(!feature.iter().any(|cell| ["low", "max"].contains(&cell.effort)));
    assert!(cells::excluded("feature", "haiku", "max").is_some() && cells::excluded("feature", "sonnet", "max").is_some());
    assert!(cells::excluded("feature", "sonnet", "medium").is_some() && cells::excluded("feature", "sonnet", "xhigh").is_some(), "a Sonnet session runs only at high effort");
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
    use super::cells::{self, LIVE, Role};
    use super::facets;
    use super::tier::{Evidence, History, route_for, seed_cell};
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let unread = BTreeMap::<String, Evidence>::new();
    let empty = BTreeMap::new();
    for category in crate::router::brief::CATEGORIES {
        let facet = |key: &str| if key == "Category" { category.to_owned() } else { facets::default(key).to_owned() };
        let seed = seed_cell(category);
        assert!(cells::allowed(category).contains(&seed), "{category} seeds on {}, which the arm table excludes", seed.name());
        let live = cells::live(category, seed, &empty);
        assert!(!live.is_empty() && live.len() <= LIVE, "{category}: {live:?}");
        let route = route_for("north#939", &facet, &History::default(), &unread).unwrap();
        let cell = route.cell.expect("a drawn cell");
        assert!(cells::allowed(category).contains(&cell), "{category} drew {}, which the arm table excludes", cell.name());
        let (model, effort) = route.model_effort(&cfg).unwrap();
        assert!(cells::excluded(category, &model, &effort).is_none(), "{category} routes to {model} at {effort}, which the arm table excludes");
        assert_eq!(live.iter().find(|live| live.cell == cell).map(|live| live.role), route.cell_role.map(|role| if role == "leader" { Role::Leader } else { Role::Challenger }));
    }

    let boxes = |usd: f64| vec![crate::router::settle::Outcome { usd, closed: true, kept: true }; 15];
    let outcomes = BTreeMap::from([("sonnet-high".to_owned(), boxes(10.0))]);
    let live = cells::live("feature", seed_cell("feature"), &outcomes);
    assert_eq!(live.iter().map(|live| live.cell.name()).collect::<Vec<_>>(), ["sonnet-high", "haiku-high", "opus-high"], "one evidenced cell brings in the two nearest untried cells, never a fourth");
    assert!(live[1..].iter().all(|live| live.role == Role::Challenger));

    let settled = BTreeMap::from([("sonnet-high".to_owned(), boxes(10.0)), ("haiku-high".to_owned(), boxes(20.0))]);
    let live = cells::live("feature", seed_cell("feature"), &settled);
    assert_eq!(live.len(), LIVE, "{live:?}");
    assert!(live.iter().all(|live| live.cell.name() != "haiku-high"), "a challenger settled behind the leader leaves and the next untried cell comes in: {live:?}");
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
    let entry = started.entry("gex45", "claude:b", "n939-d0000").unwrap();
    let cell = started.route.cell.expect("a drawn cell").name();
    assert_eq!((entry.cell.as_deref(), entry.cell_role.as_deref()), (Some(cell.as_str()), started.route.cell_role));
}
