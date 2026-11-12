use super::*;

const KILLS: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-land-scans/src/test_admission/,src/cli/ --no-build land scan test-admission kills --remote-north \"CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-land-scans/src/test_admission/,src/cli/\" ${SAFE_PUSH_LAND_BASE}..${SAFE_PUSH_LAND_HEAD}";
const KILLS_REWRITTEN: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-land-scans/src/test_admission/,src/cli/ land scan test-admission kills --remote-north \"CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-land-scans/src/test_admission/,src/cli/\" ${SAFE_PUSH_LAND_BASE}..${SAFE_PUSH_LAND_HEAD}";
const UNDECLARED: &str = "CARGO_TARGET_DIR=target ./bin/head-north --code crates/north-land-scans/src/gate_shadow.rs,src/cli/ --no-build land scan gate-shadow ${SAFE_PUSH_LAND_BASE}..${SAFE_PUSH_LAND_HEAD}";

#[test]
fn a_local_only_entry_with_no_command_is_refused_with_its_line() {
    let config = Config { deadlines: vec![format!("900{SEPARATOR}{KILLS}")], ..Config::default() };
    assert_eq!(config.deadline_seconds(KILLS), Some(900));
    assert_eq!(config.deadline_seconds(KILLS_REWRITTEN), Some(900));
    assert_eq!(config.deadline_seconds(UNDECLARED), None);

    let malformed = Config { local_only: vec!["firn needs the host store".to_owned()], ..Config::default() };
    let refusal = malformed.commands(Path::new("."), "", "HEAD").unwrap_err();
    assert!(refusal.contains("land.local-only: firn needs the host store"), "{refusal}");
}
