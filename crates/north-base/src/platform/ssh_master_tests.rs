use std::ffi::OsString;
use std::process::Command;
use std::time::Duration;

use super::super::ssh::OWN_CONNECTION;
use super::super::ssh_fixture::{master_words, which, Fixture};
use super::{probe_with, reopen_with, socket, Dial, Health, Master, Process, SLOW};

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

struct Counting {
    through: Vec<bool>,
}

impl Dial for Counting {
    fn round_trip(&mut self, words: &[OsString], limit: Duration) -> Option<Duration> {
        let answer = Process.round_trip(words, limit);
        if !words.iter().any(|word| word == OWN_CONNECTION) && limit == SLOW {
            self.through.push(answer.is_some());
        }
        answer
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

    let mut dial = Counting { through: Vec::new() };
    let health = probe_with(&mut dial, &program, &env, "testhost", &mut Respawned { fixture: &mut fixture, marker, words });

    assert_eq!(dial.through[..2], [false, false], "the stalled master was not sampled twice before it was recycled: {:?}", dial.through);
    let Health::Recycled { after: Some(_), .. } = health else { panic!("the stalled master was not recycled: {health:?}") };
    assert!(!std::path::Path::new(&format!("/proc/{stalled}")).exists(), "the stalled master {stalled} still runs");
}

struct Scripted {
    answers: Vec<Option<Duration>>,
}

impl Dial for Scripted {
    fn round_trip(&mut self, _words: &[OsString], _limit: Duration) -> Option<Duration> {
        self.answers.remove(0)
    }
}

struct Forbidden;

impl Master for Forbidden {
    fn stop(&mut self, host: &str) -> Result<(), String> {
        panic!("killed the master for {host} while a direct connection was as slow")
    }

    fn start(&mut self, host: &str) -> Result<(), String> {
        panic!("restarted the master for {host}")
    }
}

#[test]
fn a_master_is_left_alone_while_a_direct_connection_is_nearly_as_slow() {
    let mut dial = Scripted { answers: vec![None, Some(Duration::from_millis(2600)), None, None] };
    let health = probe_with(&mut dial, &[which("ssh").into()], &Default::default(), "testhost", &mut Forbidden);
    assert!(matches!(health, Health::Loaded { .. }), "{health:?}");
}

#[test]
fn a_dead_or_missing_master_is_started_and_answers_before_the_call_is_retried_but_a_live_master_is_left() {
    let mut fixture = Fixture::new();
    let words = master_words(&fixture);
    let marker = fixture.unit("north-ssh-master-live", &words);
    fixture.wait_for_socket();
    let env = fixture.env();
    let program = [which("ssh").into()];
    let socket = socket(&program, &env, "testhost").unwrap();
    let mut master = Respawned { fixture: &mut fixture, marker, words };

    assert_eq!(reopen_with(&program, &env, "testhost", &mut master), Ok(false));
    for missing in [false, true] {
        master.stop("testhost").unwrap();
        assert!(socket.exists());
        if missing {
            std::fs::remove_file(&socket).unwrap();
        }
        assert_eq!(reopen_with(&program, &env, "testhost", &mut master), Ok(true));
        assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok(), "the reopened master did not answer");
    }
}
