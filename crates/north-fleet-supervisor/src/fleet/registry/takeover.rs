use std::fs;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};

use super::Fleet;
use super::stopping::Halt;
use super::super::model::{LaunchRequest, Role, SessionRecord};
use super::super::records::record_file;
use super::super::runtime::PROXY;
use super::super::store::{Addressee, Claim, FleetRow, RowKind};

/// The session a takeover ended.
pub(super) struct Predecessor {
    record: SessionRecord,
}

impl Predecessor {
    /// The successor takes every message to the name from where the predecessor's line began, so a report sent before the takeover that the old session never took still arrives.
    pub(super) fn record(&self) -> &SessionRecord {
        &self.record
    }

    pub(super) fn addressed_since(&self) -> u64 {
        self.record.addressed_since()
    }
}

/// The name a launch takes, with the session that holds it while the launch claims what it needs, so a refused claim leaves that session running as it was; it is stopped and archived only by [`Seat::retire`].
pub(super) struct Seat<'a> {
    fleet: &'a Fleet,
    name: &'a str,
    held: Option<SessionRecord>,
    archived: Option<PathBuf>,
}

impl Seat<'_> {
    pub(super) fn held(&self) -> Option<&SessionRecord> {
        self.held.as_ref()
    }

    /// Ends and archives the session holding the name, giving up its claims but those in `carried`, which its successor holds under the same name; None when no session holds it.
    pub(super) fn retire(&mut self, carried: &[String]) -> NorthResult<Option<Predecessor>> {
        let Some(record) = self.held.clone() else { return Ok(None) };
        let archived = self.fleet.archive_carrying(self.name, Halt::TakenOver, Halt::TakenOver.label(), carried)?;
        eprintln!("north fleet: {} was held by conversation {}; it is stopped and archived at {}, and the name and its messages now go to the new session", self.name, record.conversation.as_deref().unwrap_or("none"), archived.display());
        self.archived = Some(archived);
        Ok(Some(Predecessor { record }))
    }

    /// Retires the seat for its successor `record`, keeping the claims the successor carries, and starts the successor's messages where the predecessor's began.
    pub(super) fn hand_to(&mut self, record: &mut SessionRecord) -> NorthResult<Option<Predecessor>> {
        let predecessor = self.retire(&record.claim_keys())?;
        record.addressed_since_ms = predecessor.as_ref().map(Predecessor::addressed_since);
        Ok(predecessor)
    }
}

/// Whether the seat taken over keeps its claims: the launch names no task, or only tasks the seat held, as a relaunch of its own record does.
pub(super) fn carries_for(held: &SessionRecord, named: &[String]) -> bool {
    named.iter().all(|task| held.tasks().any(|kept| kept == task))
}

/// Puts every task the taken-over seat `held` into its successor's `record`, and its box and other claims too unless the launch names a box of its own, so a lead keeps the task that binds it to the lead grant; returns the claim keys the successor's record names.
pub(super) fn carry_into(held: &SessionRecord, record: &mut SessionRecord) -> Vec<String> {
    record.task_ref = held.task_ref.clone();
    record.extra_tasks = held.extra_tasks.clone();
    let named = record.claim_box.take();
    (record.claim_box, record.other_claims) = named.map_or_else(|| (held.claim_box.clone(), held.other_claims.clone()), |named| (Some(named), std::mem::take(&mut record.other_claims)));
    record.claim_keys()
}

impl Fleet {
    /// Claims what a launch taking over `seat` needs before the seat is stopped, so a refused claim leaves the seat running; returns the keys the launch still has to claim and whether they were claimed here.
    pub(super) fn claim_for(&self, seat: &Seat<'_>, request: &LaunchRequest, record: &mut SessionRecord, tasks: &[String], verification: bool) -> NorthResult<(Vec<String>, bool)> {
        let tasks = seat.held().filter(|held| carries_for(held, &request.tasks())).map_or_else(|| tasks.to_vec(), |held| self.left_to_claim(&request.name, &carry_into(held, record), request.role() == Role::Proxy));
        let claimed_first = seat.held().is_some();
        if claimed_first {
            self.claim_before_start(request, &tasks, verification)?;
        }
        Ok((tasks, claimed_first))
    }

    /// The session holding `name` when this launch takes the name over; None when no session holds it.
    fn held_for(&self, name: &str, claim: Claim) -> NorthResult<Option<SessionRecord>> {
        if claim != Claim::TakeOver || !self.session_dir(name)?.exists() || self.clear_ghost(name)? {
            return Ok(None);
        }
        self.record(name).map(Some)
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

    /// Runs `succeed` to bring up the successor of the session holding `name`, which it retires through the [`Seat`] once its claims are in, all under the name's start lock, so a revive racing them either runs before the old session is stopped or finds the successor running.
    pub(super) fn succeed<T>(&self, name: &str, claim: Claim, succeed: impl FnOnce(&mut Seat<'_>) -> NorthResult<T>) -> NorthResult<T> {
        let _start = self.start_lock(name)?;
        let mut seat = Seat { fleet: self, name, held: self.held_for(name, claim)?, archived: None };
        let launched = succeed(&mut seat);
        if let (Err(error), Some(archived)) = (&launched, &seat.archived) {
            self.restore_predecessor(name, archived, error);
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
    use super::super::fixtures::{fleet, lead, live, live_under, standing_host};
    use super::*;
    use std::collections::BTreeSet;
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
    use crate::fleet::claims::{Claims, Rows, Taken, split_key};
    use crate::fleet::session_lifecycle::ARCHIVE;
    use crate::fleet::registry::NoDesk;

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

    fn seat_taken_over(fleet: &Fleet, name: &str, named: Option<&str>) -> (SessionRecord, Vec<String>) {
        let main = fleet.home.join("code/north/main");
        let seat = fleet.root.join(name);
        std::fs::create_dir_all(&seat).unwrap();
        let held = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": main, "model": null, "created_ms": 1, "host_pid": null, "conversation": name, "host_dir": seat, "stream_offset": 0, "role": "lead", "task_ref": "north#1054", "extra_tasks": ["north#1055", "north#1056"], "claim_box": "old box"});
        std::fs::write(seat.join("session.json"), held.to_string()).unwrap();
        let request = LaunchRequest { prompt: "carry on".into(), task_ref: None, claim_box: named.map(str::to_owned), ..LaunchRequest::relaunching(&fleet.session(name).unwrap()) };
        fleet
            .succeed(name, Claim::TakeOver, |taken| {
                let held = taken.held().unwrap().clone();
                assert!(carries_for(&held, &request.tasks()));
                let mut successor = request.record(None, Some(name.into()), false, 2);
                successor.host_dir = seat.clone();
                let claimed = carry_into(&held, &mut successor);
                taken.retire(&claimed)?;
                std::fs::create_dir_all(&seat).unwrap();
                write_record(&seat, &successor).unwrap();
                Ok((successor, claimed))
            })
            .unwrap()
    }

    #[test]
    fn a_hand_over_whose_client_never_comes_up_leaves_the_session_revivable_on_its_own_account_and_a_take_over_with_no_live_successor_or_window_leaves_the_working_session_running() {
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

        let home = tempfile::tempdir().unwrap();
        let (fleet, claims) = lead_whose_child_holds_an_issue(home.path());
        let mut record = fleet.session("slice").unwrap();
        record.extra_tasks.push("north#1400".into());
        write_record(&fleet.root.join("slice"), &record).unwrap();
        claims.hold("north#1400", "outsider", &BTreeSet::new()).unwrap();
        let before = std::fs::read(fleet.root.join("slice/session.json")).unwrap();
        let request = LaunchRequest { prompt: "carry on".into(), task_ref: None, resume: None, ..LaunchRequest::relaunching(&record) };

        let refused = fleet.take_over(&request).unwrap_err();

        assert!(matches!(&refused, NorthError::Refused(why) if why == "north#1400 is already owned by outsider"), "{}", refused.user_message());
        assert_eq!(std::fs::read(fleet.root.join("slice/session.json")).unwrap(), before, "the refused take-over changed the old session's record");
        assert!(fleet.session("slice").unwrap().live() && !fleet.root.join(ARCHIVE).exists(), "the refused take-over stopped or archived the old session");
        assert!(claims.holders(Rows::Items(&["north#1054".to_owned()])).unwrap().contains(&("north#1054".to_owned(), "slice".to_owned())), "the refused take-over gave up the old session's claim");

        let home = tempfile::tempdir().unwrap();
        let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(())).showing_proxy_on(Arc::new(NoDesk));
        live(&fleet, PROXY);
        let mut record = fleet.session(PROXY).unwrap();
        record.role = Some(Role::Proxy);
        write_record(&fleet.root.join(PROXY), &record).unwrap();
        let before = std::fs::read(fleet.root.join(PROXY).join("session.json")).unwrap();

        let refused = fleet.take_over(&LaunchRequest { prompt: "carry on".into(), resume: None, ..LaunchRequest::relaunching(&record) });

        assert!(refused.is_err(), "{refused:?}");
        assert_eq!(std::fs::read(fleet.root.join(PROXY).join("session.json")).unwrap(), before, "the window-held take-over whose window never opened changed the old proxy's record");
        assert!(!fleet.root.join(ARCHIVE).exists(), "the window-held take-over whose window never opened archived the old proxy");
    }

    /// A ledger in which a claim another session holds is refused, as the threads ledger refuses one a live owner holds.
    #[derive(Default)]
    struct Owned(std::sync::Mutex<Vec<(String, String)>>);

    impl Claims for Owned {
        fn hold(&self, item: &str, session: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
            let mut held = self.0.lock().unwrap();
            if let Some((_, owner)) = held.iter().find(|(claim, owner)| claim == item && owner != session) {
                return Err(NorthError::Refused(format!("{item} is already owned by {owner}")));
            }
            held.retain(|(claim, owner)| !(claim == item && owner == session));
            held.push((item.to_owned(), session.to_owned()));
            Ok(Taken::Held)
        }

        fn release(&self, item: &str, session: &str) -> NorthResult<()> {
            self.0.lock().unwrap().retain(|(claim, owner)| !(claim == item && owner == session));
            Ok(())
        }

        fn holders(&self, rows: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Ok(self.0.lock().unwrap().iter().filter(|(claim, owner)| match rows {
                Rows::Items(items) => items.iter().any(|item| item == split_key(claim).0),
                Rows::Owners(owners) => owners.contains(owner),
            }).cloned().collect())
        }
    }

    /// A ledger that never answers: every claim waits in a spool under its session's name, and a withdraw drops that name's spooled claims, as the threads client does on a host that cannot reach the ledger.
    #[derive(Default)]
    struct Unanswered(std::sync::Mutex<Vec<(String, String)>>);

    impl Claims for Unanswered {
        fn hold(&self, item: &str, session: &str, _: &BTreeSet<String>) -> NorthResult<Taken> {
            self.0.lock().unwrap().push((item.to_owned(), session.to_owned()));
            Ok(Taken::Queued(format!("{session}-{item}")))
        }

        fn withdraw_queued(&self, session: &str) -> usize {
            let mut spooled = self.0.lock().unwrap();
            let before = spooled.len();
            spooled.retain(|(_, owner)| owner != session);
            before - spooled.len()
        }

        fn release(&self, _: &str, _: &str) -> NorthResult<()> {
            Err(NorthError::State("the threads ledger did not answer".into()))
        }

        fn holders(&self, _: Rows<'_>) -> NorthResult<Vec<(String, String)>> {
            Err(NorthError::State("the threads ledger did not answer".into()))
        }
    }

    /// On 2026-11-14 a take-over of p-agent-user-slice3 stopped and archived the lead, then was refused north#1397, which its child p-broker-code held.
    fn lead_whose_child_holds_an_issue(home: &Path) -> (Fleet, Arc<Owned>) {
        let claims = Arc::new(Owned::default());
        let fleet = Fleet::at_claiming(home.into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap().arming(|_| Ok(())).starting_hosts(standing_host);
        lead(&fleet, "slice", "north#1054");
        let mut record = fleet.session("slice").unwrap();
        record.extra_tasks = vec!["north#1397".into()];
        write_record(&fleet.root.join("slice"), &record).unwrap();
        live_under(&fleet, "broker", Some("slice"));
        claims.hold("north#1054", "slice", &BTreeSet::new()).unwrap();
        claims.hold("north#1397", "broker", &BTreeSet::new()).unwrap();
        (fleet, claims)
    }

    #[test]
    fn a_taken_over_lead_seat_keeps_its_tasks_and_its_childs_issues_and_a_take_over_racing_a_revive_leaves_one_running_session_in_its_own_conversation() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let dir = fleet.root.join("worker");
        std::fs::create_dir_all(&dir).unwrap();
        let parked = serde_json::json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "worker", "host_dir": dir, "stream_offset": 0});
        std::fs::write(dir.join("session.json"), parked.to_string()).unwrap();
        let row = fleet.new_row(RowKind::Message, "worker", "carry on with the build", &Sender::Tom).unwrap();

        let (predecessor, successor, revive) = fleet
            .succeed("worker", Claim::TakeOver, |seat| {
                let predecessor = seat.retire(&[])?;
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
        let (taken, claimed) = seat_taken_over(&fleet, "seat", None);
        assert_eq!((taken.role(), taken.tasks().collect::<Vec<_>>(), taken.claim_box.as_deref()), (Role::Lead, ["north#1054", "north#1055", "north#1056"].to_vec(), Some("old box")));
        assert_eq!(claimed, ["north#1054@old box", "north#1055", "north#1056"]);
        assert_eq!(fleet.granted_launch("seat", &["--name", "n1054-child", "--task", "north#1054", "brief"].map(str::to_owned)).unwrap().name, "n1054-child");
        let (boxed, claimed) = seat_taken_over(&fleet, "boxed", Some("new box"));
        assert_eq!((boxed.claim_box.as_deref(), claimed[0].as_str()), (Some("new box"), "north#1054@new box"));

        let bare = fleet.root.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(bare.join("session.json"), serde_json::json!({"name": "bare", "provider": "claude", "account": "a", "cwd": main, "model": null, "created_ms": 1, "host_pid": null, "conversation": "bare", "host_dir": bare, "stream_offset": 0, "role": "lead"}).to_string()).unwrap();
        let refused = fleet.granted_launch("bare", &["--name", "n1054-child", "--task", "north#1054", "brief"].map(str::to_owned)).unwrap_err();
        assert!(refused.contains("bare's record holds no task"), "{refused}");

        let home = tempfile::tempdir().unwrap();
        let (fleet, claims) = lead_whose_child_holds_an_issue(home.path());
        let request = LaunchRequest { prompt: "carry on".into(), task_ref: None, resume: None, ..LaunchRequest::relaunching(&fleet.session("slice").unwrap()) };
        let taken = fleet.take_over(&request).unwrap_or_else(|error| panic!("the take-over was refused: {}", error.user_message()));
        assert_eq!(taken.tasks().collect::<Vec<_>>(), ["north#1054", "north#1397"]);
        assert_eq!(claims.holders(Rows::Items(&["north#1397".to_owned()])).unwrap(), [("north#1397".to_owned(), "broker".to_owned())]);
        assert_eq!(claims.holders(Rows::Items(&["north#1054".to_owned()])).unwrap(), [("north#1054".to_owned(), "slice".to_owned())], "the old seat's retire gave up the claim its successor carries");
    }

    /// On 2026-11-14 the floor tick's take-over of the coordinator's window-held proxy was refused north#643, which a worker owned, after it had retired the old record.
    #[test]
    fn a_taken_over_proxy_carries_its_task_list_keeps_its_own_claims_held_or_spooled_and_leaves_another_sessions_item_to_it() {
        let home = tempfile::tempdir().unwrap();
        let claims = Arc::new(Owned::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), claims.clone()).unwrap().arming(|_| Ok(())).starting_hosts(standing_host);
        live(&fleet, PROXY);
        let mut record = fleet.session(PROXY).unwrap();
        (record.role, record.task_ref, record.extra_tasks) = (Some(Role::Proxy), Some("north#643".into()), vec!["north#1427".into()]);
        write_record(&fleet.root.join(PROXY), &record).unwrap();
        claims.hold("north#643", "north-643-worker", &BTreeSet::new()).unwrap();
        claims.hold("north#1427", PROXY, &BTreeSet::new()).unwrap();
        let request = LaunchRequest { prompt: "carry on".into(), resume: None, ..LaunchRequest::relaunching(&record) };
        let taken = fleet.take_over(&request).unwrap_or_else(|error| panic!("the proxy take-over was refused: {}", error.user_message()));
        assert_eq!(taken.tasks().collect::<Vec<_>>(), ["north#643", "north#1427"]);
        assert_eq!(claims.holders(Rows::Items(&["north#1427".to_owned()])).unwrap(), [("north#1427".to_owned(), PROXY.to_owned())], "the old proxy's retire gave up the claim its window-held successor carries");

        let home = tempfile::tempdir().unwrap();
        let spool = Arc::new(Unanswered::default());
        let fleet = Fleet::at_claiming(home.path().into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default()), spool.clone()).unwrap().arming(|_| Ok(()));
        live(&fleet, PROXY);
        let mut record = fleet.session(PROXY).unwrap();
        (record.role, record.task_ref) = (Some(Role::Proxy), Some("north#1427".into()));
        write_record(&fleet.root.join(PROXY), &record).unwrap();
        fleet.take_over(&LaunchRequest { prompt: "carry on".into(), resume: None, ..LaunchRequest::relaunching(&record) }).unwrap_or_else(|error| panic!("the proxy take-over was refused: {}", error.user_message()));
        assert!(spool.0.lock().unwrap().contains(&("north#1427".to_owned(), PROXY.to_owned())), "the old proxy's retire withdrew the claim its successor spooled under the same name");
        assert_eq!(claims.holders(Rows::Items(&["north#643".to_owned()])).unwrap(), [("north#643".to_owned(), "north-643-worker".to_owned())]);
    }
}
