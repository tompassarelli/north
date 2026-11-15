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

    for category in crate::router::brief::CATEGORIES {
        for scope in ["cross-module", "cross-repo"] {
            let facet = |key: &str| match key {
                "Category" => category.to_owned(),
                "Scope" => scope.to_owned(),
                "Verify" => "live-run".to_owned(),
                other => super::facets::default(other).to_owned(),
            };
            for number in 0..40 {
                let route = route_for(&format!("north#{number}"), &facet, &History::default(), &unread).unwrap();
                let cell = route.cell.expect("a drawn cell");
                assert_ne!(cell.model, "haiku", "{category} at Scope: {scope} drew {}", cell.name());
            }
        }
    }
    assert!(cells::excluded(cells::Shape { category: "tooling", scope: "module", verify: "live-run" }, "haiku", "high").is_none());

    let boxes = |usd: f64| vec![crate::router::settle::Outcome { usd, closed: true, kept: true }; 15];
    let outcomes = BTreeMap::from([("sonnet-high".to_owned(), boxes(10.0))]);
    let live = cells::live("feature", seed_cell("feature"), &outcomes);
    assert_eq!(live.iter().map(|live| live.cell.name()).collect::<Vec<_>>(), ["sonnet-high", "haiku-high", "opus-high"], "one evidenced cell brings in the two nearest untried cells, never a fourth");
    assert!(live[1..].iter().all(|live| live.role == Role::Challenger));

    let settled = BTreeMap::from([("sonnet-high".to_owned(), boxes(10.0)), ("haiku-high".to_owned(), boxes(20.0))]);
    let live = cells::live("feature", seed_cell("feature"), &settled);
    assert_eq!(live.len(), LIVE, "{live:?}");
    assert!(live.iter().all(|live| live.cell.name() != "haiku-high"), "a challenger settled behind the leader leaves and the next untried cell comes in: {live:?}");

    let haiku_led = BTreeMap::from([("sonnet-high".to_owned(), boxes(10.0)), ("haiku-high".to_owned(), boxes(2.0)), ("haiku-xhigh".to_owned(), boxes(1.0)[..1].to_vec())]);
    let live = cells::live("tooling", seed_cell("tooling"), &haiku_led);
    assert_eq!(live[0].cell.name(), "haiku-high", "{live:?}");
    assert!(live.iter().any(|live| live.cell == seed_cell("tooling") && live.role == Role::Challenger), "the starting cell stays live beside a Haiku leader that settled ahead of it: {live:?}");
}
