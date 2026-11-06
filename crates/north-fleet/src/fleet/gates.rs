use crate::error::{NorthError, NorthResult};
use crate::fleet::ask_once;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GateRequest {
    pub verification: bool,
    #[serde(default)]
    pub issue: Option<String>,
    /// The one box of `issue` this gate decides, matched against a launch's `Item:` line; none holds the whole issue.
    #[serde(default)]
    pub box_text: Option<String>,
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
        if self.box_text.as_deref().is_some_and(|text| text.trim().is_empty() || self.issue.is_none()) {
            return Err(NorthError::Usage("--box needs non-empty text and an --issue to scope".into()));
        }
        if self.question.trim().is_empty() {
            return Err(NorthError::Usage("a gate needs a question".into()));
        }
        if self.options.len() < 2 || self.options.iter().any(|option| option.trim().is_empty()) {
            return Err(NorthError::Usage("a gate needs at least two non-empty options".into()));
        }
        if !in_range(self.recommend) {
            return Err(NorthError::Usage(format!("--recommend must name an option from 1 to {}", self.options.len())));
        }
        let joined = std::iter::once(self.question.as_str()).chain(self.options.iter().map(String::as_str)).collect::<Vec<_>>().join("\n");
        if let Some(phrase) = north_base::platform::signin::names(&joined) && !ask_once::names("accounts", &self.question, &self.options) {
            return Err(NorthError::Usage(format!("this gate asks Tom to sign in (\"{phrase}\"), and no agent work waits on a human sign-in: build the machine path (a GitHub App token, a sops-held key or a setup token) as a box instead; only a new account, which nobody can create for him, may ask him to sign in")));
        }
        match ask_once::class(&self.question, &self.options) {
            Some(class) if self.has_default() => Err(NorthError::Usage(format!("this gate decides an ask-once matter ({class}) that only Tom answers: open it without --default and --default-after, and it waits for his answer"))),
            Some(_) => Ok(()),
            None if in_range(self.default) && self.default_after_ms > 0 => Ok(()),
            None => Err(NorthError::Usage(format!("--default must name an option from 1 to {} and --default-after must be a duration", self.options.len()))),
        }
    }

    pub fn has_default(&self) -> bool {
        self.default != 0 || self.default_after_ms != 0
    }
}

pub const ASKED_REPLY: &str = "North put this question to your parent session as a gate (to Tom when you have no parent). \
The answer arrives later as a message that starts with [gate N]. Keep working on everything the question does not block, \
and end your turn when nothing else is left.";
const ASK_DEFAULT_AFTER_MS: u64 = 30 * 60 * 1000;
const RECOMMENDED: &str = "(Recommended)";
const FILLERS: [&str; 2] = ["Use your best judgment", "Wait for a written answer"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ask {
    pub text: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Question {
    pub conversation: String,
    pub asks: Vec<Ask>,
    pub reply: String,
}

impl Ask {
    pub fn gate(&self, session: &str) -> GateRequest {
        let mut options: Vec<String> = self.options.iter().map(|option| option.trim()).filter(|option| !option.is_empty()).map(str::to_owned).collect();
        let recommend = options.iter().position(|option| option.contains(RECOMMENDED)).map_or(1, |index| index + 1);
        for filler in FILLERS {
            if options.len() < 2 {
                options.push(filler.to_owned());
            }
        }
        let (default, default_after_ms) = if ask_once::class(&self.text, &options).is_some() { (0, 0) } else { (recommend, ASK_DEFAULT_AFTER_MS) };
        GateRequest { verification: false, issue: None, box_text: None, session: session.to_owned(), question: self.text.clone(), options, recommend, default, default_after_ms }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Answerer {
    Session(String),
    Tom,
    Default,
}

impl Answerer {
    pub fn label(&self) -> &str {
        match self {
            Answerer::Session(name) => name,
            Answerer::Tom => "tom",
            Answerer::Default => "default",
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "tom" => Some(Answerer::Tom),
            "default" => Some(Answerer::Default),
            name if !name.is_empty() => Some(Answerer::Session(name.to_owned())),
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
    pub fn default_at_ms(&self) -> Option<u64> {
        self.request.has_default().then(|| self.opened_ms.saturating_add(self.request.default_after_ms))
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

pub fn open(book: &dyn GateBook, request: &GateRequest, parent: Option<&str>, host: Option<&str>, now_ms: u64, notify: impl FnOnce(&str, &str)) -> NorthResult<i64> {
    request.validate()?;
    let id = book.open_gate(request, now_ms)?;
    if let Some(parent) = parent {
        let label = host.map_or_else(|| id.to_string(), |host| format!("{host}:{id}"));
        notify(parent, &opened_notice(&label, request));
    }
    Ok(id)
}

pub fn settle(book: &dyn GateBook, id: i64, decision: Decision, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let by = answerer(&decision);
    settle_as(book, id, decision, by, None, now_ms, deliver)
}

pub struct Authority<'a> {
    pub caller: Option<&'a str>,
    pub parent: Option<&'a str>,
}

impl Authority<'_> {
    pub fn admit(&self) -> NorthResult<()> {
        if self.caller.is_some() && self.caller != self.parent {
            return Err(NorthError::Rejected("only Tom or the gate session's parent may answer".into()));
        }
        Ok(())
    }
}

pub fn answer_gate(book: &dyn GateBook, id: i64, number: usize, note: Option<&str>, authority: Authority<'_>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    authority.admit()?;
    let by = authority.caller.map_or(Answerer::Tom, |name| Answerer::Session(name.to_owned()));
    settle_as(book, id, Decision::Option(number), by, note, now_ms, deliver)
}

fn settle_as(book: &dyn GateBook, id: i64, decision: Decision, by: Answerer, note: Option<&str>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    if let Some(closure) = &gate.closure {
        return Err(already_answered(id, closure));
    }
    let answer = noted_answer(&gate, &decision, note)?;
    let closure = Closure { answer, by, closed_ms: now_ms };
    if !book.close_gate(id, &closure)? {
        let closed = book.gate(id)?.and_then(|gate| gate.closure).ok_or_else(|| NorthError::State(format!("gate {id} closed without an answer")))?;
        return Err(already_answered(id, &closed));
    }
    deliver(&gate.request.session, &message(&gate, &closure))?;
    Ok(closure)
}

fn noted_answer(gate: &Gate, decision: &Decision, note: Option<&str>) -> NorthResult<String> {
    let mut answer = answer(gate, decision)?;
    if let Some(note) = note.filter(|note| !note.trim().is_empty()) {
        answer.push_str(&format!("\n{}", note.trim()));
    }
    Ok(answer)
}

pub fn closed_by_answer(gate: &Gate, by: &Answerer, number: usize, note: Option<&str>) -> bool {
    gate.closure.as_ref().is_some_and(|closure| &closure.by == by && noted_answer(gate, &Decision::Option(number), note).is_ok_and(|answer| answer == closure.answer))
}

fn already_answered(id: i64, closure: &Closure) -> NorthError {
    NorthError::GateAnswered { id, answer: closure.answer.clone(), by: closure.by.label().to_owned() }
}

/// An open gate's hold on launches: the issue it decides, the one box of it when it names one, and its label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hold {
    pub issue: String,
    pub box_text: Option<String>,
    pub gate: String,
}

impl Hold {
    /// Whether a launch for `issue` whose `Item:` line is `item` waits on this gate: a whole-issue hold refuses every launch on the issue; a box hold only the launch whose line carries the box's text, or one naming no line to tell it apart.
    pub fn refuses(&self, issue: &str, item: Option<&str>) -> bool {
        self.issue == issue && self.box_text.as_deref().is_none_or(|text| item.is_none_or(|line| line.to_lowercase().contains(&text.to_lowercase())))
    }
}

pub fn decisions(gates: Vec<Gate>) -> Vec<Gate> {
    gates.into_iter().filter(|gate| !gate.request.verification).collect()
}

pub fn due(gate: &Gate, now_ms: u64) -> bool {
    gate.is_open() && gate.default_at_ms().is_some_and(|at| now_ms >= at)
}

pub fn default_overdue(book: &dyn GateBook, now_ms: u64, deliver: impl Fn(&str, &str) -> NorthResult<()>) -> NorthResult<Vec<(i64, NorthResult<Closure>)>> {
    let overdue = book.gates(now_ms)?.into_iter().filter(|gate| due(gate, now_ms));
    let settled = overdue.map(|gate| (gate.id, settle(book, gate.id, Decision::Default, now_ms, &deliver)));
    Ok(settled.filter(|(_, outcome)| !matches!(outcome, Err(NorthError::GateAnswered { .. }))).collect())
}

fn opened_notice(label: &str, request: &GateRequest) -> String {
    let default = if request.has_default() { format!("default: {}", request.options.get(request.default.saturating_sub(1)).map_or("", String::as_str)) } else { "no default: waits for Tom's answer".to_owned() };
    format!("[gate {label} from {}] {} ({default})", request.session, request.question.replace('\n', " "))
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
    let verdict = match &closure.by {
        Answerer::Session(name) => format!("{name} answered: {}", closure.answer),
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
            verification: false,
            issue: None,
            box_text: None,
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
    }

    #[test]
    fn an_overdue_gate_defaults_exactly_once_whoever_sweeps_first() {
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
    }
}
