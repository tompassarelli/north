use std::process::Command;
use std::time::{Duration, Instant};

use super::super::ssh_fixture::{master_words, which, Fixture};
use super::{probe, Health, Master, SLOW};

struct Respawned<'a> {
    fixture: &'a mut Fixture,
    marker: String,
    words: Vec<String>,
}

impl Master for Respawned<'_> {
    fn stop(&mut self, _host: &str) -> Result<(), String> {
        self.fixture.stop(&self.marker);
        Ok(())
    }

    fn start(&mut self, _host: &str) -> Result<(), String> {
        self.marker = self.fixture.unit("north-ssh-master-respawned", &self.words);
        Ok(())
    }
}

#[test]
fn a_master_slower_than_three_seconds_while_a_direct_connection_answers_is_closed_and_recreated() {
    let mut fixture = Fixture::new();
    let words = master_words(&fixture);
    let marker = fixture.unit("north-ssh-master-stalled", &words);
    fixture.wait_for_socket();
    let stalled = fixture.pid(&marker);
    assert!(Command::new("kill").args(["-STOP", &stalled.to_string()]).status().unwrap().success());
    let env = fixture.env();
    let program = [which("ssh").into()];

    let started = Instant::now();
    let health = probe(&program, &env, "testhost", &mut Respawned { fixture: &mut fixture, marker, words });

    assert!(started.elapsed() >= 2 * SLOW, "the stalled master was not sampled twice: {health:?}");
    let Health::Recycled { direct, after: Some(after) } = health else { panic!("the stalled master was not recycled: {health:?}") };
    assert!(direct < SLOW, "{direct:?}");
    assert!(after < Duration::from_secs(1), "the recreated master took {after:?}");
    assert!(!std::path::Path::new(&format!("/proc/{stalled}")).exists(), "the stalled master {stalled} still runs");
}
