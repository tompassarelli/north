use super::*;

const ORCHESTRATION: &str = r#"
[placement]
order = ["gex45", "nexus"]
public_repos = ["north", "wisp"]
private_repos = ["south"]
cloud_credit = "proton"

[hosts.whiterabbit]
serves = ["native", "private:south"]
reaches = ["gex45"]

[hosts.zeta]
ssh = "zeta.example"
resident_cap = 2
launches = true

[hosts.gex45]
ssh = "gex45"
serves = ["native"]
pushes = ["north"]
resident_cap = 6
launches = true
warcraft_clients = 4

[hosts.nexus]
ssh = "nexus"
resident_cap = 3

[accounts.claude.plans]
a = "max-20x"
b = "max-5x"
"#;

#[test]
fn the_recorded_config_rebuilds_a_document_placement_reads_the_same_as_the_file() {
    let cfg: DocumentMut = ORCHESTRATION.parse().unwrap();
    let recorded = config(&cfg, "sha".into());
    let rebuilt = document(&recorded);
    assert_eq!(config(&rebuilt, "sha".into()), recorded);
    assert_eq!(gate::machines(&rebuilt), gate::machines(&cfg));
    assert_eq!(gate::machines(&cfg).iter().map(|machine| machine.name.as_str()).collect::<Vec<_>>(), ["gex45", "nexus", "zeta"]);
    let brief = "Item: north#1 x\nSurface: rust\nNeeds: gex45\n";
    assert_eq!(gate::surface(brief, &rebuilt), gate::surface(brief, &cfg));
    let basis = |cfg| Basis::new(cfg, json!({}), 0.0).plan("claude:a");
    assert_eq!(basis(&rebuilt), basis(&cfg));
}

#[test]
fn the_recorded_usage_gives_the_gate_value_its_verdicts_read() {
    let gate = json!({"claude": {"controls": {
        "claude:a": {"windows": [{"window": "5h", "used_pct": 40, "burn_pct_h": 3.5}, {"window": "7d", "used_pct": 61.5, "resets_at": "2026-11-08T00:00:00Z", "reset_h": 50.0, "extra": 1}]},
        "codex:x": {"windows": []}
    }}});
    let controls = gate["claude"]["controls"].as_object().unwrap().iter().filter(|(name, _)| name.starts_with("claude:")).map(|(name, control)| (name.clone(), storage::rows(&control["windows"]).iter().map(window).collect())).collect();
    let usage = Usage { sha256: String::new(), age_s: None, controls };
    let rebuilt = gate_value(&usage);
    let roster = BTreeMap::from([("claude:a".to_owned(), north_strategy_types::placement::Standing::Listed)]);
    let cfg: DocumentMut = ORCHESTRATION.parse().unwrap();
    let basis = Basis::new(&cfg, json!({}), 1_793_000_000.0);
    assert_eq!(gate::verdicts_of(&rebuilt, &roster, &basis), gate::verdicts_of(&gate, &roster, &basis));
}

