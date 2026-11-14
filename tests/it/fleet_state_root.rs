use std::path::Path;
use std::process::{Command, Output};

fn north(home: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
        .args(args)
        .env("HOME", home)
        .env("XDG_STATE_HOME", state)
        .env_remove("NORTH_FLEET_SESSION")
        .env_remove("THREADS_DB")
        .output()
        .unwrap()
}

#[test]
fn a_gate_of_a_scratch_fleet_is_booked_under_its_xdg_state_root_and_leaves_the_home_ledger_unchanged() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let state = scratch.path().join("state");
    std::fs::create_dir_all(&home).unwrap();
    let session = state.join("north/fleet/scratch-fleet");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(session.join("session.json"), r#"{"name":"scratch-fleet","provider":"claude","account":"a","cwd":"/tmp","created_ms":0}"#).unwrap();
    let home_ledger = home.join(".local/state/north/ledger.db");
    let state_ledger = state.join("north/ledger.db");
    let home_before = std::fs::read(&home_ledger).ok();

    let opened = north(&home, &state, &["gate", "open", "--session", "scratch-fleet", "--question", "cap the lander?", "--option", "yes", "--option", "no", "--recommend", "1", "--default", "1", "--default-after", "4h"]);
    assert!(opened.status.success(), "gate open failed: {}", String::from_utf8_lossy(&opened.stderr));
    let answered = north(&home, &state, &["gate", "answer", "1", "1"]);
    assert!(answered.status.success(), "gate answer failed: {}", String::from_utf8_lossy(&answered.stderr));

    let answer: String = rusqlite::Connection::open(&state_ledger)
        .unwrap()
        .query_row("SELECT answer FROM gates WHERE gate_id = 1", [], |row| row.get(0))
        .unwrap();
    assert_eq!(answer, "yes");
    assert_eq!(std::fs::read(&home_ledger).ok(), home_before, "the scratch fleet wrote into the ledger under HOME");
}
