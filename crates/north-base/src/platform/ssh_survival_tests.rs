use std::process::{Command, Stdio};
use std::time::Duration;

use super::super::ssh_fixture::{master_words, Fixture};
use super::super::wait;

fn caller_status(fixture: &Fixture, started: &std::path::Path) -> std::process::Child {
    let words = fixture.client(&["-o", "BatchMode=yes", "testhost", &format!("touch {}; sleep 4", started.display())]);
    Command::new(&words[0]).args(&words[1..]).stdout(Stdio::null()).spawn().unwrap()
}

#[test]
fn a_multiplexed_session_survives_restarting_the_unit_that_made_the_first_connection() {
    let mut fixture = Fixture::new();
    let master = master_words(&fixture);
    fixture.unit("north-ssh-master-test", &master);
    fixture.wait_for_socket();

    let first = fixture.client(&["-o", "BatchMode=yes", "testhost", "sleep", "60"]);
    let first_unit = fixture.unit("north-ssh-first-test", &first);
    let started = fixture.dir.path().join("second-started");
    let mut second = caller_status(&fixture, &started);
    assert!(wait::until(Duration::from_secs(30), || started.exists()), "the second caller never started its remote command");
    fixture.stop(&first_unit);

    let status = second.wait().unwrap();
    assert_eq!(status.code(), Some(0), "the multiplexed session must outlive the first caller's unit");
}

