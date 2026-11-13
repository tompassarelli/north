use std::sync::Mutex;

use crate::error::{NorthError, NorthResult};
use crate::gate_lifecycle::GatePhase;

use super::*;

#[derive(Default)]
struct Book(Mutex<Vec<Gate>>);

impl GateBook for Book {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64> {
        let mut gates = self.0.lock().unwrap();
        let id = i64::try_from(gates.len()).unwrap() + 1;
        gates.push(Gate::new(id, request.clone(), opened_ms, None));
        Ok(id)
    }

    fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        Ok(self.0.lock().unwrap().iter().find(|gate| gate.id == id).cloned())
    }

    fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        let gates = self.0.lock().unwrap();
        Ok(gates.iter().filter(|gate| !gate.phase.is_settled() || gate.closure.as_ref().is_some_and(|closure| closure.closed_ms >= closed_since_ms)).cloned().collect())
    }

    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool> {
        let mut gates = self.0.lock().unwrap();
        let gate = gates.iter_mut().find(|gate| gate.id == id && gate.is_open());
        Ok(gate
            .map(|gate| {
                gate.closure = Some(closure.clone());
                gate.phase = GatePhase::Delivering;
            })
            .is_some())
    }

    fn move_gate(&self, id: i64, to: GatePhase, _cause: &str, _at_ms: u64) -> NorthResult<bool> {
        let mut gates = self.0.lock().unwrap();
        Ok(gates.iter_mut().find(|gate| gate.id == id).map(|gate| gate.phase = to).is_some())
    }
}

fn request() -> GateRequest {
    GateRequest {
        verification: false,
        issue: None,
        box_text: None,
        session: "worker".into(),
        question: "Rent the GPU box?".into(),
        options: vec!["rent it".into(), "wait".into()],
        recommend: 1,
        default: 2,
        default_after_ms: 30_000,
        ask_once: None,
    }
}

#[test]
fn a_cost_question_recommending_a_bare_yes_takes_no_default_unless_it_names_ask_once() {
    let bare = |recommend: usize, ask_once: Option<&str>, default: usize| GateRequest { default, default_after_ms: if default > 0 { 60_000 } else { 0 }, ask_once: ask_once.map(str::to_owned), recommend, question: "Pay for the GPU box?".into(), options: vec!["yes".into(), "no".into()], ..request() };
    assert!(bare(1, None, 1).validate().unwrap_err().user_message().contains("--ask-once cost"), "a cost question recommending yes takes no default, however bare its options");
    assert!(bare(1, Some("cost"), 0).validate().is_ok(), "--ask-once with no default is accepted");
    assert!(bare(2, None, 2).validate().is_ok(), "a default that declines the cost commits to nothing");
}

#[test]
fn a_gate_without_a_default_names_the_ask_once_category_of_its_recommended_option() {
    let gate = |question: &str, options: [&str; 2], recommend: usize, ask_once: Option<&str>| GateRequest { question: question.into(), options: options.map(String::from).to_vec(), recommend, default: 0, default_after_ms: 0, ask_once: ask_once.map(str::to_owned), ..request() };
    let widening = ["keep the read-only token", "widen the token's access"];
    let refused = gate("Widen access for the worker?", widening, 1, None).validate().unwrap_err().user_message();
    assert!(refused.contains("--ask-once") && refused.contains("credentials") && refused.contains("--default-after"), "{refused}");
    assert!(gate("Widen access for the worker?", widening, 1, Some("widen")).validate().is_err(), "a word outside the ask-once table names no category");
    let for_tom = gate("Widen access for the worker?", widening, 2, Some("credentials"));
    assert!(for_tom.validate().is_ok() && for_tom.for_tom());
    let safe = GateRequest { default: 1, default_after_ms: 60_000, ..gate("Widen access for the worker?", widening, 1, None) };
    assert!(safe.validate().is_ok(), "the question names access, but the recommended option keeps the read-only token, so its default applies");
    let paid = GateRequest { default: 1, default_after_ms: 60_000, ..gate("Which box?", ["pay for the GPU box", "wait"], 1, None) };
    assert!(paid.validate().unwrap_err().user_message().contains("--ask-once cost"), "a recommended option that spends money waits for Tom");
    let both = GateRequest { default: 1, default_after_ms: 60_000, ..gate("Widen access for the worker?", widening, 2, Some("credentials")) };
    assert!(both.validate().is_err(), "a category and a default contradict each other");
}

#[test]
fn a_gate_closes_once_and_delivers_its_answer_to_the_asking_session() {
    let book = Book::default();
    let id = book.open_gate(&request(), 1_000).unwrap();
    let mut delivered = Vec::new();
    let closure = settle(&book, id, Decision::Option(1), 5_000, |session, text| {
        delivered.push((session.to_owned(), text.to_owned()));
        Ok(())
    })
    .unwrap();
    assert_eq!(closure, Closure { answer: "rent it".into(), by: Answerer::Tom, closed_ms: 5_000 });
    assert_eq!(delivered, [("worker".to_owned(), "[gate 1] Tom answered: rent it\nYour question: Rent the GPU box?".to_owned())]);
    let again = settle(&book, id, Decision::Default, 40_000, |_, _| panic!("a closed gate must not deliver twice"));
    assert!(matches!(again, Err(NorthError::GateAnswered { .. })));
}

struct Stale<'a>(&'a Book, Vec<Gate>);

impl GateBook for Stale<'_> {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64> {
        self.0.open_gate(request, opened_ms)
    }

    fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        self.0.gate(id)
    }

    fn gates(&self, _closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        Ok(self.1.clone())
    }

    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool> {
        self.0.close_gate(id, closure)
    }

    fn move_gate(&self, id: i64, to: GatePhase, cause: &str, at_ms: u64) -> NorthResult<bool> {
        self.0.move_gate(id, to, cause, at_ms)
    }
}

#[test]
fn an_overdue_gate_defaults_exactly_once_and_only_an_ask_once_gate_opens_without_a_default() {
    let book = Book::default();
    let overdue = book.open_gate(&request(), 1_000).unwrap();
    let pending = book.open_gate(&GateRequest { default_after_ms: 90_000, ..request() }, 1_000).unwrap();
    let snapshot = book.gates(0).unwrap();
    let delivered = Mutex::new(Vec::new());
    let deliver = |session: &str, _: &str| {
        let _: () = delivered.lock().unwrap().push(session.to_owned());
        Ok(())
    };
    let swept = default_overdue(&book, 40_000, deliver).unwrap();
    assert_eq!(swept.iter().map(|(id, outcome)| (*id, outcome.as_ref().map(|closure| closure.by.clone()).ok())).collect::<Vec<_>>(), [(overdue, Some(Answerer::Default))]);
    assert!(book.gate(pending).unwrap().unwrap().is_open(), "a gate whose default is not due stays open");
    assert!(default_overdue(&book, 41_000, deliver).unwrap().is_empty());
    assert!(default_overdue(&Stale(&book, snapshot), 42_000, deliver).unwrap().is_empty(), "a sweeper with a stale view loses the close and stays quiet");
    assert_eq!(delivered.into_inner().unwrap(), ["worker"]);
    let ask = Ask { text: "May we spend money on the GPU box?".into(), options: vec!["yes".into(), "no".into()] };
    let gate = ask.gate("worker");
    assert_eq!((gate.has_default(), gate.validate().is_ok()), (false, true));
    let book = Book::default();
    book.open_gate(&gate, 1_000).unwrap();
    assert!(default_overdue(&book, u64::MAX, |_, _| Ok(())).unwrap().is_empty());
    let routine = Ask { text: "Rename the module?".into(), options: vec!["yes".into(), "no".into()] }.gate("worker");
    assert!(routine.has_default());
    let undefaulted = |question: &str, category: Option<&str>| GateRequest { question: question.into(), options: vec!["yes".into(), "no".into()], default: 0, default_after_ms: 0, ask_once: category.map(str::to_owned), ..request() };
    let refused = undefaulted("Rename the module?", None).validate().unwrap_err().user_message();
    assert!(refused.contains("--ask-once"), "{refused}");
    for (question, category) in [("Pay for the GPU box?", "cost"), ("Create an account on the forum?", "accounts"), ("Make the repo public?", "public"), ("Vendor this code of unclear license?", "direction"), ("Send the email in Tom's name?", "tom_name"), ("Post the 0.3.1 announcement under your name?", "tom_name"), ("Delete the ledger?", "deletion"), ("Give the host an ssh key, which widens access?", "credentials"), ("Change the project direction?", "direction")] {
        let gate = undefaulted(question, Some(category));
        assert!(gate.validate().is_ok() && gate.for_tom(), "{question} is ask-once, so it waits for Tom without a default");
        let defaulted = GateRequest { default: 1, default_after_ms: 60_000, ..gate };
        assert!(defaulted.validate().is_err(), "{question} must not carry a default");
    }
}
