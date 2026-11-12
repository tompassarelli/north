use crate::error::{NorthError, NorthResult};
use crate::fleet::fold::{Admit, Condition, Route, admit_via, told};
use crate::fleet::model::{Delivery, Status};
use crate::fleet::runtime::{PROXY, new_uuid, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind, SessionRow, Spool, cached_sessions};
use north_base::platform::land_notice::OWNER;

use super::Fleet;

#[derive(Debug, Eq, PartialEq)]
pub enum Sent {
    HandedIn,
    Spooled { id: String, host: String },
    Queued { id: String },
    Retired { heir: String },
    Unreached,
    Held,
    Flooded { line: String },
}

impl Fleet {
    pub fn reaches(&self, session: &str) -> NorthResult<()> {
        if self.session_dir(session)?.exists() || self.held_elsewhere(session)?.is_some() {
            return Ok(());
        }
        Err(NorthError::Usage(format!("no session named {session}")))
    }

    /// The spool this process writes: a worker's own location, else the fleet's shared one.
    pub fn spool(&self) -> Spool {
        self.spools_as.as_deref().and_then(|session| Spool::of(&self.root, session).ok()).unwrap_or_else(|| Spool::at(&self.root))
    }

    pub fn held_since(&self, since_ms: u64) -> Vec<crate::fleet::held::Held> {
        crate::fleet::held::since(&self.root, since_ms)
    }

    pub fn send_message(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        self.send_condition(from, name, text, None, delivery)
    }

    /// Sends `text` as `send_message` does, naming the standing condition it reports so the store delivers it once per episode.
    pub fn send_condition(&self, from: &Sender, name: &str, text: &str, condition: Option<&Condition>, delivery: Delivery) -> NorthResult<Sent> {
        let now = now_ms();
        let absent = matches!(from, Sender::Service(_)) && name == OWNER && !self.addressable(name);
        let spool = self.spool();
        let route = Route { to: name, recipient: if absent { PROXY } else { name }, spooled: &|id| spool.holds(id) };
        match admit_via(&self.root, from, &route, text, condition, now) {
            Admit::Send => {}
            Admit::Fold(folded) => {
                if let Some(earlier) = &folded.replaces {
                    spool.withdraw(earlier)?;
                }
                let row = self.spool_message(from, route.recipient, &folded.text, delivery)?;
                told(&self.root, &folded.key, &row.id);
                return Ok(Sent::Queued { id: row.id });
            }
            Admit::Drop => return Ok(Sent::Held),
            Admit::Hold => {
                crate::fleet::held::hold(&self.root, name, &from.to_string(), text, now);
                return Ok(Sent::Held);
            }
            Admit::Flood(line) => {
                crate::fleet::held::hold(&self.root, name, &from.to_string(), text, now);
                self.deliver_message(from, name, &line, delivery)?;
                return Ok(Sent::Flooded { line });
            }
        }
        crate::fleet::alarms::record(&self.root, from, text, now);
        if absent {
            self.log_dead_letter(name, PROXY);
            return self.deliver_message(from, PROXY, &format!("(for {name}, not live) {text}"), delivery).map(|_| Sent::Unreached);
        }
        self.deliver_message(from, name, text, delivery)
    }

    fn deliver_message(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<Sent> {
        if self.session_dir(name)?.exists() {
            return self.hand_in_signed(from, name, text, delivery);
        }
        if let Some(heir) = self.heir(name)? {
            self.deliver_message(from, &heir, &self.dead_letter(name, &heir, text), delivery)?;
            return Ok(Sent::Retired { heir });
        }
        let held = self.held_elsewhere(name)?.ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?;
        let row = self.spool_message(from, name, text, delivery)?;
        Ok(Sent::Spooled { id: row.id, host: held.host })
    }

    fn addressable(&self, name: &str) -> bool {
        self.session_dir(name).is_ok_and(|dir| dir.exists()) || self.held_elsewhere(name).is_ok_and(|held| held.is_some())
    }

    pub(super) fn known_elsewhere(&self, name: &str) -> NorthResult<bool> {
        Ok(self.held_elsewhere(name)?.is_some())
    }

    pub(super) fn held_elsewhere(&self, name: &str) -> NorthResult<Option<SessionRow>> {
        Ok(cached_sessions(&self.root)?.unwrap_or_default().into_iter().find(|session| session.name == name && session.host != self.host && session.status != Status::Exited.label()))
    }

    pub(super) fn spool_message(&self, from: &Sender, name: &str, text: &str, delivery: Delivery) -> NorthResult<FleetRow> {
        let sent_at = now_ms();
        let row = FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind: RowKind::Message,
            to: name.into(),
            from: self.host.clone(),
            urgent: delivery == Delivery::Now,
            sent_at,
            text: text.into(),
            sender: from.clone(),
            gate: None,
            answer: None,
            job: None,
            origin: None,
            spooled_by: None,
        };
        self.spool().put(&row)?;
        Ok(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixtures::{fleet, live};
    use crate::fleet::store::cache_sessions;
    use crate::fleet::registry::HandedIn;

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "far-host".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
        cache_sessions(&fleet.root, &rows).unwrap();
    }

    #[test]
    fn a_send_to_a_session_here_spools_one_row_and_hands_it_in_once_however_often_the_row_is_handed_in_again() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");

        assert_eq!(fleet.send_message(&Sender::Session("lead".into()), "worker", "hello", Delivery::Queued).unwrap(), Sent::HandedIn);
        let spooled = Spool::at(&fleet.root).pending().unwrap();

        assert_eq!(spooled.len(), 1);
        assert_eq!((spooled[0].to.as_str(), spooled[0].text.as_str(), &spooled[0].sender), ("worker", "hello", &Sender::Session("lead".into())));
        assert_eq!(fleet.hand_in(&spooled[0]).unwrap(), HandedIn::Already);
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("[from lead] hello"), "{}", frames[0]);
    }

    #[test]
    fn a_message_from_tom_reaches_the_session_unprefixed_and_any_other_sender_is_named() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");

        fleet.send_signed(&Sender::Tom, "worker", "ship it", Delivery::Queued).unwrap();
        fleet.send_signed(&Sender::Service("north-ops-check".into()), "worker", "tick", Delivery::Queued).unwrap();
        fleet.send_signed(&Sender::Session("lead".into()), "worker", "also", Delivery::Queued).unwrap();

        let rows = Spool::at(&fleet.root).pending().unwrap();
        for sender in [Sender::Tom, Sender::Service("north-ops-check".into()), Sender::Session("lead".into())] {
            assert!(rows.iter().any(|row| row.sender == sender), "{sender:?}");
        }
        let frames = worker.frames.lock().unwrap();
        let frame = |text: &str| frames.iter().find(|frame| frame.contains(text)).unwrap_or_else(|| panic!("{text}: {frames:?}")).clone();
        assert!(!frame("ship it").contains("[from"), "{frames:?}");
        assert!(frame("tick").contains("[from service:north-ops-check] tick"));
        assert!(frame("also").contains("[from lead] also"));
    }

    #[test]
    fn tom_on_another_host_is_still_unprefixed_where_a_service_names_its_host() {
        let home = tempfile::tempdir().unwrap();
        let far_home = tempfile::tempdir().unwrap();
        let (laptop, far) = (fleet(home.path()).hosted_as("laptop"), fleet(far_home.path()).hosted_as("far"));
        let worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.send_message(&Sender::Tom, "worker", "from the TUI", Delivery::Queued).unwrap();
        far.send_message(&Sender::Service("tick".into()), "worker", "from a timer", Delivery::Queued).unwrap();
        for row in Spool::at(&far.root).pending().unwrap() {
            laptop.hand_in(&row).unwrap();
        }
        let frames = worker.frames.lock().unwrap();
        assert!(frames.iter().any(|frame| frame.contains("from the TUI") && !frame.contains("[from")), "{frames:?}");
        assert!(frames.iter().any(|frame| frame.contains("[from service:tick@far] from a timer")), "{frames:?}");
    }

    #[test]
    fn a_hand_in_that_fails_is_withdrawn_from_the_spool_and_the_row_can_be_handed_in_later() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let worker = live(&fleet, "worker");
        let socket = north_base::platform::session_host_files::socket_path(&fleet.root.join("worker"));
        let parked = socket.with_extension("moved");
        std::fs::rename(&socket, &parked).unwrap();

        assert!(fleet.send_signed(&Sender::Tom, "worker", "hello", Delivery::Queued).is_err());
        assert!(Spool::at(&fleet.root).pending().unwrap().is_empty());
        let row = fleet.spool_message(&Sender::Tom, "worker", "again", Delivery::Queued).unwrap();
        assert!(fleet.hand_in(&row).is_err());
        std::fs::rename(&parked, &socket).unwrap();

        assert_eq!(fleet.hand_in(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(worker.frames.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_service_send_to_the_landers_owner_reaches_it_while_live_else_the_proxy_prefixed_with_its_name_and_recorded() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let (proxy, owner) = (live(&fleet, "proxy"), live(&fleet, OWNER));
        let lander = Sender::Service("north-land".into());

        assert_eq!(fleet.send_message(&lander, OWNER, "lander stalled", Delivery::Queued).unwrap(), Sent::HandedIn);
        assert!(owner.frames.lock().unwrap().iter().any(|frame| frame.contains("[from service:north-land] lander stalled")));
        assert!(proxy.frames.lock().unwrap().is_empty());

        fleet.forget(OWNER).unwrap();
        assert_eq!(fleet.send_message(&lander, OWNER, "lander stalled again", Delivery::Queued).unwrap(), Sent::Unreached);
        let frames = proxy.frames.lock().unwrap();
        assert!(frames.iter().any(|frame| frame.contains(&format!("[from service:north-land] (for {OWNER}, not live) lander stalled again"))), "{frames:?}");
        let log = std::fs::read_to_string(fleet.root.join(".dead-letters.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 1, "{log}");
    }

    #[test]
    fn a_hand_in_nested_in_another_to_the_same_session_is_delivered_instead_of_waiting_on_its_lock() {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let home = tempfile::tempdir().unwrap();
            let fleet = fleet(home.path());
            let worker = live(&fleet, "worker");
            let row = fleet.spool_message(&Sender::Tom, "worker", "outer", Delivery::Queued).unwrap();
            let handed = fleet.journaled(&row, || fleet.send_signed(&Sender::fleet(), "worker", "nested", Delivery::Queued));
            done.send((handed.map_err(|error| error.user_message()), worker.frames.lock().unwrap().clone())).unwrap();
        });
        let (handed, frames) = finished.recv_timeout(std::time::Duration::from_secs(20)).expect("the nested hand-in waited on its own delivery lock");
        assert_eq!(handed, Ok(HandedIn::Delivered));
        assert!(frames.iter().any(|frame| frame.contains("nested")), "{frames:?}");
    }

    #[test]
    fn a_hand_in_gives_up_on_a_delivery_lock_another_holds_and_leaves_the_row_for_the_next_tick() {
        use std::{sync::mpsc::channel, thread::spawn, time::Duration};
        let home = tempfile::tempdir().unwrap();
        let _worker = live(&fleet(home.path()), "worker");
        let ((ready, readied), (go, going), (gave_up, gave)) = (channel(), channel::<()>(), channel());
        let root = home.path().to_owned();
        spawn(move || {
            let laptop = fleet(&root);
            let outer = laptop.spool_message(&Sender::Tom, "worker", "outer", Delivery::Queued).unwrap();
            laptop.journaled(&outer, || laptop.send_signed(&Sender::fleet(), "worker", "nested", Delivery::Queued)).unwrap();
            ready.send(()).unwrap();
            going.recv().unwrap();
            let row = laptop.spool_message(&Sender::Tom, "worker", "late", Delivery::Queued).unwrap();
            gave_up.send(laptop.hand_in(&row).map_err(|error| error.user_message())).unwrap();
        });
        readied.recv_timeout(Duration::from_secs(10)).expect("the first hand-in never finished");
        let ((held, holding), (release, released)) = (channel(), channel::<()>());
        let root = home.path().to_owned();
        spawn(move || {
            let other = fleet(&root);
            let row = other.spool_message(&Sender::Tom, "worker", "held", Delivery::Queued).unwrap();
            other.journaled(&row, || {
                held.send(()).unwrap();
                let _ = released.recv();
                Ok(())
            })
        });
        holding.recv_timeout(Duration::from_secs(10)).expect("the holder never took the delivery lock");
        go.send(()).unwrap();
        let handed = gave.recv_timeout(Duration::from_secs(10)).expect("the hand-in waited on the held delivery lock without a bound");
        release.send(()).unwrap();

        assert!(handed.is_err(), "{handed:?}");
    }

    #[test]
    fn an_interim_worker_turn_and_a_cleared_alert_go_to_the_digest_not_the_proxy() {
        use crate::fleet::receipts::{Report, deliver_report};
        use crate::fleet::turns::TurnExit;
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let proxy = live(&fleet, PROXY);
        let books = crate::fleet::fakes::Paper::default();
        let turn = |turn, reply: &str| deliver_report(&books, &books, &fleet, PROXY, &Report { child: "broker-94".into(), turn, exit: TurnExit::Completed, landed: Some("be5886ab3b".into()), reply: Some(reply.into()), waiting: false }, &[], 1).unwrap();
        let (watch, health) = (Sender::Service("north-land-watch@gex45".into()), Sender::Service("north-health".into()));
        let red = "north main is red: Cargo failed on main commit 4e387709b975";
        let alert = |sender: &Sender, text: &str| fleet.send_message(sender, PROXY, text, Delivery::Queued).unwrap();

        assert!(!turn(1, "Landing in progress; waiting for the runtime update."), "north#1019: broker-94's interim turns woke the proxy on 2026-11-09");
        assert_eq!(alert(&watch, &format!("north ops watch: {red}")), Sent::HandedIn);
        assert_eq!(alert(&watch, &format!("north ops watch: still, 12 min: {red}")), Sent::Held);
        assert_eq!(alert(&watch, &format!("north ops watch: cleared after 13 min: {red}")), Sent::Held);
        assert_eq!(alert(&health, "health: ci: recovered"), Sent::Held);
        assert!(!turn(2, "Done: landed be5886ab3b.\nNext: none"), "a Done report waits in the digest");
        assert_eq!(alert(&watch, &format!("north ops watch: {red}")), Sent::HandedIn, "a cleared cause opens again");
        let lander = Sender::Service("north-land".into());
        assert_eq!((alert(&lander, "landed a82759cff8 n909-policy"), alert(&lander, "landed a82759cff8 n909-policy")), (Sent::HandedIn, Sent::HandedIn), "a landing notice is no alarm");

        let frames = proxy.frames.lock().unwrap();
        assert_eq!(frames.len(), 4, "{frames:?}");
        assert!(frames.iter().all(|frame| frame.contains("main is red") || frame.contains("landed a82759cff8")), "{frames:?}");
        let held: Vec<String> = fleet.held_since(0).into_iter().map(|held| held.text).collect();
        assert_eq!(held.len(), 5, "{held:?}");
        assert!(held[0].contains("Landing in progress") && held[2].contains("cleared after 13 min") && held[3] == "health: ci: recovered" && held[4].contains("Done: landed be5886ab3b"), "{held:?}");

        use crate::fleet::fold::{WINDOW_MS, admit};
        let watchdog = Sender::Service("north-capacity-watchdog".into());
        let alarm = |text: &str, at: u64| admit(home.path(), &watchdog, PROXY, text, None, at);
        assert_eq!(alarm("watchdog: host disk past its high watermark: 3.1 GiB free on /nix/store", 0), Admit::Send);
        assert_eq!(alarm("watchdog: host disk past its high watermark: 2.9 GiB free on /nix/store", 60_000), Admit::Hold, "north#909: one disk alarm re-sent with moved numbers");
        assert_eq!(alarm("watchdog: host disk past its high watermark: 2.1 GiB free on /nix/store", 60_000 + WINDOW_MS), Admit::Send, "a new window sends again");
        assert_eq!(alarm("watchdog: host overload, load 107.96 (4.5/core), cpu pressure 72.02% over 60 s.", 0), Admit::Hold, "an overload is not page-now and waits for the digest");
        assert_eq!(admit(home.path(), &health, PROXY, "harness unit fleet-tick.service recovered: under 3 failed starts in the last hour", None, 0), Admit::Send, "one cleared line for a health unit");
    }

    #[test]
    fn one_lander_behind_main_line_sent_ten_times_for_one_unchanged_link_reaches_the_proxy_once() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let proxy = live(&fleet, PROXY);
        let lander = Sender::Service("north-land".into());
        let behind = Condition { key: "lander-build-behind".into(), cleared: false };
        let line = |main: &str| format!("north land: the lander runs older build cb929a45e8, 4 lander changes behind main {main}, over the limit of 3; lanes are judged by lander code main has replaced, so read why north land promote holds the lander link");
        let sent: Vec<Sent> = (0..10).map(|round| fleet.send_condition(&lander, PROXY, &line(&format!("f9fb98f95{round}")), Some(&behind), Delivery::Queued).unwrap()).collect();

        assert_eq!(sent.iter().filter(|sent| **sent == Sent::HandedIn).count(), 1, "{sent:?}");
        assert_eq!(proxy.frames.lock().unwrap().len(), 1);
    }

    #[test]
    fn an_alarm_re_sent_with_a_fresh_runtime_sha_or_canary_waits_in_the_digest() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let proxy = live(&fleet, PROXY);
        let health = Sender::Service("north-health".into());
        let alert = |sha: &str, canary: &str| {
            let text = format!("harness unit north-runtime-follow.service failed to start 13 times in the last hour: runtime {sha} for {canary}");
            fleet.send_message(&health, PROXY, &text, Delivery::Queued).unwrap()
        };

        assert_eq!(alert("636c151b", "canary-opencode-4101624"), Sent::HandedIn);
        assert_eq!(alert("22272a0b", "canary-opencode-4123229"), Sent::Held, "a fresh runtime sha and canary session id are the same cause");
        assert_eq!(proxy.frames.lock().unwrap().len(), 1);
        let held: Vec<String> = fleet.held_since(0).into_iter().map(|held| held.text).collect();
        assert_eq!(held.len(), 1);
        assert!(held[0].contains("22272a0b"), "{held:?}");
    }
}
