use std::collections::BTreeMap;
use std::time::Duration;

use super::{Hosts, deliver};

struct Tunnel {
    clock: Duration,
    answers_from: BTreeMap<&'static str, Duration>,
    copies: Vec<(String, Duration)>,
    said: Vec<String>,
}

impl Hosts for Tunnel {
    fn copy(&mut self, host: &str) -> Result<(), String> {
        self.copies.push((host.to_owned(), self.clock));
        self.clock += Duration::from_secs(20);
        if self.clock >= self.answers_from[host] { Ok(()) } else { Err("ssh exited 255: Connection timed out during banner exchange".to_owned()) }
    }

    fn pause(&mut self, wait: Duration) {
        self.clock += wait;
    }

    fn elapsed(&self) -> Duration {
        self.clock
    }

    fn say(&mut self, line: String) {
        self.said.push(line);
    }
}

fn tunnel(cpx62_answers_from: u64, cpx62b_answers_from: u64) -> Tunnel {
    let at = |seconds| Duration::from_secs(seconds);
    Tunnel { clock: at(0), answers_from: BTreeMap::from([("gex45", at(0)), ("cpx62", at(cpx62_answers_from)), ("cpx62b", at(cpx62b_answers_from))]), copies: Vec::new(), said: Vec::new() }
}

#[test]
fn a_failed_token_copy_to_a_build_host_is_retried_until_the_host_has_a_fresh_token() {
    let hosts: Vec<String> = ["gex45", "cpx62", "cpx62b"].map(str::to_owned).to_vec();
    let window = Duration::from_secs(27 * 60);
    let pause = Duration::from_secs(10);

    let mut run = tunnel(200, 150);
    assert!(deliver(&hosts, window, pause, &mut run).is_empty());
    let copied = |host: &str, run: &Tunnel| run.copies.iter().filter(|(copied, _)| copied == host).count();
    assert_eq!(copied("gex45", &run), 1);
    assert!(copied("cpx62", &run) > 1 && copied("cpx62b", &run) > 1);
    let last_cpx62 = run.copies.iter().rfind(|(host, _)| host == "cpx62").unwrap().1;
    assert!(last_cpx62 < Duration::from_secs(200 + 120), "cpx62 answered at 200 s and was copied at {last_cpx62:?}");
    assert!(run.said.iter().any(|line| line.contains("cpx62b") && line.contains("banner exchange") && line.contains("attempt 1")));

    let mut down = tunnel(40 * 60, 0);
    assert_eq!(deliver(&hosts, window, pause, &mut down), ["cpx62"]);
    assert!(down.clock < window + Duration::from_secs(60), "retrying went on to {:?}, past the window", down.clock);
    assert_eq!(copied("cpx62b", &down), 1);
}
