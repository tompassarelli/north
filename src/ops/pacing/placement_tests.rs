use super::placement;
use serde_json::Value;

#[test]
fn recorded_placement_preserves_account_order_floors_tiers_and_host_restrictions() {
    #[derive(serde::Deserialize)]
    struct Record {
        gate: Box<serde_json::value::RawValue>,
        #[serde(flatten)]
        fields: serde_json::Map<String, Value>,
    }
    let cases: Vec<Record> =
        serde_json::from_str(include_str!("fixtures/placement-replay.json")).unwrap();
    for record in cases {
        let account_order = super::gate_order::accounts(record.gate.get()).unwrap();
        let gate: Value = serde_json::from_str(record.gate.get()).unwrap();
        let case = Value::Object(record.fields);
        let mut defaults = placement::routing_defaults();
        for (category, class) in case["classes"].as_object().unwrap() {
            defaults
                .codex_route
                .insert(category.clone(), class["route"].as_str().unwrap().into());
            defaults.codex_tier.insert(
                category.clone(),
                serde_json::from_value(class["tiers"].clone()).unwrap(),
            );
        }
        let cfg = case["config"].as_str().unwrap().parse().unwrap();
        let at = super::target::seconds(case["now"].as_str().unwrap()).unwrap();
        let actual = placement::spawn(
            case["brief"].as_str().unwrap(),
            &case["spawner"],
            &gate,
            &cfg,
            at,
            placement::Probes {
                configured: case["probe"].clone(),
                observed: case["observations"].clone(),
            },
            placement::Routing {
                local: case["local"].as_str().unwrap(),
                defaults: &defaults,
                account_order: &account_order,
            },
        );
        assert_eq!(actual, case["expected"], "{}", case["name"]);
    }
}
