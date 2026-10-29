use super::expectations::{self, Direction, Expectation, Roster};
use super::facts::{Machine, write_heartbeat};
use super::{Breaches, Facts, Notify, SOURCE, tick};
use std::cell::Cell;

const NOW: f64 = 1_792_000_000.0;

struct Fixed(Cell<f64>);

impl Facts for Fixed {
    fn now(&self) -> f64 {
        NOW
    }

    fn measure(&self, _: &Expectation) -> Result<f64, String> {
        Ok(self.0.get())
    }
}

struct Unread;

impl Facts for Unread {
    fn now(&self) -> f64 {
        NOW
    }

    fn measure(&self, _: &Expectation) -> Result<f64, String> {
        Err("no ledger".into())
    }
}

#[derive(Default)]
struct Sent {
    phone: Vec<(String, String)>,
    proxy: Vec<(String, String)>,
    phone_down: bool,
}

impl Notify for Sent {
    fn phone(&mut self, title: &str, text: &str) -> Result<(), String> {
        if self.phone_down {
            return Err("nexus unreachable".into());
        }
        self.phone.push((title.into(), text.into()));
        Ok(())
    }

    fn proxy(&mut self, session: &str, text: &str) -> Result<(), String> {
        self.proxy.push((session.into(), text.into()));
        Ok(())
    }
}

fn shipped() -> Roster {
    expectations::parse(SOURCE).unwrap()
}

fn breaching_value(expectation: &Expectation) -> f64 {
    match expectation.direction {
        Direction::Below => expectation.threshold - 1.0,
        Direction::Above => expectation.threshold + 1.0,
    }
}

fn only(roster: &Roster, name: &str) -> Roster {
    Roster { proxy: roster.proxy.clone(), expectations: roster.expectations.iter().filter(|one| one.name == name).cloned().collect() }
}

#[test]
fn every_shipped_expectation_replays_a_breach_once_and_a_clear_once() {
    for expectation in shipped().expectations {
        let roster = only(&shipped(), &expectation.name);
        let facts = Fixed(Cell::new(breaching_value(&expectation)));
        let (mut sent, mut breaches) = (Sent::default(), Breaches::new());
        for _ in 0..3 {
            let outcome = tick(&roster, &expectation.on, &facts, &mut sent, &mut breaches, false);
            assert!(!outcome.failed, "{}", expectation.name);
        }
        assert_eq!(sent.phone.len(), 1, "{} pushed the phone once for a standing breach", expectation.name);
        assert_eq!(sent.proxy.len(), 1, "{} messaged the proxy once", expectation.name);
        assert!(sent.phone[0].0.contains("BREACH") && sent.phone[0].0.contains(&expectation.name));
        assert!(sent.phone[0].1.contains(&expectation.owner) && sent.phone[0].1.contains(&expectation.fix));
        assert_eq!(sent.proxy[0].0, "proxy");
        assert!(breaches.contains_key(&expectation.name));

        facts.0.set(expectation.threshold);
        for _ in 0..3 {
            tick(&roster, &expectation.on, &facts, &mut sent, &mut breaches, false);
        }
        assert_eq!(sent.phone.len(), 2, "{} cleared once", expectation.name);
        assert!(sent.phone[1].0.contains("cleared") && sent.proxy[1].1.contains("cleared"));
        assert!(breaches.is_empty());
    }
}

#[test]
fn a_dry_run_prints_every_state_and_sends_and_records_nothing() {
    let roster = shipped();
    let facts = Fixed(Cell::new(0.0));
    let (mut sent, mut breaches) = (Sent::default(), Breaches::new());
    let outcome = tick(&roster, "nexus", &facts, &mut sent, &mut breaches, true);
    assert_eq!(outcome.lines.iter().filter(|line| line.contains("not checked here")).count(), 1);
    assert!(outcome.lines.iter().any(|line| line.starts_with("cloud-lane-use: BREACH cloud_launches 0 < 1 over 24h")));
    assert!(outcome.lines.iter().any(|line| line.starts_with("nexus-heartbeat: ok")));
    assert!(sent.phone.is_empty() && sent.proxy.is_empty() && breaches.is_empty());
}

#[test]
fn a_breach_the_phone_missed_still_reaches_the_proxy_once_and_fails_the_tick() {
    let roster = only(&shipped(), "launch-failures");
    let facts = Fixed(Cell::new(9.0));
    let (mut sent, mut breaches) = (Sent { phone_down: true, ..Sent::default() }, Breaches::new());
    let first = tick(&roster, "nexus", &facts, &mut sent, &mut breaches, false);
    let second = tick(&roster, "nexus", &facts, &mut sent, &mut breaches, false);
    assert!(first.failed && first.lines.iter().any(|line| line.contains("phone push failed: nexus unreachable")));
    assert!(!second.failed);
    assert_eq!((sent.phone.len(), sent.proxy.len(), breaches.len()), (0, 1, 1));
}

#[test]
fn a_breach_neither_channel_carried_is_tried_again_next_tick() {
    struct Down;
    impl Notify for Down {
        fn phone(&mut self, _: &str, _: &str) -> Result<(), String> {
            Err("down".into())
        }
        fn proxy(&mut self, _: &str, _: &str) -> Result<(), String> {
            Err("down".into())
        }
    }
    let roster = only(&shipped(), "launch-failures");
    let mut breaches = Breaches::new();
    let outcome = tick(&roster, "nexus", &Fixed(Cell::new(9.0)), &mut Down, &mut breaches, false);
    assert!(outcome.failed && breaches.is_empty());
}

#[test]
fn an_unread_measure_fails_the_tick_and_keeps_the_standing_breach() {
    let roster = only(&shipped(), "closed-boxes");
    let mut breaches = Breaches::from([("closed-boxes".to_owned(), NOW - 600.0)]);
    let mut sent = Sent::default();
    let outcome = tick(&roster, "nexus", &Unread, &mut sent, &mut breaches, false);
    assert!(outcome.failed && outcome.lines[0].contains("unread (no ledger)"));
    assert!(sent.phone.is_empty() && breaches.len() == 1);
}

#[test]
fn an_expectation_past_its_until_date_is_retired_and_forgotten() {
    let roster = only(&shipped(), "cloud-lane-use");
    assert!(roster.expectations[0].until.is_some_and(|until| until < NOW + 86_400.0 * 400.0));
    let late = Fixed(Cell::new(0.0));
    struct Later(Fixed);
    impl Facts for Later {
        fn now(&self) -> f64 {
            2_000_000_000.0
        }
        fn measure(&self, expectation: &Expectation) -> Result<f64, String> {
            self.0.measure(expectation)
        }
    }
    let mut breaches = Breaches::from([("cloud-lane-use".to_owned(), NOW)]);
    let outcome = tick(&roster, "nexus", &Later(late), &mut Sent::default(), &mut breaches, false);
    assert_eq!(outcome.lines, ["cloud-lane-use: retired"]);
    assert!(breaches.is_empty());
}

#[test]
fn the_roster_refuses_an_unknown_measure_a_repeated_name_and_a_missing_fix() {
    let one = "[notify]\nproxy = \"proxy\"\n[[expectation]]\nname = \"a\"\nmeasure = \"cloud_launches\"\nbreach_when = \"below\"\nthreshold = 1\nwindow = \"1h\"\nowner = \"o\"\nfix = \"f\"\non = \"nexus\"\n";
    assert!(expectations::parse(one).is_ok());
    assert!(expectations::parse(&one.replace("cloud_launches", "weather")).unwrap_err().contains("not one of"));
    assert!(expectations::parse(&format!("{one}{}", &one[one.find("[[").unwrap()..])).unwrap_err().contains("listed twice"));
    assert!(expectations::parse(&one.replace("fix = \"f\"\n", "")).unwrap_err().contains("fix is missing"));
    assert!(expectations::parse(&one.replace("1h", "1w")).unwrap_err().contains("must end in m, h or d"));
}

fn record_line(at: f64, cloud: bool, launched: bool) -> String {
    format!("{{\"at\":{at},\"item\":\"north#1\",\"host\":\"nexus\",\"account\":\"claude:a\",\"name\":\"n\",\"cloud\":{cloud},\"launched\":{launched},\"detail\":\"\"}}\n")
}

#[test]
fn the_machine_counts_cloud_launches_and_failures_in_the_window_and_ages_the_heartbeat() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("dispatch")).unwrap();
    let lines = [record_line(NOW - 100.0, true, true), record_line(NOW - 100.0, false, true), record_line(NOW - 100.0, true, false), record_line(NOW - 90_000.0, true, true)].concat();
    std::fs::write(directory.path().join("dispatch/launches.jsonl"), lines).unwrap();
    write_heartbeat(directory.path(), "nexus", NOW - 600.0).unwrap();
    let machine = Machine { directory: directory.path().into(), host: "nexus".into(), at: NOW };
    let measured = |name: &str| machine.measure(shipped().expectations.iter().find(|one| one.name == name).unwrap());
    assert_eq!(measured("nexus-heartbeat"), Ok(10.0));
    assert_eq!(measured("cloud-lane-use"), Ok(1.0));
    assert_eq!(measured("launch-failures"), Ok(1.0));
}
