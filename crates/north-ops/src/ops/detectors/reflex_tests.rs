use std::cell::RefCell;
use std::fs;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitStatus, Output};
use std::time::Duration;

use super::slow_test::PROFILE_MARK;
use super::tick;
use crate::ops::flow::{Host, Places};

const STRACE: &str = "% time     seconds  usecs/call     calls    errors syscall\n------ ----------- ----------- --------- --------- ------------------\n 99.90   16.000500    16000500         1           clock_nanosleep\n  0.10    0.016000           3      3200           read\n------ ----------- ----------- --------- --------- ------------------\n100.00   16.016500                  3201           total\n";

#[derive(Default)]
struct Scripted {
    offloads: RefCell<Vec<Vec<String>>>,
    edits: RefCell<Vec<String>>,
}

fn answer(stdout: &str) -> Option<Output> {
    Some(Output { status: ExitStatus::from_raw(0), stdout: stdout.as_bytes().to_vec(), stderr: Vec::new() })
}

impl Host for Scripted {
    fn output(&self, argv: &[&str], _: Option<&Path>, input: Option<&str>, _: Duration) -> Option<Output> {
        match argv {
            ["git", "grep", ..] => answer("crates/north-core/src/slow.rs\ntests/it/other.rs\ntests/it/slow.rs\n"),
            ["git", "rev-parse", "origin/main"] => answer("c88cebc6a54ba3f362a9be84f7d4eec17cdee9b7\n"),
            ["north", "capacity", "offload", "run", ..] => {
                self.offloads.borrow_mut().push(argv.iter().map(|word| (*word).to_owned()).collect());
                answer(&format!("cargo output\n{PROFILE_MARK}\n{STRACE}"))
            }
            ["gh", "issue", "view", ..] => answer(r#"{"body":"Done when:\n- [ ] an older box","state":"OPEN"}"#),
            ["gh", "issue", "edit", ..] => {
                self.edits.borrow_mut().push(input.unwrap_or_default().to_owned());
                answer("")
            }
            _ => None,
        }
    }
}

fn places(root: &Path) -> Places {
    let places = Places { home: root.join("home"), state: root.join("state"), ledger: root.join("threads.db") };
    let checkout = places.checkout("north");
    fs::create_dir_all(checkout.join("docs/rationale")).unwrap();
    fs::write(checkout.join("docs/rationale/speed.md"), "# Speed\n\nOwner: north#777\n\nThe tests live in `tests/it/slow.rs`.\n").unwrap();
    places
}

fn write_run(places: &Places, sha: &str, slow: &[(&str, f64)], finished: bool) {
    let logs = places.state.join("north/land-queue/q1/logs");
    fs::create_dir_all(&logs).unwrap();
    let mut log = "    Starting tests across 1 binary\n        PASS [   0.020s] (  1/2) north::it quick::one\n".to_owned();
    for (name, seconds) in slow {
        log.push_str(&format!("        PASS [{seconds:8.3}s] (  2/2) north::it {name}\n"));
    }
    fs::write(logs.join(format!("post-land-{sha}.log")), log).unwrap();
    if finished {
        fs::write(logs.join(format!("post-land-{sha}.outcome")), "").unwrap();
    }
    fs::write(logs.join(format!("post-land-{sha}-on-{sha}0.log")), "        PASS [  99.000s] (  1/1) north::it never::read\n").unwrap();
    fs::write(logs.join(format!("post-land-{sha}-on-{sha}0.outcome")), "").unwrap();
}

struct FailsFor(Scripted, &'static str);

impl Host for FailsFor {
    fn output(&self, argv: &[&str], cwd: Option<&Path>, input: Option<&str>, limit: Duration) -> Option<Output> {
        if argv.get(1) == Some(&"capacity") && argv.last().is_some_and(|command| command.contains(self.1)) {
            return None;
        }
        self.0.output(argv, cwd, input, limit)
    }
}

#[test]
fn a_test_over_the_line_in_three_after_landing_runs_gets_one_profiled_box_on_its_owner_and_never_a_second() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let host = Scripted::default();
    let slow = |seconds| [("slow::sixteen_seconds", seconds)];
    write_run(&places, "aaaaaaaaaaaa", &slow(16.2), true);
    write_run(&places, "ffffffffffff", &[], true);
    write_run(&places, "bbbbbbbbbbbb", &slow(16.1), true);
    write_run(&places, "uuuuuuuuuuuu", &[], false);
    let waiting = tick(&places, &host, 1.0, true);
    assert!(waiting.iter().all(|line| !line.contains("slow-test:")), "{waiting:?}");
    assert!(host.offloads.borrow().is_empty());

    write_run(&places, "cccccccccccc", &slow(16.3), true);
    let lines = tick(&places, &host, 2.0, true);
    assert!(lines.iter().any(|line| line.contains("slow-test slow-test:north::it:slow::sixteen_seconds: filed a box on north#777")), "{lines:?}");
    let offloads = host.offloads.borrow();
    assert_eq!(offloads.len(), 1);
    assert!(offloads[0].windows(2).any(|pair| pair == ["--host", "ccx33"]));
    assert!(offloads[0].last().unwrap().contains("-p north --test it -- --exact slow::sixteen_seconds"));
    let edits = host.edits.borrow();
    assert_eq!(edits.len(), 1);
    let box_line = edits[0].lines().last().unwrap();
    assert!(box_line.starts_with("- [ ] **Speed up the slow test slow::sixteen_seconds.** `north::it slow::sixteen_seconds` in tests/it/slow.rs took 16.2 s, 16.1 s, 16.3 s in its newest 3 after-landing runs, over the 15 s line;"), "{box_line}");
    assert!(box_line.contains("clock_nanosleep 99.9% 16.00 s in 1 calls"), "{box_line}");
    assert!(box_line.contains(" Done when: `north::it slow::sixteen_seconds` passes in at most 15 s in each of 3 later after-landing runs"), "{box_line}");
    assert!(box_line.ends_with("(detector slow-test:north::it:slow::sixteen_seconds)"));
    drop((offloads, edits));

    let again = tick(&places, &host, 3.0, true);
    assert!(again.iter().all(|line| !line.contains("filed a box")), "{again:?}");
    assert_eq!((host.offloads.borrow().len(), host.edits.borrow().len()), (1, 1));
    a_dry_run_spends_no_slot_a_failed_profile_does_not_starve_the_next_test_and_a_healed_test_is_left_alone();
}

fn a_dry_run_spends_no_slot_a_failed_profile_does_not_starve_the_next_test_and_a_healed_test_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let places = places(root.path());
    let host = FailsFor(Scripted::default(), "alpha_slow");
    for sha in ["aaaaaaaaaaaa", "bbbbbbbbbbbb", "cccccccccccc"] {
        write_run(&places, sha, &[("slow::alpha_slow", 20.0), ("slow::beta_slow", 20.0), ("slow::healed", 20.0)], true);
    }
    write_run(&places, "dddddddddddd", &[("slow::alpha_slow", 20.0), ("slow::beta_slow", 20.0), ("slow::healed", 2.0)], true);
    let dry = tick(&places, &host, 1.0, false);
    assert!(dry.iter().any(|line| line.starts_with("detectors: slow-test slow-test:north::it:slow::alpha_slow:")), "{dry:?}");
    assert!(dry.iter().all(|line| !line.contains("healed")), "{dry:?}");
    assert!(host.0.offloads.borrow().is_empty() && host.0.edits.borrow().is_empty());

    let first = tick(&places, &host, 2.0, true);
    assert!(first.iter().all(|line| !line.contains("filed a box")), "{first:?}");
    let second = tick(&places, &host, 3.0, true);
    assert!(second.iter().any(|line| line.contains("slow-test:north::it:slow::beta_slow: filed a box")), "{second:?}");
    assert_eq!(host.0.offloads.borrow().len(), 1);
}
