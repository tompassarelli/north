use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{self, Answerer, GateRequest};
use crate::fleet::model::Delivery;
use crate::fleet::runtime::{new_uuid, now_ms};
use crate::fleet::sender::Sender;
use crate::fleet::store::{AnswerPayload, FleetRow, RowKind, Spool};

use super::{Fleet, HandedIn};

pub enum Opened {
    Here(i64),
    Spooled(Box<FleetRow>),
}

impl Fleet {
    pub fn open_or_spool_gate(&self, request: &GateRequest, from: &Sender) -> NorthResult<Opened> {
        if self.session_dir(&request.session)?.exists() {
            return self.open_gate(request).map(Opened::Here);
        }
        request.validate()?;
        if !self.known_elsewhere(&request.session)? {
            return Err(NorthError::Usage(format!("no session named {}", request.session)));
        }
        let row = FleetRow { gate: Some(Box::new(request.clone())), ..self.new_row(RowKind::Gate, &request.session, &request.question, from)? };
        Spool::at(&self.root).put(&row)?;
        Ok(Opened::Spooled(Box::new(row)))
    }

    pub fn spool_gate_answer(&self, session: &str, gate: i64, number: usize, note: Option<&str>, caller: Option<&str>) -> NorthResult<FleetRow> {
        let row = FleetRow { answer: Some(AnswerPayload { gate, number }), ..self.new_row(RowKind::GateAnswer, session, note.unwrap_or_default(), &Sender::typed(caller.map(str::to_owned)))? };
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
        let held = self.cached_sessions()?.unwrap_or_default().into_iter().find(|row| row.name == session && row.host != self.host).ok_or_else(|| NorthError::Usage(format!("no session named {session} on another host")))?;
        if held.host != host {
            return Err(NorthError::Usage(format!("{session} runs on {}, not {host}", held.host)));
        }
        gates::Authority { caller, parent: held.parent.as_deref() }.admit()?;
        let row = self.spool_gate_answer(session, id, number, note, caller)?;
        Ok(format!("gate {label}: answer {number} spooled as {} for {session}'s host to apply", row.id))
    }

    pub fn deliver_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        match row.kind {
            RowKind::Message | RowKind::JobExit => self.hand_in(row),
            RowKind::Control => self.apply_control(row),
            RowKind::Gate => self.open_gate_row(row),
            RowKind::GateAnswer => self.apply_gate_answer(row),
            RowKind::Wait => self.register_wait_row(row),
            RowKind::Spend => self.answer_spend_row(row),
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
        if let Sender::Service(unit) = &row.sender {
            return Err(NorthError::Usage(format!("GATE-ANSWER row {} is from service {unit}, which cannot answer a gate", row.id)));
        }
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
        if let (Some(text), Some(answerer)) = (told, row.sender.session()) {
            self.tell(answerer, &text, Delivery::Queued);
        }
        Ok(handed)
    }

    fn closed_by_this_answer(&self, row: &FleetRow, gate: i64, number: usize, note: Option<&str>) -> bool {
        let by = row.sender.session().map_or(Answerer::Tom, |name| Answerer::Session(name.to_owned()));
        self.gate(gate).ok().flatten().is_some_and(|gate| gates::closed_by_answer(&gate, &by, number, note))
    }

    fn answer_row_gate(&self, row: &FleetRow, gate: i64, number: usize, note: Option<&str>, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<()> {
        let asked = self.gate(gate)?.ok_or_else(|| NorthError::Usage(format!("no gate {gate}")))?;
        if asked.request.session != row.to {
            return Err(NorthError::Usage(format!("gate {gate} belongs to {}, not {}", asked.request.session, row.to)));
        }
        self.answer_gate_with(gate, number, note, row.sender.session(), deliver).map(drop)
    }

    pub(super) fn answer_session(&self, session: &str, text: &str) -> NorthResult<()> {
        let row = self.spool_answer(session, text)?;
        self.hand_in_answer(&row)
    }

    fn spool_answer(&self, session: &str, text: &str) -> NorthResult<FleetRow> {
        let row = FleetRow { urgent: true, ..self.new_row(RowKind::Message, session, text, &Sender::Tom)? };
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

    pub(super) fn new_row(&self, kind: RowKind, to: &str, text: &str, sender: &Sender) -> NorthResult<FleetRow> {
        let sent_at = now_ms();
        Ok(FleetRow {
            id: format!("{sent_at}-{}", new_uuid()?),
            kind,
            to: to.into(),
            from: self.host.clone(),
            urgent: false,
            sent_at,
            text: text.into(),
            sender: sender.clone(),
            gate: None,
            answer: None,
            job: None,
            origin: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live, live_under};
    use super::*;
    use crate::fleet::store::{SessionRow, cache_sessions};

    fn request(session: &str) -> GateRequest {
        GateRequest { verification: false, issue: None, box_text: None, session: session.into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 60_000 }
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

    #[test]
    fn a_spooled_gate_answer_for_a_session_held_on_another_host_is_applied_there_and_one_from_a_session_that_is_not_its_parent_is_refused_before_it_is_spooled() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let worker = live_under(&far, "worker", Some("boss"));
        let id = far.open_gate(&request("worker")).unwrap();
        let held = SessionRow { name: "worker".into(), host: "far".into(), status: "exited".into(), parent: Some("boss".into()), created: "t".into(), updated: "t".into(), ..SessionRow::default() };
        cache_sessions(&laptop.root, &[held]).unwrap();
        let label = format!("far:{id}");

        assert!(matches!(laptop.answer_remote_gate(&label, Some("worker"), 1, None, Some("intruder")), Err(NorthError::Rejected(_))));
        assert_eq!(Spool::at(&laptop.root).pending().unwrap().len(), 0);
        laptop.answer_remote_gate(&label, Some("worker"), 1, Some("go"), None).unwrap();
        let row = spooled(&laptop);

        assert_eq!(far.deliver_row(&row).unwrap(), HandedIn::Delivered);
        let closure = far.gate(id).unwrap().unwrap().closure.unwrap();
        assert_eq!((closure.by, closure.answer.as_str()), (Answerer::Tom, "yes\ngo"));
        assert!(worker.frames.lock().unwrap().iter().any(|frame| frame.contains("Tom answered: yes")));
    }
}
