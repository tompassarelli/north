use std::process::Command;
use std::time::Duration;

use super::super::ssh_fixture::{master_words, Fixture};
use super::super::wait;
use super::{retried, Link, Reply, Rerun};

const BANNER: &str = "Connection timed out during banner exchange\n";

fn scripted(replies: &mut Vec<(Option<i32>, &'static str)>, links: &mut Vec<Link>, link: Link) -> Reply<Option<i32>> {
    links.push(link);
    let (exit, said) = replies.remove(0);
    Reply { value: exit, exit, said: said.to_owned() }
}

#[test]
fn a_read_whose_shared_master_was_killed_under_it_is_run_again_on_a_fresh_connection() {
    let mut fixture = Fixture::new();
    let master = fixture.unit("north-ssh-master-killed-under-a-read", &master_words(&fixture));
    fixture.wait_for_socket();
    let pid = fixture.pid(&master);
    let started = fixture.dir.path().join("read-started");
    let mut attempts = Vec::new();

    let exit = std::thread::scope(|scope| {
        let killer = scope.spawn(|| {
            assert!(wait::until(Duration::from_secs(120), || started.exists()), "the read never started");
            assert!(Command::new("kill").args(["-KILL", &pid.to_string()]).status().unwrap().success());
        });
        let exit = retried("testhost", Rerun::Anywhere, |link| {
            attempts.push(link);
            let command = if attempts.len() == 1 { format!("touch {}; sleep 60", started.display()) } else { "true".to_owned() };
            let mut words = fixture.client(&["-o", "BatchMode=yes"]);
            words.extend(link.options().iter().map(|word| (*word).to_owned()));
            words.extend(["testhost".to_owned(), command]);
            let output = Command::new(&words[0]).args(&words[1..]).output().unwrap();
            Reply { value: output.status.code(), exit: output.status.code(), said: String::from_utf8_lossy(&output.stderr).into_owned() }
        });
        killer.join().unwrap();
        exit
    });

    assert_eq!(exit, Some(0), "the read was given up after the master died under it");
    assert_eq!(attempts, [Link::Shared, Link::Fresh]);
}

#[test]
fn a_command_that_may_have_run_is_not_sent_twice_but_one_that_never_left_is() {
    let mut links = Vec::new();
    let mut after_send = vec![(Some(255), ""), (Some(0), "")];
    assert_eq!(retried("host", Rerun::BeforeSend, |link| scripted(&mut after_send, &mut links, link)), Some(255));
    assert_eq!(links, [Link::Shared]);

    let mut links = Vec::new();
    let mut before_send = vec![(Some(255), BANNER), (Some(0), "")];
    assert_eq!(retried("host", Rerun::BeforeSend, |link| scripted(&mut before_send, &mut links, link)), Some(0));
    assert_eq!(links, [Link::Shared, Link::Fresh]);
}

#[test]
fn an_outage_that_outlasts_the_one_retry_still_fails_with_the_second_exit() {
    let mut links = Vec::new();
    let mut replies = vec![(Some(255), BANNER), (Some(255), BANNER), (Some(0), "")];
    assert_eq!(retried("host", Rerun::Anywhere, |link| scripted(&mut replies, &mut links, link)), Some(255));
    assert_eq!(links.len(), 2);

    let mut links = Vec::new();
    let mut replies = vec![(Some(1), BANNER), (Some(0), "")];
    assert_eq!(retried("host", Rerun::Anywhere, |link| scripted(&mut replies, &mut links, link)), Some(1));
    assert_eq!(links.len(), 1);
}
