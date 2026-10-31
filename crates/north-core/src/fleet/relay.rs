use std::fs;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};

use super::remote_view::Snapshot;
use super::ssh::{over_ssh, over_ssh_within};

const REVIVE_WAIT: Duration = Duration::from_secs(25);

pub trait Courier: Sync {
    fn snapshot(&self, host: &str) -> NorthResult<Snapshot>;
    /// Runs again the sessions the host's reboot stranded, returning one line per session.
    fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>>;
}

pub struct Ssh;

impl Courier for Ssh {
    fn snapshot(&self, host: &str) -> NorthResult<Snapshot> {
        Ok(serde_json::from_slice(&over_ssh(host, &["fleet", "outbox", "snapshot"])?)?)
    }

    fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>> {
        let output = over_ssh_within(host, &["fleet", "rebooted", "--revive"], None, REVIVE_WAIT)?;
        Ok(String::from_utf8_lossy(&output).lines().map(str::to_owned).collect())
    }
}

#[derive(Default)]
pub struct Relayed {
    pub unreachable: Vec<(String, String)>,
    pub snapshots: Vec<(String, Snapshot)>,
    /// The lines each reachable host gave for the sessions its reboot stranded and it ran again.
    pub revived: Vec<(String, String)>,
}

pub fn relay(courier: &dyn Courier, hosts: &[String]) -> Relayed {
    let mut relayed = Relayed::default();
    let mut reachable = Vec::new();
    for (host, outcome) in hosts.iter().zip(each(hosts, |host| courier.snapshot(host))) {
        match outcome {
            Ok(snapshot) => {
                reachable.push(host.clone());
                relayed.snapshots.push((host.clone(), snapshot));
            }
            Err(error) => relayed.unreachable.push((host.clone(), format!("its sessions and gates were not read: {}", error.user_message()))),
        }
    }
    for (host, outcome) in reachable.iter().zip(each(&reachable, |host| courier.revive_rebooted(host))) {
        match outcome {
            Ok(lines) => relayed.revived.extend(lines.into_iter().map(|line| (host.clone(), line))),
            Err(error) => relayed.unreachable.push((host.clone(), format!("its rebooted sessions were not revived: {}", error.user_message()))),
        }
    }
    relayed
}

fn each<I: Sync, T: Send>(items: &[I], call: impl Fn(&I) -> NorthResult<T> + Sync) -> Vec<NorthResult<T>> {
    std::thread::scope(|scope| {
        let calls: Vec<_> = items.iter().map(|item| scope.spawn(|| call(item))).collect();
        calls.into_iter().map(|call| call.join().unwrap_or_else(|_| Err(NorthError::Usage("a relay call to a host panicked".into())))).collect()
    })
}

impl Relayed {
    pub fn lines(&self) -> Vec<String> {
        let revived = self.revived.iter().map(|(host, line)| format!("{host}: {line}"));
        let unreachable = self.unreachable.iter().map(|(host, reason)| format!("unreachable {host}: {reason}"));
        revived.chain(unreachable).collect()
    }
}

pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Hosts {
        up: Vec<&'static str>,
        wait: Duration,
    }

    impl Courier for Hosts {
        fn snapshot(&self, host: &str) -> NorthResult<Snapshot> {
            std::thread::sleep(self.wait);
            if !self.up.contains(&host) {
                return Err(NorthError::Usage(format!("ssh {host}: unreachable")));
            }
            Ok(Snapshot { host: host.into(), ..Snapshot::default() })
        }

        fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>> {
            std::thread::sleep(self.wait);
            Ok(vec![format!("resumed {host}-worker")])
        }
    }

    #[test]
    fn a_host_that_answers_is_read_and_asked_to_revive_its_rebooted_sessions_and_a_host_that_does_not_is_not() {
        let hosts = Hosts { up: vec!["far"], wait: Duration::ZERO };

        let relayed = relay(&hosts, &["far".to_owned(), "down".to_owned()]);

        assert_eq!(relayed.snapshots.iter().map(|(host, _)| host.as_str()).collect::<Vec<_>>(), ["far"]);
        assert_eq!(relayed.revived, [("far".to_owned(), "resumed far-worker".to_owned())]);
        assert_eq!(relayed.lines(), ["far: resumed far-worker".to_owned(), "unreachable down: its sessions and gates were not read: ssh down: unreachable".to_owned()]);
    }

    #[test]
    fn hosts_that_each_answer_slowly_are_relayed_within_one_wait_for_reading_and_one_for_reviving() {
        let names = ["nexus", "gex45", "far", "near"];
        let hosts: Vec<String> = names.map(str::to_owned).into();
        let begun = std::time::Instant::now();
        let relayed = relay(&Hosts { up: names.into(), wait: Duration::from_millis(400) }, &hosts);
        assert!(begun.elapsed() < Duration::from_millis(1500), "{:?}", begun.elapsed());
        assert_eq!(relayed.revived.iter().map(|(host, _)| host.as_str()).collect::<Vec<_>>(), names);
        assert!(relayed.unreachable.is_empty());
    }
}
