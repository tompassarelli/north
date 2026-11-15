use north_base::lifecycle::State;

use north_foundation::error::{NorthError, NorthResult};
use north_foundation::foundation::clock::Timestamp;
use crate::gate_lifecycle::GatePhase;

use super::record::{Answerer, Closure, Decision, Gate, GateBook};
use super::request::{GateRequest, opened_notice};

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

/// The message Tom typed that opened a coordinator's current turn: its conversation and when he sent it.
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
    /// Tom from his own shell, Tom relayed by a coordinator whose turn he opened by typing after the gate opened, else the
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
            return Err(NorthError::Rejected(format!("the gate has no default, so it is put to Tom and only his own answer or a coordinator relaying his typed turn closes it; {caller}, its asking session's parent, cannot answer it")));
        }
        if self.parent == Some(caller) {
            return Ok((Answerer::Session(caller.to_owned()), None));
        }
        Err(NorthError::Rejected("only Tom, the gate session's parent, or a coordinator whose current turn Tom opened by typing after the gate opened may answer".into()))
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

/// Who asks to withdraw a gate: the caller's session name (none for Tom from his own shell) and whether that session is the coordinator.
pub struct Withdrawer<'a> {
    pub caller: Option<&'a str>,
    pub coordinator: bool,
}

/// Closes a gate opened in error with the withdrawer's name and the `reason` as its recorded answer: Tom, the coordinator or the gate's asking session may; no option is chosen, so nothing is approved.
pub fn withdraw(book: &dyn GateBook, id: i64, reason: &str, by: Withdrawer<'_>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let gate = book.gate(id)?.ok_or_else(|| NorthError::Usage(format!("no gate {id}")))?;
    if let Some(caller) = by.caller
        && caller != gate.request.session
        && !by.coordinator
    {
        return Err(NorthError::Rejected(format!("only Tom, the coordinator or the gate's asking session {} may withdraw gate {id}; {caller} is none of them", gate.request.session)));
    }
    let who = by.caller.unwrap_or("tom");
    settle_as(book, id, Decision::Text(format!("{who}: {}", reason.trim())), Answerer::Withdrawn, None, now_ms, deliver)
}

fn named_option(gate: &Gate, text: &str) -> Option<usize> {
    let options: Vec<String> = gate.request.options.iter().map(|option| option.trim().to_lowercase()).collect();
    let own = text.to_lowercase().replace(&gate.request.question.trim().to_lowercase(), "");
    let spelled = options.iter().enumerate().filter(|(_, option)| own.contains(option.as_str())).map(|(index, _)| index + 1);
    let answer = options.iter().fold(own.clone(), |rest, option| rest.replace(option.as_str(), ""));
    let numbered = answer.lines().filter(|line| !line.trim_start().starts_with('>')).flat_map(answer_numbers).filter(|number| gate.option(*number).is_some());
    let mut named: Vec<usize> = spelled.chain(numbered).collect();
    named.sort_unstable();
    named.dedup();
    match named[..] {
        [only] => Some(only),
        _ => None,
    }
}

fn answer_numbers(line: &str) -> Vec<usize> {
    if asks_or_denies(line) {
        return Vec::new();
    }
    let words: Vec<&str> = line.split_whitespace().collect();
    (0..words.len()).filter(|index| answers_at(&words, *index)).filter_map(|index| number(words[index])).collect()
}

fn asks_or_denies(line: &str) -> bool {
    let lower = line.to_lowercase();
    lower.contains('?') || lower.split(|character: char| !character.is_alphanumeric() && character != '\'').any(|word| word.ends_with("n't") || matches!(word, "not" | "no" | "never" | "except" | "instead" | "without" | "rather" | "why"))
}

fn number(word: &str) -> Option<usize> {
    let digits = word.trim_end_matches(['.', ')', ':', ',', '!']);
    if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) { None } else { digits.parse().ok() }
}

fn answers_at(words: &[&str], index: usize) -> bool {
    let previous = index.checked_sub(1).map(|before| words[before]);
    if previous == Some("gate") {
        return false;
    }
    let cue = |word: &str| matches!(word, "option" | "choice" | "choose" | "pick" | "take" | "say");
    let closing = index + 1 == words.len() && previous.is_none_or(|word| word.ends_with(['.', '!', '>', '→']) || cue(word));
    index == 0 || closing || previous.is_some_and(cue) || (index >= 2 && words[index - 2] == "go" && words[index - 1] == "with")
}

pub(super) fn settle_as(book: &dyn GateBook, id: i64, decision: Decision, by: Answerer, note: Option<&str>, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
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
        Answerer::Withdrawn => format!("Withdrawn, nothing approved, by {}", closure.answer),
    };
    format!("[gate {}] {verdict}\nYour question: {}", gate.id, gate.request.question)
}
