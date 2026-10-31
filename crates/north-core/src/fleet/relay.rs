use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::platform::process::run_within;

use super::gates::GateRequest;
use super::model::Delivery;
use super::remote_view::Snapshot;
use super::store::{FleetRow, RowKind};
use super::runtime::{SESSION_ENV, binary, new_uuid, now_ms};

const RELAY: &str = "relay";
const LETTER: &str = "json";
const SSH_WAIT: Duration = Duration::from_secs(20);
const REVIVE_WAIT: Duration = Duration::from_secs(25);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Letter {
    pub id: String,
    pub to: String,
    pub text: String,
    pub now: bool,
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<Box<GateRequest>>,
    pub at_ms: u64,
}

impl Letter {
    pub fn delivery(&self) -> Delivery {
        if self.now { Delivery::Now } else { Delivery::Queued }
    }

    pub fn signer(&self) -> String {
        self.sender.as_ref().map_or_else(|| self.from.clone(), |sender| format!("{sender}@{}", self.from))
    }
}

impl From<&FleetRow> for Letter {
    fn from(row: &FleetRow) -> Self {
        Self { id: row.id.clone(), to: row.to.clone(), text: row.text.clone(), now: row.urgent, from: row.from.clone(), sender: row.sender.clone(), gate: None, at_ms: row.sent_at }
    }
}

impl From<&Letter> for FleetRow {
    fn from(letter: &Letter) -> Self {
        Self { id: letter.id.clone(), kind: RowKind::Message, to: letter.to.clone(), from: letter.from.clone(), urgent: letter.now, sent_at: letter.at_ms, text: letter.text.clone(), sender: letter.sender.clone(), gate: None, job: None }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Relay {
    pub host: String,
    pub age: Duration,
}

pub struct Outbox {
    dir: PathBuf,
}

impl Outbox {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn relay(&self) -> Option<Relay> {
        let path = self.dir.join(RELAY);
        let host = fs::read_to_string(&path).ok()?.trim().to_owned();
        let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
        let age = SystemTime::now().duration_since(modified).unwrap_or_default();
        (!host.is_empty()).then_some(Relay { host, age })
    }

    pub fn post(&self, row: &FleetRow) -> NorthResult<Letter> {
        self.stage(Letter::from(row))
    }

    pub fn post_gate(&self, request: &GateRequest) -> NorthResult<Letter> {
        let at_ms = now_ms();
        let id = format!("{at_ms}-{}", new_uuid()?);
        self.stage(Letter { id, to: request.session.clone(), text: request.question.clone(), now: false, from: hostname(), sender: None, gate: Some(Box::new(request.clone())), at_ms })
    }

    fn stage(&self, letter: Letter) -> NorthResult<Letter> {
        fs::create_dir_all(&self.dir)?;
        let staged = self.dir.join(format!(".{}.tmp", letter.id));
        fs::write(&staged, serde_json::to_vec(&letter)?)?;
        fs::rename(staged, self.letter(&letter.id)?)?;
        Ok(letter)
    }

    pub fn take(&self, relay_host: &str) -> NorthResult<Vec<Letter>> {
        fs::create_dir_all(&self.dir)?;
        let staged = self.dir.join(format!(".{RELAY}.tmp"));
        fs::write(&staged, format!("{relay_host}\n"))?;
        fs::rename(staged, self.dir.join(RELAY))?;
        let mut paths: Vec<PathBuf> = fs::read_dir(&self.dir)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == LETTER))
            .collect();
        paths.sort();
        paths.iter().map(|path| Ok(serde_json::from_slice(&fs::read(path)?)?)).collect()
    }

    pub fn drop(&self, ids: &[String]) -> NorthResult<()> {
        for id in ids {
            match fs::remove_file(self.letter(id)?) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
                _ => {}
            }
        }
        Ok(())
    }

    fn letter(&self, id: &str) -> NorthResult<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(NorthError::Usage(format!("not a letter id: {id}")));
        }
        Ok(self.dir.join(format!("{id}.{LETTER}")))
    }
}

pub trait Courier: Sync {
    fn take(&self, host: &str, relay_host: &str) -> NorthResult<Vec<Letter>>;
    fn drop(&self, host: &str, ids: &[String]) -> NorthResult<()>;
    fn snapshot(&self, host: &str) -> NorthResult<Snapshot>;
    /// Runs again the sessions the host's reboot stranded, returning one line per session.
    fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>>;
    fn answer(&self, host: &str, caller: Option<&str>, id: i64, number: usize, note: Option<&str>) -> NorthResult<String>;
}

pub struct Ssh;

impl Courier for Ssh {
    fn take(&self, host: &str, relay_host: &str) -> NorthResult<Vec<Letter>> {
        Ok(serde_json::from_slice(&over_ssh(host, None, &["fleet", "outbox", "take", "--for", relay_host])?)?)
    }

    fn drop(&self, host: &str, ids: &[String]) -> NorthResult<()> {
        let words: Vec<&str> = ["fleet", "outbox", "drop"].into_iter().chain(ids.iter().map(String::as_str)).collect();
        over_ssh(host, None, &words).map(|_| ())
    }

    fn snapshot(&self, host: &str) -> NorthResult<Snapshot> {
        Ok(serde_json::from_slice(&over_ssh(host, None, &["fleet", "outbox", "snapshot"])?)?)
    }

    fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>> {
        let output = over_ssh_within(host, None, &["fleet", "rebooted", "--revive"], REVIVE_WAIT)?;
        Ok(String::from_utf8_lossy(&output).lines().map(str::to_owned).collect())
    }

    fn answer(&self, host: &str, caller: Option<&str>, id: i64, number: usize, note: Option<&str>) -> NorthResult<String> {
        let (id, number) = (id.to_string(), number.to_string());
        let words: Vec<&str> = ["gate", "answer", id.as_str(), number.as_str()].into_iter().chain(note.into_iter().flat_map(|note| ["--note", note])).collect();
        Ok(String::from_utf8_lossy(&over_ssh(host, caller, &words)?).trim_end().to_owned())
    }
}

fn over_ssh(host: &str, caller: Option<&str>, words: &[&str]) -> NorthResult<Vec<u8>> {
    over_ssh_within(host, caller, words, SSH_WAIT)
}

fn over_ssh_within(host: &str, caller: Option<&str>, words: &[&str], wait: Duration) -> NorthResult<Vec<u8>> {
    let signed = caller.map(|caller| format!("{SESSION_ENV}={} ", quote(caller))).unwrap_or_default();
    let command = format!("{signed}north {}", words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "));
    let ssh = binary("ssh");
    let argv = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", host, "--", &command];
    match run_within(&ssh.to_string_lossy(), &argv, wait) {
        Some(output) if output.status.success() => Ok(output.stdout),
        Some(output) => Err(NorthError::Usage(format!("ssh {host} {command}: {}: {}", output.status, String::from_utf8_lossy(&output.stderr).trim()))),
        None => Err(NorthError::Usage(format!("ssh {host} {command}: no answer within {}s", wait.as_secs()))),
    }
}

pub struct Undelivered {
    pub host: String,
    pub letter: Letter,
    pub reason: String,
}

#[derive(Default)]
pub struct Relayed {
    pub delivered: Vec<(String, Letter)>,
    pub undelivered: Vec<Undelivered>,
    pub unreachable: Vec<(String, String)>,
    pub undropped: Vec<String>,
    pub snapshots: Vec<(String, Snapshot)>,
    /// The lines each reachable host gave for the sessions its reboot stranded and it ran again.
    pub revived: Vec<(String, String)>,
}

pub fn relay(courier: &dyn Courier, relay_host: &str, hosts: &[String], undropped: &BTreeSet<String>, deliver: &mut dyn FnMut(&Letter) -> NorthResult<()>) -> Relayed {
    let mut relayed = Relayed::default();
    let mut drops = Vec::new();
    let mut reachable = Vec::new();
    for (host, taken) in hosts.iter().zip(each(hosts, |host| courier.take(host, relay_host))) {
        let letters = match taken {
            Ok(letters) => letters,
            Err(error) => {
                relayed.unreachable.push((host.clone(), error.user_message()));
                continue;
            }
        };
        reachable.push(host.clone());
        let mut dropped = Vec::new();
        for letter in letters {
            if undropped.contains(&format!("{host} {}", letter.id)) {
                dropped.push(letter.id.clone());
                continue;
            }
            match deliver(&letter) {
                Ok(()) => {
                    dropped.push(letter.id.clone());
                    relayed.delivered.push((host.clone(), letter));
                }
                Err(error) => relayed.undelivered.push(Undelivered { host: host.clone(), letter, reason: error.user_message() }),
            }
        }
        if !dropped.is_empty() {
            drops.push((host.clone(), dropped));
        }
    }
    let (dropped, snapshots, revived) = std::thread::scope(|scope| {
        let snapshots = scope.spawn(|| each(&reachable, |host| courier.snapshot(host)));
        let revived = scope.spawn(|| each(&reachable, |host| courier.revive_rebooted(host)));
        (each(&drops, |(host, ids)| courier.drop(host, ids)), snapshots.join().unwrap_or_default(), revived.join().unwrap_or_default())
    });
    relayed.note_revived(&reachable, revived);
    for ((host, dropped), outcome) in drops.iter().zip(dropped) {
        if let Err(error) = outcome {
            relayed.unreachable.push((host.clone(), format!("delivered {} but could not drop them: {}", dropped.join(" "), error.user_message())));
            relayed.undropped.extend(dropped.iter().map(|id| format!("{host} {id}")));
        }
    }
    for (host, outcome) in reachable.into_iter().zip(snapshots) {
        match outcome {
            Ok(snapshot) => relayed.snapshots.push((host, snapshot)),
            Err(error) => relayed.unreachable.push((host, format!("its sessions and gates were not read: {}", error.user_message()))),
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
    fn note_revived(&mut self, hosts: &[String], outcomes: Vec<NorthResult<Vec<String>>>) {
        for (host, outcome) in hosts.iter().zip(outcomes) {
            match outcome {
                Ok(lines) => self.revived.extend(lines.into_iter().map(|line| (host.clone(), line))),
                Err(error) => self.unreachable.push((host.clone(), format!("its rebooted sessions were not revived: {}", error.user_message()))),
            }
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let delivered = self.delivered.iter().map(|(host, letter)| format!("relayed {} from {host} to {}", letter.id, letter.to));
        let undelivered = self.undelivered.iter().map(|failed| format!("undelivered {} from {} to {}: {}", failed.letter.id, failed.host, failed.letter.to, failed.reason));
        let revived = self.revived.iter().map(|(host, line)| format!("{host}: {line}"));
        let unreachable = self.unreachable.iter().map(|(host, reason)| format!("unreachable {host}: {reason}"));
        delivered.chain(undelivered).chain(revived).chain(unreachable).collect()
    }

    pub fn newly_undelivered(&self, seen: &Path) -> NorthResult<Vec<String>> {
        self.settle(seen, self.undelivered.iter().map(|failed| format!("{} {}", failed.host, failed.letter.id)).collect())
    }

    pub fn newly_undropped(&self, seen: &Path) -> NorthResult<Vec<String>> {
        self.settle(seen, self.undropped.iter().cloned().collect())
    }

    fn settle(&self, seen: &Path, now: BTreeSet<String>) -> NorthResult<Vec<String>> {
        let before = entries(seen);
        let unseen = |host: &str| self.unreachable.iter().any(|(unreachable, _)| unreachable == host);
        let held = before.iter().filter(|entry| entry.split_once(' ').is_some_and(|(host, _)| unseen(host)));
        let kept: BTreeSet<&String> = now.iter().chain(held).collect();
        if let Some(parent) = seen.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(seen, kept.iter().map(|entry| format!("{entry}\n")).collect::<String>())?;
        Ok(now.difference(&before).cloned().collect())
    }
}

pub fn entries(seen: &Path) -> BTreeSet<String> {
    fs::read_to_string(seen).unwrap_or_default().lines().map(str::to_owned).collect()
}

pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned()
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::collections::BTreeMap;

    use super::*;

    struct Hosts(BTreeMap<String, Outbox>, AtomicBool);

    impl Courier for Hosts {
        fn take(&self, host: &str, relay_host: &str) -> NorthResult<Vec<Letter>> {
            self.0.get(host).ok_or_else(|| NorthError::Usage(format!("ssh {host}: unreachable")))?.take(relay_host)
        }

        fn drop(&self, host: &str, ids: &[String]) -> NorthResult<()> {
            if self.1.load(Ordering::SeqCst) {
                return Err(NorthError::Usage(format!("ssh {host}: no answer within 20s")));
            }
            self.0[host].drop(ids)
        }

        fn snapshot(&self, host: &str) -> NorthResult<Snapshot> {
            Ok(Snapshot { host: host.into(), ..Snapshot::default() })
        }

        fn revive_rebooted(&self, _: &str) -> NorthResult<Vec<String>> {
            Ok(Vec::new())
        }

        fn answer(&self, host: &str, _: Option<&str>, _: i64, _: usize, _: Option<&str>) -> NorthResult<String> {
            Err(NorthError::Usage(format!("ssh {host}: unreachable")))
        }
    }

    fn message(sender: Option<&str>, to: &str, text: &str, delivery: Delivery) -> FleetRow {
        FleetRow { id: format!("{}-{to}", now_ms()), kind: RowKind::Message, to: to.into(), from: hostname(), urgent: delivery == Delivery::Now, sent_at: now_ms(), text: text.into(), sender: sender.map(str::to_owned), gate: None, job: None }
    }

    fn far(root: &Path) -> Hosts {
        Hosts(BTreeMap::from([("far".to_owned(), Outbox::new(root.join("far")))]), AtomicBool::new(false))
    }

    #[test]
    fn a_letter_posted_where_its_session_is_not_is_delivered_on_the_fleet_host_and_dropped_where_it_was_posted() {
        let root = tempfile::tempdir().unwrap();
        let hosts = far(root.path());
        let outbox = &hosts.0["far"];
        assert_eq!(outbox.relay(), None);
        assert!(outbox.take("fleet").unwrap().is_empty());
        assert_eq!(outbox.relay().map(|relay| relay.host), Some("fleet".to_owned()));
        let posted = outbox.post(&message(Some("wisp-lead"), "proxy", "rolled back", Delivery::Queued)).unwrap();
        let mut received = Vec::new();
        let relayed = relay(&hosts, "fleet", &["far".to_owned()], &BTreeSet::new(), &mut |letter| {
            received.push((letter.signer(), letter.to.clone(), letter.text.clone(), letter.delivery()));
            Ok(())
        });
        assert_eq!(received, [(format!("wisp-lead@{}", posted.from), "proxy".to_owned(), "rolled back".to_owned(), Delivery::Queued)]);
        assert_eq!(relayed.lines(), [format!("relayed {} from far to proxy", posted.id)]);
        assert!(outbox.take("fleet").unwrap().is_empty(), "a delivered letter is dropped from the host that posted it");
        assert!(outbox.drop(&["../relay".to_owned()]).is_err() && outbox.drop(&[".relay".to_owned()]).is_err(), "a letter id cannot name a path outside the outbox");
    }

    #[test]
    fn an_undelivered_letter_stays_posted_and_is_new_only_once() {
        let root = tempfile::tempdir().unwrap();
        let seen = root.path().join("seen");
        let hosts = far(root.path());
        let outbox = &hosts.0["far"];
        let kept = outbox.post(&message(None, "proxy", "rolled back", Delivery::Now)).unwrap();
        let mut refused = |_: &Letter| Err(NorthError::Usage("no session named proxy".into()));
        let first = relay(&hosts, "fleet", &["far".to_owned()], &BTreeSet::new(), &mut refused);
        assert_eq!(first.lines(), [format!("undelivered {} from far to proxy: no session named proxy", kept.id)]);
        assert_eq!(first.newly_undelivered(&seen).unwrap(), [format!("far {}", kept.id)]);
        let again = relay(&hosts, "fleet", &["far".to_owned()], &BTreeSet::new(), &mut refused);
        assert!(again.newly_undelivered(&seen).unwrap().is_empty(), "a letter already reported is not new again");
        let gone = Hosts(BTreeMap::new(), AtomicBool::new(false));
        assert!(relay(&gone, "fleet", &["far".to_owned()], &BTreeSet::new(), &mut refused).newly_undelivered(&seen).unwrap().is_empty());
        let back = relay(&hosts, "fleet", &["far".to_owned()], &BTreeSet::new(), &mut refused);
        assert!(back.newly_undelivered(&seen).unwrap().is_empty(), "an unreachable host keeps its letters reported");
        assert_eq!(outbox.take("fleet").unwrap(), [kept]);
    }

    #[test]
    fn a_letter_delivered_but_not_dropped_is_dropped_later_and_never_delivered_twice() {
        let root = tempfile::tempdir().unwrap();
        let seen = root.path().join("undropped");
        let hosts = far(root.path());
        let outbox = &hosts.0["far"];
        let posted = outbox.post(&message(None, "proxy", "rolled back", Delivery::Queued)).unwrap();
        let mut deliveries = 0;
        hosts.1.store(true, Ordering::SeqCst);
        let first = relay(&hosts, "fleet", &["far".to_owned()], &entries(&seen), &mut |_| {
            deliveries += 1;
            Ok(())
        });
        assert_eq!(first.newly_undropped(&seen).unwrap(), [format!("far {}", posted.id)]);
        let still = relay(&hosts, "fleet", &["far".to_owned()], &entries(&seen), &mut |_| {
            deliveries += 1;
            Ok(())
        });
        assert!(still.newly_undropped(&seen).unwrap().is_empty(), "a letter already reported undropped is not new again");
        hosts.1.store(false, Ordering::SeqCst);
        let second = relay(&hosts, "fleet", &["far".to_owned()], &entries(&seen), &mut |_| {
            deliveries += 1;
            Ok(())
        });
        assert!(second.newly_undropped(&seen).unwrap().is_empty());
        assert_eq!(deliveries, 1, "a letter delivered once is not delivered again while its drop is retried");
        assert!(outbox.take("fleet").unwrap().is_empty() && entries(&seen).is_empty());
    }

    struct Slow(Duration);

    impl Courier for Slow {
        fn take(&self, host: &str, _: &str) -> NorthResult<Vec<Letter>> {
            std::thread::sleep(self.0);
            Ok(vec![Letter { id: format!("1-{host}"), to: "proxy".into(), text: host.into(), now: false, from: host.into(), sender: None, gate: None, at_ms: 1 }])
        }

        fn drop(&self, _: &str, _: &[String]) -> NorthResult<()> {
            std::thread::sleep(self.0);
            Ok(())
        }

        fn snapshot(&self, host: &str) -> NorthResult<Snapshot> {
            std::thread::sleep(self.0);
            Ok(Snapshot { host: host.into(), ..Snapshot::default() })
        }

        fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>> {
            Ok(vec![format!("resumed {host}-worker")])
        }

        fn answer(&self, host: &str, _: Option<&str>, _: i64, _: usize, _: Option<&str>) -> NorthResult<String> {
            Err(NorthError::Usage(format!("ssh {host}: unreachable")))
        }
    }

    struct Rebooting(Hosts);

    impl Courier for Rebooting {
        fn take(&self, host: &str, relay_host: &str) -> NorthResult<Vec<Letter>> {
            self.0.take(host, relay_host)
        }

        fn drop(&self, host: &str, ids: &[String]) -> NorthResult<()> {
            self.0.drop(host, ids)
        }

        fn snapshot(&self, host: &str) -> NorthResult<Snapshot> {
            self.0.snapshot(host)
        }

        fn revive_rebooted(&self, host: &str) -> NorthResult<Vec<String>> {
            Ok(vec![format!("resumed {host}-worker")])
        }

        fn answer(&self, host: &str, caller: Option<&str>, id: i64, number: usize, note: Option<&str>) -> NorthResult<String> {
            self.0.answer(host, caller, id, number, note)
        }
    }

    #[test]
    fn a_host_that_answers_is_asked_to_revive_its_rebooted_sessions_and_a_host_that_does_not_is_not() {
        let root = tempfile::tempdir().unwrap();
        let hosts = Rebooting(far(root.path()));
        let list = ["far".to_owned(), "down".to_owned()];

        let relayed = relay(&hosts, "fleet", &list, &BTreeSet::new(), &mut |_| Ok(()));

        assert_eq!(relayed.revived, [("far".to_owned(), "resumed far-worker".to_owned())]);
        assert!(relayed.lines().contains(&"far: resumed far-worker".to_owned()), "{:?}", relayed.lines());
        assert!(relayed.lines().iter().any(|line| line.starts_with("unreachable down:")));
    }

    #[test]
    fn hosts_that_each_answer_slowly_are_relayed_within_one_wait_for_taking_and_one_for_dropping() {
        let hosts: Vec<String> = ["nexus", "gex45", "far", "near"].map(str::to_owned).into();
        let begun = std::time::Instant::now();
        let relayed = relay(&Slow(Duration::from_millis(400)), "fleet", &hosts, &BTreeSet::new(), &mut |_| Ok(()));
        assert!(begun.elapsed() < Duration::from_millis(1500), "{:?}", begun.elapsed());
        let from: Vec<&str> = relayed.delivered.iter().map(|(host, _)| host.as_str()).collect();
        assert_eq!(from, ["nexus", "gex45", "far", "near"]);
        assert!(relayed.unreachable.is_empty() && relayed.undropped.is_empty());
    }
}
