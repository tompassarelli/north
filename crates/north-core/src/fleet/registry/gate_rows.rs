use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{self, Answerer, GateRequest};
use crate::fleet::model::Delivery;
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::store::{AnswerPayload, FleetRow, RowKind, Spool};

use super::{Fleet, HandedIn};

pub enum Opened {
    Here(i64),
    Spooled(Box<FleetRow>),
}

impl Fleet {
    pub fn open_or_spool_gate(&self, request: &GateRequest) -> NorthResult<Opened> {
        if self.session_dir(&request.session)?.exists() {
            return self.open_gate(request).map(Opened::Here);
        }
        request.validate()?;
        if !self.known_elsewhere(&request.session)? {
            return Err(NorthError::Usage(format!("no session named {}", request.session)));
        }
        let row = FleetRow { gate: Some(Box::new(request.clone())), ..self.new_row(RowKind::Gate, &request.session, &request.question, None)? };
        Spool::at(&self.root).put(&row)?;
        Ok(Opened::Spooled(Box::new(row)))
    }

    pub fn spool_gate_answer(&self, session: &str, gate: i64, number: usize, note: Option<&str>, caller: Option<&str>) -> NorthResult<FleetRow> {
        let row = FleetRow { answer: Some(AnswerPayload { gate, number }), ..self.new_row(RowKind::GateAnswer, session, note.unwrap_or_default(), caller)? };
        Spool::at(&self.root).put(&row)?;
        Ok(row)
    }

    pub fn answer_remote_gate(&self, label: &str, session: Option<&str>, number: usize, note: Option<&str>, caller: Option<&str>) -> NorthResult<String> {
        let (host, id) = label.rsplit_once(':').and_then(|(host, id)| Some((host, id.parse::<i64>().ok()?))).filter(|(host, _)| !host.is_empty()).ok_or_else(|| NorthError::Usage(format!("not a host-qualified gate id: {label}")))?;
        let session = match session {
            Some(session) => session.to_owned(),
            None => self.gates_elsewhere()?.into_iter().find(|gate| gate.host == host && gate.id == id).map(|gate| gate.request.session).ok_or_else(|| NorthError::Usage(format!("no open gate {label} in the cached fleet table; name its session: north gate answer {label} {number} --session NAME")))?,
        };
        let session = session.as_str();
        let held = self.held_elsewhere(session)?.ok_or_else(|| NorthError::Usage(format!("no session named {session} on another host")))?;
        if held.host != host {
            return Err(NorthError::Usage(format!("{session} runs on {}, not {host}", held.host)));
        }
        let row = self.spool_gate_answer(session, id, number, note, caller)?;
        Ok(format!("gate {label}: answer {number} spooled as {} for {session}'s host to apply", row.id))
    }

    pub fn deliver_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        match row.kind {
            RowKind::Message | RowKind::JobExit => self.hand_in(row),
            RowKind::Control => self.apply_control(row),
            RowKind::Gate => self.open_gate_row(row),
            RowKind::GateAnswer => self.apply_gate_answer(row),
            kind => Err(NorthError::Usage(format!("a {} row is not delivered into a session", kind.label()))),
        }
    }

    fn open_gate_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let request = row.gate.as_deref().ok_or_else(|| NorthError::Usage(format!("GATE row {} carries no gate", row.id)))?;
        if request.session != row.to {
            return Err(NorthError::Usage(format!("GATE row {} is addressed to {} but asks for {}", row.id, row.to, request.session)));
        }
        self.journaled(row, || {
            let id = self.record_gate(request)?;
            if let Err(error) = (self.arm)(id) {
                eprintln!("north fleet: gate {id} for {} opened but its default timer did not start: {}", request.session, error.user_message());
            }
            Ok(())
        })
    }

    fn apply_gate_answer(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let AnswerPayload { gate, number } = row.answer.ok_or_else(|| NorthError::Usage(format!("GATE-ANSWER row {} carries no answer", row.id)))?;
        let note = Some(row.text.as_str()).filter(|note| !note.trim().is_empty());
        let mut told = None;
        let mut answers = Vec::new();
        let handed = self.journaled(row, || match self.answer_row_gate(row, gate, number, note, |session, text| {
            answers.push(self.spool_answer(session, text)?);
            Ok(())
        }) {
            Err(NorthError::GateAnswered { .. }) if self.closed_by_this_answer(row, gate, number, note) => {
                told = Some(format!("your answer {number} to gate {gate} was applied"));
                Ok(())
            }
            Err(error @ (NorthError::Usage(_) | NorthError::Rejected(_) | NorthError::GateAnswered { .. })) => {
                told = Some(format!("your answer {number} to gate {gate} was not applied: {}", error.user_message()));
                Ok(())
            }
            outcome => outcome,
        })?;
        for answer in &answers {
            self.hand_in_answer(answer)?;
        }
        if let (Some(text), Some(answerer)) = (told, row.sender.as_deref()) {
            self.tell(answerer, &text, Delivery::Queued);
        }
        Ok(handed)
    }

    fn closed_by_this_answer(&self, row: &FleetRow, gate: i64, number: usize, note: Option<&str>) -> bool {
        let by = row.sender.as_deref().map_or(Answerer::Tom, |name| Answerer::Session(name.to_owned()));
        self.gate(gate).ok().flatten().is_some_and(|gate| gates::closed_by_answer(&gate, &by, number, note))
    }

    fn answer_row_gate(&self, row: &FleetRow, gate: i64, number: usize, note: Option<&str>, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<()> {
        let asked = self.gate(gate)?.ok_or_else(|| NorthError::Usage(format!("no gate {gate}")))?;
        if asked.request.session != row.to {
            return Err(NorthError::Usage(format!("gate {gate} belongs to {}, not {}", asked.request.session, row.to)));
        }
        self.answer_gate_with(gate, number, note, row.sender.as_deref(), deliver).map(drop)
    }

    pub(super) fn answer_session(&self, session: &str, text: &str) -> NorthResult<()> {
        let row = self.spool_answer(session, text)?;
        self.hand_in_answer(&row)
    }

    fn spool_answer(&self, session: &str, text: &str) -> NorthResult<FleetRow> {
        let row = FleetRow { urgent: true, ..self.new_row(RowKind::Message, session, text, None)? };
        self.spool().put(&row)?;
        Ok(row)
    }

    fn hand_in_answer(&self, row: &FleetRow) -> NorthResult<()> {
        match self.hand_in(row) {
            Ok(_) => self.spool().withdraw(&row.id),
            Err(error) => {
                eprintln!("north fleet: the answer to {} is spooled as row {} and the tick will hand it in: {}", row.to, row.id, error.user_message());
                Ok(())
            }
        }
    }

    pub(super) fn new_row(&self, kind: RowKind, to: &str, text: &str, sender: Option<&str>) -> NorthResult<FleetRow> {
        let sent_at = now_ms();
        Ok(FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind,
            to: to.into(),
            from: self.host.clone(),
            urgent: false,
            sent_at,
            text: text.into(),
            sender: sender.map(str::to_owned),
            gate: None,
            answer: None,
            job: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live, live_under};
    use super::*;
    use crate::fleet::store::{SessionRow, cache_sessions};

    fn request(session: &str) -> GateRequest {
        GateRequest { verification: false, issue: None, session: session.into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 60_000 }
    }

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "laptop".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
        cache_sessions(&fleet.root, &rows).unwrap();
    }

    fn spooled(fleet: &Fleet) -> FleetRow {
        let mut pending = Spool::at(&fleet.root).pending().unwrap();
        assert_eq!(pending.len(), 1);
        pending.remove(0)
    }

    #[test]
    fn a_gate_for_a_session_this_registry_lacks_is_spooled_as_one_gate_row() {
        let home = tempfile::tempdir().unwrap();
        let far = fleet(home.path());
        cached(&far, &["worker"]);

        let Opened::Spooled(row) = far.open_or_spool_gate(&request("worker")).unwrap() else { panic!("a session of another host is spooled") };

        assert_eq!((row.kind, row.to.as_str(), row.text.as_str()), (RowKind::Gate, "worker", "Rent the box?"));
        assert_eq!(row.gate.as_deref(), Some(&request("worker")));
        assert_eq!(spooled(&far), *row);
        assert!(matches!(far.open_or_spool_gate(&request("nobody")), Err(NorthError::Usage(why)) if why == "no session named nobody"));
        assert!(matches!(far.open_or_spool_gate(&GateRequest { options: vec![], ..request("worker") }), Err(NorthError::Usage(_))));
        assert_eq!(Spool::at(&far.root).pending().unwrap().len(), 1);
    }

    #[test]
    fn a_gate_for_a_session_in_this_registry_opens_at_once_and_returns_its_id() {
        let home = tempfile::tempdir().unwrap();
        let here = fleet(home.path());
        let _worker = live(&here, "worker");

        let opened = here.open_gate(&request("worker")).unwrap();

        assert_eq!(opened, 1);
        assert!(Spool::at(&here.root).pending().unwrap().is_empty());
    }

    #[test]
    fn the_ticks_delivery_of_a_gate_row_opens_the_gate_on_the_sessions_host_exactly_once_by_row_id() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let _worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.open_or_spool_gate(&request("worker")).unwrap();
        let row = spooled(&far);

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);
        assert_eq!(laptop.deliver_row(&row.clone()).unwrap(), HandedIn::Already);

        let gates = laptop.digest(0).unwrap().gates;
        assert_eq!(gates.iter().map(|gate| (gate.request.session.as_str(), gate.request.question.as_str(), gate.is_open())).collect::<Vec<_>>(), [("worker", "Rent the box?", true)]);
    }

    #[test]
    fn a_gate_row_for_a_session_not_yet_here_is_not_journaled_so_a_later_tick_opens_it() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        cached(&far, &["worker"]);
        far.open_or_spool_gate(&request("worker")).unwrap();
        let row = spooled(&far);

        assert!(laptop.deliver_row(&row).is_err());
        let _worker = live(&laptop, "worker");

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);
        assert_eq!(laptop.digest(0).unwrap().gates.len(), 1);
    }

    #[test]
    fn a_gate_whose_timer_fails_to_start_is_opened_once_and_its_row_is_not_retried() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()).arming(|_| Err(NorthError::State("no timer".into()))));
        let _worker = live(&laptop, "worker");
        cached(&far, &["worker"]);
        far.open_or_spool_gate(&request("worker")).unwrap();
        let row = spooled(&far);

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);

        assert_eq!(laptop.digest(0).unwrap().gates.len(), 1);
    }

    #[test]
    fn a_gate_row_naming_another_session_than_it_is_addressed_to_is_refused() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let _worker = live(&laptop, "worker");
        let row = FleetRow { gate: Some(Box::new(request("other"))), ..laptop.new_row(RowKind::Gate, "worker", "Rent the box?", None).unwrap() };

        assert!(matches!(laptop.deliver_row(&row), Err(NorthError::Usage(_))));
        assert!(laptop.digest(0).unwrap().gates.is_empty());
    }

    #[test]
    fn an_answer_given_on_any_host_is_a_gate_answer_row_that_the_sessions_host_applies_once() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let worker = live(&laptop, "worker");
        let id = laptop.open_gate(&request("worker")).unwrap();
        cached(&far, &["worker"]);

        far.answer_remote_gate(&format!("laptop:{id}"), Some("worker"), 1, Some("but only today"), None).unwrap();
        let row = spooled(&far);

        assert_eq!((row.kind, row.to.as_str(), row.answer, row.text.as_str(), row.sender.as_deref()), (RowKind::GateAnswer, "worker", Some(AnswerPayload { gate: id, number: 1 }), "but only today", None));
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);

        let closure = laptop.gate(id).unwrap().unwrap().closure.unwrap();
        assert_eq!((closure.answer.as_str(), closure.by), ("yes\nbut only today", crate::fleet::gates::Answerer::Tom));
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("Tom answered: yes"), "{}", frames[0]);
    }

    #[test]
    fn a_gate_closed_while_its_answer_could_not_be_handed_in_still_reaches_the_session_when_the_tick_hands_the_spooled_row_in() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let worker = live(&laptop, "worker");
        let id = laptop.open_gate(&request("worker")).unwrap();
        let journal = laptop.root.join("worker/delivered");
        std::fs::write(&journal, "in the way").unwrap();

        laptop.answer_gate(id, 1, None, None).unwrap();

        assert!(laptop.gate(id).unwrap().unwrap().closure.is_some());
        assert!(worker.frames.lock().unwrap().is_empty());
        let pending = spooled(&laptop);
        assert_eq!((pending.kind, pending.to.as_str(), pending.urgent), (RowKind::Message, "worker", true));
        std::fs::remove_file(&journal).unwrap();

        assert_eq!(laptop.deliver_row(&pending).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&pending).unwrap(), HandedIn::Already);
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("Tom answered: yes"), "{}", frames[0]);
    }

    #[test]
    fn a_gate_answer_handed_in_at_once_leaves_nothing_in_the_spool() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let worker = live(&laptop, "worker");
        let id = laptop.open_gate(&request("worker")).unwrap();

        laptop.answer_gate(id, 2, None, None).unwrap();

        assert!(Spool::at(&laptop.root).pending().unwrap().is_empty());
        assert!(worker.frames.lock().unwrap()[0].contains("Tom answered: no"));
    }

    #[test]
    fn an_answer_row_from_a_session_that_is_not_the_parent_closes_nothing_and_is_consumed() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let worker = live_under(&laptop, "worker", Some("boss"));
        let id = laptop.open_gate(&request("worker")).unwrap();
        let row = laptop.spool_gate_answer("worker", id, 1, None, Some("intruder")).unwrap();
        let by_the_gate_session = laptop.spool_gate_answer("worker", id, 1, None, Some("worker")).unwrap();

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);
        assert_eq!(laptop.deliver_row(&by_the_gate_session).unwrap(), HandedIn::Delivered);

        assert!(laptop.gate(id).unwrap().unwrap().is_open());
        let frames = worker.frames.lock().unwrap();
        assert_eq!(frames.len(), 1, "only the gate session itself is told its answer was refused: {frames:?}");
        assert!(frames[0].contains("your answer 1 to gate 1 was not applied"), "{}", frames[0]);
    }

    #[test]
    fn a_gate_answer_retried_after_its_journal_write_failed_tells_the_answerer_it_was_applied() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let worker = live_under(&laptop, "worker", Some("boss"));
        let boss = live(&laptop, "boss");
        let id = laptop.open_gate(&request("worker")).unwrap();
        let row = laptop.spool_gate_answer("worker", id, 2, None, Some("boss")).unwrap();
        laptop.deliver_row(&FleetRow { id: "1-other".into(), ..laptop.new_row(RowKind::Message, "worker", "hello", None).unwrap() }).unwrap();
        let journal = laptop.root.join("worker/delivered");
        std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o555)).unwrap();

        assert!(laptop.deliver_row(&row).is_err());
        std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);

        assert_eq!(laptop.gate(id).unwrap().unwrap().closure.unwrap().answer, "no");
        let frames = boss.frames.lock().unwrap();
        let told: Vec<&String> = frames.iter().filter(|frame| frame.contains("your answer")).collect();
        assert_eq!(told.len(), 1, "{frames:?}");
        assert!(told[0].contains("your answer 2 to gate 1 was applied") && !told[0].contains("not applied"), "{}", told[0]);
        let waiting = Spool::at(&laptop.root).pending().unwrap().into_iter().filter(|row| row.kind == RowKind::Message && row.to == "worker" && row.urgent).count();
        assert_eq!(waiting, 1, "the answer itself stays spooled for the tick");
        drop(worker);
    }

    #[test]
    fn a_gate_answer_that_reaches_a_gate_another_answerer_closed_still_tells_its_answerer_it_was_not_applied() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let _worker = live_under(&laptop, "worker", Some("boss"));
        let boss = live(&laptop, "boss");
        let id = laptop.open_gate(&request("worker")).unwrap();
        laptop.answer_gate(id, 1, None, None).unwrap();
        let row = laptop.spool_gate_answer("worker", id, 2, None, Some("boss")).unwrap();

        laptop.deliver_row(&row).unwrap();

        let frames = boss.frames.lock().unwrap();
        let told: Vec<&String> = frames.iter().filter(|frame| frame.contains("your answer")).collect();
        assert!(told.len() == 1 && told[0].contains("your answer 2 to gate 1 was not applied"), "{frames:?}");
    }

    #[test]
    fn an_answer_row_from_the_gate_sessions_parent_closes_the_gate_as_that_parent() {
        let home = tempfile::tempdir().unwrap();
        let laptop = fleet(home.path());
        let worker = live_under(&laptop, "worker", Some("boss"));
        let id = laptop.open_gate(&request("worker")).unwrap();
        let row = laptop.spool_gate_answer("worker", id, 2, None, Some("boss")).unwrap();

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);

        let closure = laptop.gate(id).unwrap().unwrap().closure.unwrap();
        assert_eq!((closure.answer.as_str(), closure.by), ("no", crate::fleet::gates::Answerer::Session("boss".into())));
        assert!(worker.frames.lock().unwrap()[0].contains("boss answered: no"));
    }

    #[test]
    fn an_answer_row_for_a_gate_of_another_session_is_refused_and_a_remote_answer_needs_a_host_qualified_gate_and_a_session_that_host_runs() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let _worker = live(&laptop, "worker");
        let _other = live(&laptop, "other");
        let id = laptop.open_gate(&request("worker")).unwrap();
        let wrong = laptop.spool_gate_answer("other", id, 1, None, None).unwrap();
        cached(&far, &["worker"]);

        laptop.deliver_row(&wrong).unwrap();

        assert!(laptop.gate(id).unwrap().unwrap().is_open());
        assert!(matches!(far.answer_remote_gate("laptop", Some("worker"), 1, None, None), Err(NorthError::Usage(_))));
        assert!(matches!(far.answer_remote_gate(&format!("mars:{id}"), Some("worker"), 1, None, None), Err(NorthError::Usage(_))));
        assert!(matches!(far.answer_remote_gate(&format!("laptop:{id}"), Some("nobody"), 1, None, None), Err(NorthError::Usage(_))));
        assert_eq!(Spool::at(&far.root).pending().unwrap().len(), 0);
    }
}
