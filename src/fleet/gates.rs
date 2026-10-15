use crate::error::{NorthError, NorthResult};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateRequest {
    pub session: String,
    pub question: String,
    pub options: Vec<String>,
    pub recommend: usize,
    pub default: usize,
    pub default_after_ms: u64,
}

impl GateRequest {
    pub fn validate(&self) -> NorthResult<()> {
        let in_range = |choice: usize| (1..=self.options.len()).contains(&choice);
        if self.question.trim().is_empty() {
            return Err(NorthError::Usage("a gate needs a question".into()));
        }
        if self.options.len() < 2 || self.options.iter().any(|option| option.trim().is_empty()) {
            return Err(NorthError::Usage("a gate needs at least two non-empty options".into()));
        }
        if !in_range(self.recommend) || !in_range(self.default) {
            return Err(NorthError::Usage(format!("--recommend and --default must name an option from 1 to {}", self.options.len())));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Answerer {
    Tom,
    Default,
}

impl Answerer {
    pub fn label(self) -> &'static str {
        match self {
            Answerer::Tom => "tom",
            Answerer::Default => "default",
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "tom" => Some(Answerer::Tom),
            "default" => Some(Answerer::Default),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Closure {
    pub answer: String,
    pub by: Answerer,
    pub closed_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gate {
    pub id: i64,
    pub request: GateRequest,
    pub opened_ms: u64,
    pub closure: Option<Closure>,
}

impl Gate {
    pub fn default_at_ms(&self) -> u64 {
        self.opened_ms.saturating_add(self.request.default_after_ms)
    }

    pub fn is_open(&self) -> bool {
        self.closure.is_none()
    }

    pub fn option(&self, number: usize) -> Option<&str> {
        self.request.options.get(number.checked_sub(1)?).map(String::as_str)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Decision {
    Option(usize),
    Text(String),
    Default,
}

pub trait GateBook: Send + Sync {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64>;
    fn gate(&self, id: i64) -> NorthResult<Option<Gate>>;
    fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>>;
    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool>;
}

pub fn settle(book: &dyn GateBook, id: i64, decision: Decision, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    let closure = Closure { answer: answer(&gate, &decision)?, by: answerer(&decision), closed_ms: now_ms };
    if !book.close_gate(id, &closure)? {
        return Err(NorthError::Rejected(format!("gate {id} is already closed")));
    }
    deliver(&gate.request.session, &message(&gate, &closure))?;
    Ok(closure)
}

pub fn due(gate: &Gate, now_ms: u64) -> bool {
    gate.is_open() && now_ms >= gate.default_at_ms()
}

pub fn default_overdue(book: &dyn GateBook, now_ms: u64, deliver: impl Fn(&str, &str) -> NorthResult<()>) -> NorthResult<Vec<(i64, NorthResult<Closure>)>> {
    let overdue = book.gates(now_ms)?.into_iter().filter(|gate| due(gate, now_ms));
    let settled = overdue.map(|gate| (gate.id, settle(book, gate.id, Decision::Default, now_ms, &deliver)));
    Ok(settled.filter(|(_, outcome)| !matches!(outcome, Err(NorthError::Rejected(_)))).collect())
}

fn answer(gate: &Gate, decision: &Decision) -> NorthResult<String> {
    let chosen = |number: usize| gate.option(number).map(str::to_owned).ok_or_else(|| NorthError::Usage(format!("gate {} has no option {number}", gate.id)));
    match decision {
        Decision::Option(number) => chosen(*number),
        Decision::Default => chosen(gate.request.default),
        Decision::Text(text) if text.trim().is_empty() => Err(NorthError::Usage("an answer needs text".into())),
        Decision::Text(text) => Ok(text.trim().to_owned()),
    }
}

fn answerer(decision: &Decision) -> Answerer {
    match decision {
        Decision::Default => Answerer::Default,
        Decision::Option(_) | Decision::Text(_) => Answerer::Tom,
    }
}

fn message(gate: &Gate, closure: &Closure) -> String {
    let verdict = match closure.by {
        Answerer::Tom => format!("Tom answered: {}", closure.answer),
        Answerer::Default => format!("No answer in time, so the default applies: {}", closure.answer),
    };
    format!("[gate {}] {verdict}\nYour question: {}", gate.id, gate.request.question)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Book(Mutex<Vec<Gate>>);

    impl GateBook for Book {
        fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64> {
            let mut gates = self.0.lock().unwrap();
            let id = i64::try_from(gates.len()).unwrap() + 1;
            gates.push(Gate { id, request: request.clone(), opened_ms, closure: None });
            Ok(id)
        }

        fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
            Ok(self.0.lock().unwrap().iter().find(|gate| gate.id == id).cloned())
        }

        fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
            let gates = self.0.lock().unwrap();
            Ok(gates.iter().filter(|gate| gate.closure.as_ref().is_none_or(|closure| closure.closed_ms >= closed_since_ms)).cloned().collect())
        }

        fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool> {
            let mut gates = self.0.lock().unwrap();
            let gate = gates.iter_mut().find(|gate| gate.id == id && gate.closure.is_none());
            Ok(gate.map(|gate| gate.closure = Some(closure.clone())).is_some())
        }
    }

    fn request() -> GateRequest {
        GateRequest {
            session: "worker".into(),
            question: "Rent the GPU box?".into(),
            options: vec!["rent it".into(), "wait".into()],
            recommend: 1,
            default: 2,
            default_after_ms: 30_000,
        }
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
        assert!(matches!(again, Err(NorthError::Rejected(_))));
    }

    #[test]
    fn an_unanswered_gate_falls_due_and_closes_with_its_default() {
        let book = Book::default();
        let id = book.open_gate(&request(), 1_000).unwrap();
        let gate = book.gate(id).unwrap().unwrap();
        assert!(!due(&gate, 30_999));
        assert!(due(&gate, 31_000));
        let closure = settle(&book, id, Decision::Default, 31_000, |_, text| {
            assert!(text.contains("default applies: wait"), "{text}");
            Ok(())
        })
        .unwrap();
        assert_eq!(closure.by, Answerer::Default);
        assert!(!due(&book.gate(id).unwrap().unwrap(), 99_000));
    }

    #[test]
    fn free_text_answers_are_tom_and_out_of_range_choices_are_refused() {
        let book = Book::default();
        let id = book.open_gate(&request(), 0).unwrap();
        assert!(settle(&book, id, Decision::Option(3), 1, |_, _| Ok(())).is_err());
        assert!(book.gate(id).unwrap().unwrap().is_open());
        let closure = settle(&book, id, Decision::Text("  rent a smaller one ".into()), 2, |_, _| Ok(())).unwrap();
        assert_eq!((closure.answer.as_str(), closure.by), ("rent a smaller one", Answerer::Tom));
    }

    #[test]
    fn a_gate_request_needs_two_options_and_choices_among_them() {
        assert!(request().validate().is_ok());
        assert!(GateRequest { options: vec!["only".into()], recommend: 1, default: 1, ..request() }.validate().is_err());
        assert!(GateRequest { default: 3, ..request() }.validate().is_err());
        assert!(GateRequest { recommend: 0, ..request() }.validate().is_err());
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
    }

    #[test]
    fn an_overdue_gate_defaults_exactly_once_whoever_sweeps_first() {
        let book = Book::default();
        let overdue = book.open_gate(&request(), 1_000).unwrap();
        let pending = book.open_gate(&GateRequest { default_after_ms: 90_000, ..request() }, 1_000).unwrap();
        let snapshot = book.gates(0).unwrap();
        let delivered = Mutex::new(Vec::new());
        let deliver = |session: &str, _: &str| Ok(delivered.lock().unwrap().push(session.to_owned()));
        let swept = default_overdue(&book, 40_000, deliver).unwrap();
        assert_eq!(swept.iter().map(|(id, outcome)| (*id, outcome.as_ref().map(|closure| closure.by).ok())).collect::<Vec<_>>(), [(overdue, Some(Answerer::Default))]);
        assert!(book.gate(pending).unwrap().unwrap().is_open(), "a gate whose default is not due stays open");
        assert!(default_overdue(&book, 41_000, deliver).unwrap().is_empty());
        assert!(default_overdue(&Stale(&book, snapshot), 42_000, deliver).unwrap().is_empty(), "a sweeper with a stale view loses the close and stays quiet");
        assert_eq!(delivered.into_inner().unwrap(), ["worker"]);
    }
}
