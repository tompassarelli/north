use super::remote_land::ledger_client;

#[test]
fn remote_land_reads_ledger_client_from_this_hosts_table_only() {
    let config: toml_edit::DocumentMut = "[hosts.gex45]\nledger_client = true\n[hosts.nexus]\nsettles_fleet = true\n[hosts.laptop]\nledger_client = \"yes\"\n".parse().unwrap();
    assert!(ledger_client(&config, "gex45"));
    assert!(!ledger_client(&config, "nexus"));
    assert!(!ledger_client(&config, "laptop"));
    assert!(!ledger_client(&config, "absent"));
}
