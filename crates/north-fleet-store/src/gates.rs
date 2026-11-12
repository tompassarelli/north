use crate::error::{NorthError, NorthResult};
use crate::ask_once;
use north_base::lifecycle::State;

use crate::gate_lifecycle::GatePhase;
use crate::foundation::clock::Timestamp;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GateRequest {
    pub verification: bool,
    #[serde(default)]
    pub issue: Option<String>,
    /// The one box of `issue` this gate decides, matched against a launch's `Item:` and `Box:` lines; none holds the whole issue.
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

    /// Whether only Tom answers this gate: an ask-once gate carries no default, so it waits for him while every other gate applies its default at its deadline.
    pub fn for_tom(&self) -> bool {
        !self.has_default()
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
    Orphaned,
    Ticked,
}

impl Answerer {
    pub fn label(&self) -> &str {
        match self {
            Answerer::Session(name) => name,
            Answerer::Tom => "tom",
            Answerer::Default => "default",
            Answerer::Orphaned => "orphaned",
            Answerer::Ticked => "ticked",
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "tom" => Some(Answerer::Tom),
            "default" => Some(Answerer::Default),
            "orphaned" => Some(Answerer::Orphaned),
            "ticked" => Some(Answerer::Ticked),
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
    pub phase: GatePhase,
    pub closure: Option<Closure>,
}

impl Gate {
    /// A gate as a book without lifecycle records holds it: its phase follows from whether it has closed.
    pub fn new(id: i64, request: GateRequest, opened_ms: u64, closure: Option<Closure>) -> Self {
        let phase = closure.as_ref().map_or_else(|| GatePhase::opened(&request), |closure| GatePhase::settled(&closure.by));
        Self { id, request, opened_ms, phase, closure }
    }

    pub fn default_at_ms(&self) -> Option<u64> {
        self.request.has_default().then(|| self.opened_ms.saturating_add(self.request.default_after_ms))
    }

    pub fn is_open(&self) -> bool {
        self.phase.is_open()
    }

    pub fn for_tom(&self) -> bool {
        self.request.for_tom()
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
    /// Closes an open gate with `closure` and starts delivering its answer; false when another closer got there first.
    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool>;
    /// Moves a gate to `to` for `cause`; false when its phase or a live holder refuses the move.
    fn move_gate(&self, id: i64, to: GatePhase, cause: &str, at_ms: u64) -> NorthResult<bool>;
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
    pub typed: Option<&'a TypedTurn>,
}

/// The message Tom typed that opened a proxy's current turn: its conversation and when he sent it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedTurn {
    pub conversation: String,
    pub at_ms: u64,
}

impl Authority<'_> {
    pub fn admit(&self) -> NorthResult<()> {
        self.answerer(None, false).map(drop)
    }

    /// Who an answer to a gate opened at `opened_ms` is recorded as, with the note saying how it reached the gate:
    /// Tom from his own shell, Tom relayed by a proxy whose turn he opened by typing after the gate opened, else the
    /// gate session's parent, unless the gate is put to Tom (`for_tom`).
    fn answerer(&self, opened_ms: Option<u64>, for_tom: bool) -> NorthResult<(Answerer, Option<String>)> {
        let Some(caller) = self.caller else { return Ok((Answerer::Tom, None)) };
        if let (Some(typed), Some(opened_ms)) = (self.typed, opened_ms)
            && typed.at_ms > opened_ms
        {
            let at = Timestamp::from_unix_millis(i64::try_from(typed.at_ms).unwrap_or(i64::MAX)).iso();
            return Ok((Answerer::Tom, Some(format!("relayed by {caller} from Tom's message in conversation {} at {at}", typed.conversation))));
        }
        if self.parent == Some(caller) && for_tom {
            return Err(NorthError::Rejected(format!("the gate has no default, so it is put to Tom and only his own answer or a proxy relaying his typed turn closes it; {caller}, its asking session's parent, cannot answer it")));
        }
        if self.parent == Some(caller) {
            return Ok((Answerer::Session(caller.to_owned()), None));
        }
        Err(NorthError::Rejected("only Tom, the gate session's parent, or a proxy whose current turn Tom opened by typing after the gate opened may answer".into()))
    }
}

pub fn answer_gate(book: &dyn GateBook, id: i64, number: usize, note: Option<&str>, authority: Authority<'_>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    let (by, relayed) = authority.answerer(Some(gate.opened_ms), gate.for_tom())?;
    let note = [relayed.as_deref(), note].into_iter().flatten().collect::<Vec<_>>().join("\n");
    settle_as(book, id, Decision::Option(number), by, Some(&note), now_ms, deliver)
}

/// One of Tom's own typed turns, as the convo store holds it, offered as the evidence for an answer a session relays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TomTurn {
    pub id: i64,
    pub at_ms: u64,
    pub text: String,
}

/// Tom's typed `turn` as `relayer` passes it on, with the `parent` of the gate's asking session, who answers a gate with a default.
pub struct Relay<'a> {
    pub turn: &'a TomTurn,
    pub relayer: &'a str,
    pub parent: Option<&'a str>,
}

/// Closes a gate put to Tom, one with no default or whose asking session has no `parent` to answer it, with the answer a
/// session relays from his typed `turn`.
pub fn relay_gate(book: &dyn GateBook, id: i64, number: usize, relay: Relay<'_>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let Relay { turn, relayer, parent } = relay;
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    let refuse = |why: String| Err(NorthError::Refused(format!("convo turn {} cannot close gate {id}: {why}", turn.id)));
    if let Some(parent) = parent.filter(|_| !gate.for_tom()) {
        return refuse(format!("a gate with a default is put to its asking session's parent {parent}, who answers it directly"));
    }
    if turn.at_ms <= gate.opened_ms {
        return refuse("it was typed before the gate opened".into());
    }
    match named_option(&gate, &turn.text) {
        Some(named) if named == number => {}
        Some(named) => return refuse(format!("it names option {named}, not {number}")),
        None => return refuse("it names no single option of the gate".into()),
    }
    let note = format!("relayed by {relayer} from convo turn {}", turn.id);
    settle_as(book, id, Decision::Option(number), Answerer::Tom, Some(&note), now_ms, deliver)
}

pub fn orphan(book: &dyn GateBook, id: i64, now_ms: u64) -> NorthResult<Closure> {
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    let text = format!("orphaned: its asking session {} is gone and the issue it concerns is closed, so nobody can act on an answer", gate.request.session);
    settle_as(book, id, Decision::Text(text), Answerer::Orphaned, None, now_ms, |_, _| Ok(()))
}

fn named_option(gate: &Gate, text: &str) -> Option<usize> {
    let text = text.trim_start();
    let digits = text.len() - text.trim_start_matches(|character: char| character.is_ascii_digit()).len();
    if digits > 0 && text[digits..].chars().next().is_none_or(|next| !next.is_alphanumeric()) {
        return text[..digits].parse().ok().filter(|number| gate.option(*number).is_some());
    }
    let lower = text.to_lowercase();
    let mut named = gate.request.options.iter().enumerate().filter(|(_, option)| lower.contains(&option.trim().to_lowercase())).map(|(index, _)| index + 1);
    named.next().filter(|_| named.next().is_none())
}

fn settle_as(book: &dyn GateBook, id: i64, decision: Decision, by: Answerer, note: Option<&str>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    if !gate.is_open() {
        return Err(already_answered(&gate));
    }
    let answer = noted_answer(&gate, &decision, note)?;
    let closure = Closure { answer, by, closed_ms: now_ms };
    if !book.close_gate(id, &closure)? {
        let closed = book.gate(id)?.ok_or_else(|| NorthError::State(format!("gate {id} closed without an answer")))?;
        return Err(already_answered(&closed));
    }
    hand_over(book, &gate, &closure, now_ms, deliver)?;
    Ok(closure)
}

/// Delivers a closed gate's answer to its asking session and settles it; a failed delivery leaves it `Undelivered` for the next sweep.
fn hand_over(book: &dyn GateBook, gate: &Gate, closure: &Closure, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<()> {
    match deliver(&gate.request.session, &message(gate, closure)) {
        Ok(()) => book.move_gate(gate.id, GatePhase::settled(&closure.by), &format!("answer delivered to {}", gate.request.session), now_ms).map(drop),
        Err(error) => {
            book.move_gate(gate.id, GatePhase::Undelivered, &format!("delivery to {} failed: {}", gate.request.session, error.user_message()), now_ms)?;
            Err(error)
        }
    }
}

/// Delivers again the answer of each gate whose first delivery failed or whose closer died, as the sweep after every fleet command.
pub fn redeliver(book: &dyn GateBook, now_ms: u64, deliver: impl Fn(&str, &str) -> NorthResult<()>) -> NorthResult<Vec<(i64, NorthResult<Closure>)>> {
    let mut sent = Vec::new();
    for gate in book.gates(now_ms)?.into_iter().filter(|gate| gate.phase == GatePhase::Undelivered) {
        let Some(closure) = gate.closure.clone() else { continue };
        if book.move_gate(gate.id, GatePhase::Delivering, "delivering the answer again", now_ms)? {
            sent.push((gate.id, hand_over(book, &gate, &closure, now_ms, &deliver).map(|()| closure)));
        }
    }
    Ok(sent)
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

fn already_answered(gate: &Gate) -> NorthError {
    match &gate.closure {
        Some(closure) => NorthError::GateAnswered { id: gate.id, answer: closure.answer.clone(), by: closure.by.label().to_owned() },
        None => NorthError::GateAnswered { id: gate.id, answer: "(being recorded)".into(), by: gate.phase.name().into() },
    }
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
        self.issue == issue && self.box_text.as_deref().is_none_or(|text| item.is_none_or(|line| carries(line, text)))
    }
}

fn carries(line: &str, text: &str) -> bool {
    line.to_lowercase().contains(&text.trim().to_lowercase())
}

/// The ticked box line of the issue `body` that the box-scoped gate `request` decides, once no unticked box carries its text.
pub fn ticked_box<'a>(request: &GateRequest, body: &'a str) -> Option<&'a str> {
    let text = request.box_text.as_deref()?;
    let boxes = |marks: &'static [&'static str]| body.lines().map(str::trim_start).filter(move |line| marks.iter().any(|mark| line.starts_with(mark)) && carries(line, text));
    if boxes(&["- [ ]", "* [ ]"]).next().is_some() {
        return None;
    }
    boxes(&["- [x]", "- [X]", "* [x]", "* [X]"]).next()
}

/// Closes the open box-scoped gate `id` whose box `line` was ticked, with the tick as its answer.
pub fn tick(book: &dyn GateBook, id: i64, line: &str, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let text = format!("its box was ticked, so the work it decided is done: {}", line.trim());
    settle_as(book, id, Decision::Text(text), Answerer::Ticked, None, now_ms, deliver)
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
    let default = if request.has_default() { format!("default: {}", request.options.get(request.default.saturating_sub(1)).map_or("", String::as_str)) } else { "for Tom: only the work this gate holds waits for his answer; keep working on everything else".to_owned() };
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
        Answerer::Orphaned => format!("Closed unanswered: {}", closure.answer),
        Answerer::Ticked => format!("Closed by its box: {}", closure.answer),
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
        let undefaulted = |question: &str| GateRequest { question: question.into(), options: vec!["yes".into(), "no".into()], default: 0, default_after_ms: 0, ..request() };
        let refused = undefaulted("Rename the module?").validate().unwrap_err().user_message();
        assert!(refused.contains("--default must name an option"), "{refused}");
        for question in ["Pay for the GPU box?", "Create an account on the forum?", "Make the repo public?", "Vendor this code of unclear license?", "Send the email in Tom's name?", "Post the 0.3.1 announcement under your name?", "Delete the ledger?", "Give the host an ssh key, which widens access?", "Change the project direction?"] {
            let gate = undefaulted(question);
            assert!(gate.validate().is_ok() && gate.for_tom(), "{question} is ask-once, so it waits for Tom without a default");
            let defaulted = GateRequest { default: 1, default_after_ms: 60_000, ..gate };
            assert!(defaulted.validate().is_err(), "{question} must not carry a default");
        }
    }
}
