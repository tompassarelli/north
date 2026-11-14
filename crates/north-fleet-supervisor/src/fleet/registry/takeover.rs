use std::fs;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};

use super::Fleet;
use super::stopping::Halt;
use super::super::model::{Role, SessionRecord};
use super::super::records::record_file;
use super::super::runtime::PROXY;
use super::super::store::{Addressee, Claim, FleetRow, RowKind};

/// The session a takeover ended, and where its directory now lies.
pub(super) struct Predecessor {
    record: SessionRecord,
    archived: PathBuf,
}

impl Predecessor {
    /// The successor takes every message to the name from where the predecessor's line began, so a report sent before the takeover that the old session never took still arrives.
    pub(super) fn record(&self) -> &SessionRecord {
        &self.record
    }

    pub(super) fn addressed_since(&self) -> u64 {
        self.record.addressed_since()
    }

    /// Whether the seat taken over keeps its claims: the launch names no task, or only tasks the seat held, as a relaunch of its own record does.
    pub(super) fn carries_for(&self, named: &[String]) -> bool {
        named.iter().all(|task| self.record.tasks().any(|held| held == task))
    }

    /// Puts every task and claim the taken-over seat held into its successor's `record`, so a lead keeps the task that binds it to the lead grant; returns the claim keys the successor takes in the ledger.
    pub(super) fn carry_into(&self, record: &mut SessionRecord) -> Vec<String> {
        record.task_ref = self.record.task_ref.clone();
        record.extra_tasks = self.record.extra_tasks.clone();
        record.claim_box = self.record.claim_box.clone();
        record.other_claims = self.record.other_claims.clone();
        record.claim_keys()
    }
}

impl Fleet {
    /// Ends and archives the session holding `name` when this launch takes the name over; None when no session holds it.
    pub(super) fn retire_for(&self, name: &str, claim: Claim) -> NorthResult<Option<Predecessor>> {
        if claim != Claim::TakeOver || !self.session_dir(name)?.exists() || self.clear_ghost(name)? {
            return Ok(None);
        }
        let record = self.record(name)?;
        let archived = self.archive(name, Halt::TakenOver, Halt::TakenOver.label())?;
        eprintln!("north fleet: {name} was held by conversation {}; it is stopped and archived at {}, and the name and its messages now go to the new session", record.conversation.as_deref().unwrap_or("none"), archived.display());
        Ok(Some(Predecessor { record, archived }))
    }

    /// Removes a directory that holds a name with no session record in it, as a launch killed after claiming the name leaves, and gives up the claims the dead launch took under the name; true when it removed one. The caller holds the name's start lock, so no live launch is building a record there.
    pub(super) fn clear_ghost(&self, name: &str) -> NorthResult<bool> {
        let dir = self.session_dir(name)?;
        if !dir.exists() || record_file(&dir).exists() {
            return Ok(false);
        }
        if let Err(error) = self.release_all_claims_of(name) {
            eprintln!("north fleet: a launch of {name} died before its session began, and its claims were not all given up: {}", error.user_message());
        }
        self.discard_failed_launch(&dir)?;
        eprintln!("north fleet: {name} was left by a launch that died before its session began; its directory is removed and the name is free");
        Ok(true)
    }

    /// Retires the session holding `name` for a take-over and runs `succeed` to bring up its successor, all under the name's start lock, so a revive racing them either runs before the old session is stopped or finds the successor running.
    pub(super) fn succeed<T>(&self, name: &str, claim: Claim, succeed: impl FnOnce(Option<Predecessor>) -> NorthResult<T>) -> NorthResult<T> {
        let _start = self.start_lock(name)?;
        let predecessor = self.retire_for(name, claim)?;
        let archived = predecessor.as_ref().map(|predecessor| predecessor.archived.clone());
        let launched = succeed(predecessor);
        if let (Err(error), Some(archived)) = (&launched, archived) {
            self.restore_predecessor(name, &archived, error);
        }
        launched
    }

    /// Puts the session a failed take-over archived back under its name and starts it again on its own account, so a successor that never came up leaves the working session running; a name already holding a directory is left as the failed launch left it.
    fn restore_predecessor(&self, name: &str, archived: &std::path::Path, failure: &NorthError) {
        let restored = self.session_dir(name).and_then(|dir| if dir.exists() { Ok(false) } else { fs::rename(archived, &dir).map(|()| true).map_err(Into::into) });
        match restored.and_then(|moved| if moved { self.reopened(name).and_then(|_| self.revive_held(name, None, None, None)).map(drop) } else { Ok(()) }) {
            Ok(()) => eprintln!("north fleet: the take-over of {name} failed ({}); the session it was to replace is running again", failure.user_message()),
            Err(error) => eprintln!("north fleet: the take-over of {name} failed ({}) and the session it was to replace was not restarted: {}; `north fleet revive {name}` restarts it", failure.user_message(), error.user_message()),
        }
    }

    pub fn fleet_proxy(&self) -> NorthResult<Option<SessionRecord>> {
        Ok(self.sessions()?.into_iter().find(|record| record.name == PROXY && record.role == Some(Role::Proxy)))
    }

    pub fn earlier_proxies(&self) -> NorthResult<Vec<SessionRecord>> {
        let earlier = |record: &SessionRecord| match record.role {
            None => record.name == PROXY || record.name.starts_with("proxy-"),
            Some(role) => role == Role::Proxy && record.name != PROXY,
        };
        Ok(self.sessions()?.into_iter().filter(earlier).collect())
    }

    pub fn retire_proxy(&self, name: &str) -> NorthResult<PathBuf> {
        self.archive(name, Halt::Retired, "a proxy take-over retired it")
    }

    pub fn addressee(&self, record: &SessionRecord, floor: u64) -> NorthResult<Addressee> {
        let taken = self.delivered(&record.name)?.into_iter().filter(|id| sent_at(id).is_none_or(|ms| ms >= floor)).collect();
        Ok(Addressee { name: record.name.clone(), stopped: record.ended(), control_since_ms: Some(record.created_ms.max(floor)), since_ms: record.addressed_since().max(floor), taken })
    }
}

pub fn backlog_line(retired: &[String], rows: &[FleetRow], now_ms: u64) -> String {
    let messages: Vec<&FleetRow> = rows.iter().filter(|row| row.kind == RowKind::Message).collect();
    let names = retired.join(" and ");
    let Some(oldest) = messages.iter().map(|row| row.sent_at).min() else {
        return format!("[north: north proxy archived {names}; no messages to them were waiting]");
    };
    let mut senders: Vec<String> = messages.iter().map(|row| row.sender.prefix().unwrap_or_else(|| "Tom".to_owned())).collect();
    senders.sort();
    senders.dedup();
    let count = messages.len();
    format!(
        "[north: north proxy archived {names}; {count} message{} to them were not replayed, from {}, the oldest {} old]",
        if count == 1 { "" } else { "s" },
        senders.join(", "),
        age(now_ms.saturating_sub(oldest))
    )
}

fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

fn sent_at(id: &str) -> Option<u64> {
    id.split('-').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::super::HandedIn;
    use super::super::fixtures::{fleet, live};
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::Value;

    use crate::error::NorthError;
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::model::{Account, LaunchRequest, ProviderName, Quota, Role, Transcript};
    use crate::fleet::roster::Roster;
    use crate::fleet::runtime::{HostSpec, Instructions, Opening, Runtime};
    use crate::fleet::runtimes::Runtimes;
    use crate::fleet::turns::TurnSignal;
    use crate::fleet::records::{read_record, write_record};
    use crate::fleet::sender::Sender;
    use crate::fleet::store::RowKind;

    struct NeverUp;

    impl Runtime for NeverUp {
        fn provider(&self) -> ProviderName {
            Stream.provider()
        }

        fn models(&self) -> &'static [&'static str] {
            Stream.models()
        }

        fn roster(&self, _state_root: &Path, home: &Path, _policy: &Path) -> Roster {
            let account = |label: &str| Account { provider: Stream.provider(), label: label.into(), home: home.join(label), token_file: None, api: None };
            Roster { accounts: vec![account("a"), account("b")], ..Roster::default() }
        }

        fn assign_conversation(&self) -> NorthResult<Option<String>> {
            Stream.assign_conversation()
        }

        fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec> {
            if account.label == "a" {
                return Stream.host(account, request, opening, instructions);
            }
            Err(NorthError::State("the client did not come up within its deadline".into()))
        }

        fn handshake(&self) -> Vec<String> {
            Stream.handshake()
        }

        fn open(&self, name: &str, request: &LaunchRequest, opening: Opening, instructions: Instructions) -> NorthResult<Vec<String>> {
            Stream.open(name, request, opening, instructions)
        }

        fn message(&self, transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>> {
            Stream.message(transcript, id, text)
        }

        fn interrupt(&self, transcript: &Transcript) -> NorthResult<Vec<String>> {
            Stream.interrupt(transcript)
        }

        fn close(&self, transcript: &Transcript) -> Vec<String> {
            Stream.close(transcript)
        }

        fn observe(&self, frame: &Value, transcript: &mut Transcript) {
            Stream.observe(frame, transcript);
        }

        fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
            Stream.signals(frame)
        }

        fn quota(&self, frame: &Value) -> Option<Quota> {
            Stream.quota(frame)
        }
    }

    #[test]
    fn a_hand_over_whose_client_never_comes_up_leaves_the_session_revivable_on_its_own_account_and_a_take_over_record_with_no_live_successor_leaves_the_working_session_running() {
        let home = tempfile::tempdir().unwrap();
        let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(NeverUp)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()));
        let dir = fleet.root.join("worker");
        std::fs::create_dir_all(&dir).unwrap();
        let held = serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "kept", "task_ref": "north#1115", "host_dir": dir, "stream_offset": 0});
        std::fs::write(dir.join("session.json"), held.to_string()).unwrap();
        let record = fleet.session("worker").unwrap();
        let request = LaunchRequest { account: "b".into(), prompt: "carry on".into(), placement: Some("handed over from account a".into()), resume: record.conversation.clone(), ..LaunchRequest::relaunching(&record) };

        let refused = fleet.take_over(&request);

        assert!(refused.is_err(), "{refused:?}");
        let kept = fleet.session("worker").unwrap_or_else(|error| panic!("no session is left under the name: {}", error.user_message()));
        assert_eq!((kept.account.as_str(), kept.conversation.as_deref(), kept.task_ref.as_deref()), ("a", Some("kept"), Some("north#1115")));

        live(&fleet, "steady");
        let working = fleet.session("steady").unwrap();
        let successor = LaunchRequest { account: "b".into(), prompt: "carry on".into(), resume: None, ..LaunchRequest::relaunching(&working) };

        let refused = fleet.take_over(&successor);

        assert!(refused.is_err(), "{refused:?}");
        let kept = fleet.session("steady").unwrap_or_else(|error| panic!("the working session was archived: {}", error.user_message()));
        assert_eq!((kept.account.as_str(), kept.conversation.as_deref(), kept.live()), ("a", Some("steady"), true), "{kept:?}");
    }

    #[test]
    fn a_taken_over_lead_seat_keeps_its_task_and_extra_tasks() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let dir = fleet.root.join("worker");
        std::fs::create_dir_all(&dir).unwrap();
        let parked = serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "worker", "host_dir": dir, "stream_offset": 0});
        std::fs::write(dir.join("session.json"), parked.to_string()).unwrap();
        let row = fleet.new_row(RowKind::Message, "worker", "carry on with the build", &Sender::Tom).unwrap();

        let (predecessor, successor, revive) = fleet
            .succeed("worker", Claim::TakeOver, |predecessor| {
                let delivery = fleet.clone();
                let revive = std::thread::spawn(move || delivery.hand_in(&row));
                let session = live(&fleet, "worker");
                let mut successor = read_record(&dir).unwrap();
                successor.created_ms = 2;
                write_record(&dir, &successor).unwrap();
                Ok((predecessor.map(|predecessor| predecessor.record().created_ms), session, revive))
            })
            .unwrap();

        assert_eq!(revive.join().unwrap().unwrap(), HandedIn::Delivered);
        assert_eq!(predecessor, Some(1));
        let running: Vec<SessionRecord> = fleet.sessions().unwrap().into_iter().filter(|record| record.name == "worker").collect();
        assert!(matches!(running.as_slice(), [record] if record.live() && record.created_ms == 2 && record.conversation.as_deref() == Some("worker")), "{running:?}");
        assert!(successor.frames.lock().unwrap().iter().any(|frame| frame.contains("carry on with the build")), "the message never reached the successor");
        let _next = fleet.start_lock("worker").unwrap();
        let free = { let fleet = fleet.clone(); std::thread::spawn(move || fleet.try_start_lock("worker").unwrap().is_some()).join().unwrap() };
        assert!(!free, "the next take-over on this thread left the name unlocked");

        let main = fleet.home.join("code/north/main");
        std::fs::create_dir_all(&main).unwrap();
        let seat = fleet.root.join("seat");
        std::fs::create_dir_all(&seat).unwrap();
        let held = serde_json::json!({"name": "seat", "provider": "claude", "account": "a", "cwd": main, "model": null, "created_ms": 1, "host_pid": null, "conversation": "seat", "host_dir": seat, "stream_offset": 0, "role": "lead", "task_ref": "north#1054", "extra_tasks": ["north#1055", "north#1056"]});
        std::fs::write(seat.join("session.json"), held.to_string()).unwrap();
        let request = LaunchRequest { prompt: "carry on".into(), task_ref: None, claim_box: None, ..LaunchRequest::relaunching(&fleet.session("seat").unwrap()) };
        let (taken, claimed) = fleet
            .succeed("seat", Claim::TakeOver, |predecessor| {
                let predecessor = predecessor.unwrap();
                assert!(predecessor.carries_for(&request.tasks()));
                let mut successor = request.record(None, Some("seat".into()), false, 2);
                successor.host_dir = seat.clone();
                let claimed = predecessor.carry_into(&mut successor);
                std::fs::create_dir_all(&seat).unwrap();
                write_record(&seat, &successor).unwrap();
                Ok((successor, claimed))
            })
            .unwrap();
        assert_eq!((taken.role(), taken.tasks().collect::<Vec<_>>()), (Role::Lead, ["north#1054", "north#1055", "north#1056"].to_vec()));
        assert_eq!(claimed, ["north#1054", "north#1055", "north#1056"]);
        assert_eq!(fleet.granted_launch("seat", &["--name", "n1054-child", "--task", "north#1054", "brief"].map(str::to_owned)).unwrap().name, "n1054-child");

        let bare = fleet.root.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("session.json"), serde_json::json!({"name": "bare", "provider": "claude", "account": "a", "cwd": main, "model": null, "created_ms": 1, "host_pid": null, "conversation": "bare", "host_dir": bare, "stream_offset": 0, "role": "lead"}).to_string()).unwrap();
        let refused = fleet.granted_launch("bare", &["--name", "n1054-child", "--task", "north#1054", "brief"].map(str::to_owned)).unwrap_err();
        assert!(refused.contains("bare's record holds no task"), "{refused}");
    }
}
