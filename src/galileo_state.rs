use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use galileo_core::{Atom, Expr, Query, Slot, Type, Variable, View};
use galileo_runtime::{Program, Session};

use crate::error::{NorthError, NorthResult};
use crate::north_bindings::{Event, Menu, North};

pub use galileo_core::{Entity as StateIdentity, Value as StateValue};
pub type ProjectionFrame = StateValue;
pub type RelationTable = BTreeMap<StateIdentity, BTreeSet<StateValue>>;

pub trait RelationRows {
    fn rows(&self) -> &RelationTable;
}

impl RelationRows for RelationTable {
    fn rows(&self) -> &RelationTable { self }
}

pub trait ValueAccess {
    fn number(number: f64) -> NorthResult<Self> where Self: Sized;
    fn as_text(&self) -> Option<&str>;
    fn as_number(&self) -> Option<f64>;
    fn as_referent(&self) -> Option<&StateIdentity>;
}

impl ValueAccess for StateValue {
    fn number(number: f64) -> NorthResult<Self> {
        if number.is_finite() && number.fract() == 0.0 && number.abs() <= ((1_u64 << 53) - 1) as f64 {
            Ok(Self::Int(number as i64))
        } else {
            Err(NorthError::State("Observation is not an exact integer".into()))
        }
    }
    fn as_text(&self) -> Option<&str> { match self { Self::Text(value) => Some(value), _ => None } }
    fn as_number(&self) -> Option<f64> { match self { Self::Int(value) => Some(*value as f64), _ => None } }
    fn as_referent(&self) -> Option<&StateIdentity> { match self { Self::Entity(value) => Some(value), _ => None } }
}

struct NativeProgram {
    program: Program,
    relations: View,
}

fn program() -> NorthResult<&'static NativeProgram> {
    static PROGRAM: OnceLock<Result<NativeProgram, String>> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        let program = Program::load(include_bytes!("north_program.galileo")).map_err(|error| format!("{error:?}"))?;
        let mut fields = Vec::new();
        for (name, identity) in RELATIONS {
            let relation = program.relation_id(identity).ok_or_else(|| format!("Missing relation {identity}"))?;
            let query = Query::all([Variable::new("subject"), Variable::new("value")], [Atom::new(relation, [Expr::var("subject"), Expr::var("value")]).positive()]);
            let query = Query { head: Some(Box::new(Expr::List(vec![Expr::var("subject"), Expr::var("value")]))), ..query };
            fields.push((Slot::new(*name, Type::Any), Expr::from(query)));
        }
        let relations = View { name: "north-host-relations".into(), parameters: vec![], fields, source: None };
        Ok(NativeProgram { program, relations })
    }).as_ref().map_err(|error| NorthError::State(error.clone()))
}

pub struct Backend {
    session: Session<'static>,
}

impl Backend {
    pub fn open(checkpoint: Option<&[u8]>, clause: Option<&[u8]>) -> NorthResult<Self> {
        let program = &program()?.program;
        let session = if let Some(bytes) = checkpoint {
            Session::open(program, bytes).map_err(|error| NorthError::State(format!("{error:?}")))?
        } else if let Some(bytes) = clause {
            let workbench = clause_workbench::ResidentSourceWorkbenchV1::reopen(include_bytes!("../clause/north.clause"), bytes)?;
            let term = clause_package::canonical_term_bytes(&workbench.project_current_world()?).map_err(|error| NorthError::State(error.to_string()))?;
            let converted = galileo_import::clause_checkpoint::convert(program, &term, &["UsageRow"]).map_err(NorthError::State)?;
            Session::converted(program, converted.state)
        } else {
            Session::new(program)
        };
        Ok(Self { session })
    }

    pub fn revision(&self) -> u64 { self.session.revision() }

    pub fn run(&mut self, transitions: &[(&[u8], Vec<StateValue>)]) -> NorthResult<()> {
        let program = &program()?.program;
        let mut events = Vec::new();
        for (name, arguments) in transitions.iter() {
            let name = std::str::from_utf8(name).map_err(|error| NorthError::State(error.to_string()))?;
            let relation = program.relation_id(&format!("clause/north/{name}")).ok_or_else(|| NorthError::State(format!("Unknown event {name}")))?;
            let mut values = vec![North::named("north-main").into_value()];
            values.extend_from_slice(arguments);
            events.push(Event::from_tuple((relation, values), program).map_err(|error| NorthError::State(format!("{error:?}")))?);
        }
        self.events(&events)
    }

    pub fn events(&mut self, events: &[Event]) -> NorthResult<()> {
        let mut events = events.to_vec();
        let program = &program()?.program;
        for (name, _) in &MAINTENANCE {
            let name = std::str::from_utf8(name).map_err(|error| NorthError::State(error.to_string()))?;
            let relation = program.relation_id(&format!("clause/north/{name}")).ok_or_else(|| NorthError::State(format!("Unknown maintenance event {name}")))?;
            events.push(Event::from_tuple((relation, vec![North::named("north-main").into_value()]), program).map_err(|error| NorthError::State(format!("{error:?}")))?);
        }
        self.session.run_all(&events).map_err(|error| NorthError::State(format!("{error:?}")))?;
        Ok(())
    }

    pub fn menu(&self) -> NorthResult<Menu> {
        self.session.view::<Menu>((North::named("north-main"),)).map_err(|error| NorthError::State(format!("{error:?}")))
    }

    pub fn frame(&self) -> NorthResult<ProjectionFrame> {
        let StateValue::Record(relations) = self.session.project(&program()?.relations, &[]).map_err(|error| NorthError::State(format!("{error:?}")))? else {
            return Err(NorthError::State("Relations projection is not a record".into()));
        };
        let mut fields = BTreeMap::new();
        for (name, table) in &relations {
            let StateValue::List(rows) = table else { return Err(NorthError::State("Relation projection is not a list".into())); };
            for row in rows {
                let StateValue::List(row) = row else { return Err(NorthError::State("Relation projection is not a row".into())); };
                let [StateValue::Entity(subject), value] = row.as_slice() else { return Err(NorthError::State("Relation projection has invalid columns".into())); };
                let key = subject.name.rsplit('/').next().unwrap_or(&subject.name);
                let subject = fields.entry(key.to_owned()).or_insert_with(|| StateValue::Record(BTreeMap::new()));
                let StateValue::Record(subject) = subject else { unreachable!() };
                subject.insert(name.clone(), value.clone());
            }
        }
        fields.insert("relations".into(), StateValue::Record(relations));
        Ok(StateValue::Record(fields))
    }

    pub fn checkpoint_native_segments(&self) -> NorthResult<Vec<clause_package::AtomPayloadSegment>> {
        Ok(self.session.save().into_iter().map(|part| clause_package::AtomPayloadSegment::Bytes(part.into())).collect())
    }

}

const MAINTENANCE: [(&[u8], Vec<StateValue>); 6] = [
    (b"clear-resolved-answers", Vec::new()),
    (b"focus-prompts", Vec::new()),
    (b"present-prompt-questions", Vec::new()),
    (b"filter-transcript", Vec::new()),
    (b"present-usage", Vec::new()),
    (b"count-usage", Vec::new()),
];

const RELATIONS: &[(&str, &str)] = &[
    ("connection-state", "clause/north/North.connection-state"),
    ("known-usage-row", "clause/north/known-usage-row"),
    ("usage-row-key", "clause/north/UsageRow.key"),
    ("usage-row-owner", "clause/north/UsageRow.owner"),
    ("usage-row-label", "clause/north/UsageRow.label"),
    ("usage-row-value", "clause/north/UsageRow.value"),
    ("usage-row-known", "clause/north/UsageRow.known"),
    ("usage-row-unit", "clause/north/UsageRow.unit"),
    ("usage-row-order", "clause/north/UsageRow.order"),
    ("usage-row-visible", "clause/north/UsageRow.visible"),
    ("usage-next-row", "clause/north/North.usage-next-row"),
    ("usage-empty", "clause/north/North.usage-empty"),
    ("usage-unknown", "clause/north/North.usage-unknown"),
    ("usage-help", "clause/north/North.usage-help"),
    ("menu-kind", "clause/north/North.menu-kind"),
    ("menu-title", "clause/north/North.menu-title"),
    ("menu-query", "clause/north/North.menu-query"),
    ("menu-help", "clause/north/North.menu-help"),
    ("menu-empty", "clause/north/North.menu-empty"),
    ("menu-selection", "clause/north/North.menu-selection"),
    ("menu-count", "clause/north/North.menu-count"),
    ("menu-row-count", "clause/north/North.menu-row-count"),
    ("known-menu-row", "clause/north/known-menu-row"),
    ("menu-row-key", "clause/north/MenuRow.key"),
    ("menu-row-label", "clause/north/MenuRow.label"),
    ("menu-row-description", "clause/north/MenuRow.description"),
    ("menu-row-order", "clause/north/MenuRow.order"),
    ("menu-row-annotation", "clause/north/MenuRow.annotation"),
    ("menu-row-position", "clause/north/MenuRow.position"),
    ("menu-row-visible", "clause/north/MenuRow.visible"),
    ("known-reference", "clause/north/known-reference"),
    ("reference-name", "clause/north/Reference.name"),
    ("reference-description", "clause/north/Reference.description"),
    ("reference-kind", "clause/north/Reference.kind"),
    ("reference-path", "clause/north/Reference.path"),
    ("reference-number", "clause/north/Reference.number"),
    ("reference-query", "clause/north/North.reference-query"),
    ("reference-count", "clause/north/North.reference-count"),
    ("reference-selection", "clause/north/North.reference-selection"),
    ("references-open", "clause/north/North.references-open"),
    ("known-context", "clause/north/known-context"),
    ("conversation-id", "clause/north/Conversation.id"),
    ("saved-draft", "clause/north/Conversation.saved-draft"),
    ("transcript-query", "clause/north/Conversation.transcript-query"),
    ("transcript-offset", "clause/north/Conversation.transcript-offset"),
    ("transcript-limit", "clause/north/Conversation.transcript-limit"),
    ("transcript-changes", "clause/north/Conversation.transcript-changes"),
    ("chat-visible", "clause/north/ChatEntry.chat-visible"),
    ("context-model", "clause/north/Conversation.context-model"),
    ("context-effort", "clause/north/Conversation.context-effort"),
    ("context-attached", "clause/north/Conversation.context-attached"),
    ("known-prompt", "clause/north/known-prompt"),
    ("next-prompt-number", "clause/north/North.next-prompt-number"),
    ("prompt-number", "clause/north/Prompt.number"),
    ("prompt-request", "clause/north/Prompt.request"),
    ("prompt-conversation", "clause/north/Prompt.conversation"),
    ("prompt-turn", "clause/north/Prompt.turn"),
    ("prompt-kind", "clause/north/Prompt.kind"),
    ("prompt-status", "clause/north/Prompt.status"),
    ("prompt-visible", "clause/north/Prompt.visible"),
    ("prompt-live", "clause/north/Prompt.live"),
    ("prompt-current", "clause/north/Prompt.current"),
    ("prompt-question", "clause/north/prompt-question"),
    ("prompt-cancellation", "clause/north/Prompt.cancellation"),
    ("prompt-outcome", "clause/north/Prompt.outcome"),
    ("question-number", "clause/north/PromptQuestion.question-number"),
    ("question-key", "clause/north/PromptQuestion.question-key"),
    ("question-header", "clause/north/PromptQuestion.question-header"),
    ("question-text", "clause/north/PromptQuestion.question-text"),
    ("question-secret", "clause/north/PromptQuestion.question-secret"),
    ("question-other", "clause/north/PromptQuestion.question-other"),
    ("question-selected", "clause/north/PromptQuestion.question-selected"),
    ("question-answer", "clause/north/PromptQuestion.question-answer"),
    ("question-answered", "clause/north/PromptQuestion.question-answered"),
    ("question-option", "clause/north/question-option"),
    ("question-text-entry", "clause/north/PromptQuestion.question-text-entry"),
    ("question-codec", "clause/north/PromptQuestion.question-codec"),
    ("question-multiple", "clause/north/PromptQuestion.question-multiple"),
    ("question-optional", "clause/north/PromptQuestion.question-optional"),
    ("question-problem", "clause/north/PromptQuestion.question-problem"),
    ("option-checked", "clause/north/PromptOption.option-checked"),
    ("option-number", "clause/north/PromptOption.option-number"),
    ("option-label", "clause/north/PromptOption.option-label"),
    ("option-description", "clause/north/PromptOption.option-description"),
    ("option-value", "clause/north/PromptOption.option-value"),
    ("known-pending-input", "clause/north/known-pending-input"),
    ("pending-input-number", "clause/north/PendingInput.number"),
    ("pending-input-conversation", "clause/north/PendingInput.conversation"),
    ("pending-input-text", "clause/north/PendingInput.text"),
    ("pending-input-status", "clause/north/PendingInput.status"),
    ("pending-input-client-id", "clause/north/PendingInput.client-id"),
    ("pending-input-attachment", "clause/north/pending-input-attachment"),
    ("effect-input-number", "clause/north/North.effect-input-number"),
    ("active-turn", "clause/north/Conversation.active-turn"),
    ("phase", "clause/north/Conversation.phase"),
    ("known-chat-entry", "clause/north/known-chat-entry"),
    ("next-chat-order", "clause/north/North.next-chat-order"),
    ("chat-conversation", "clause/north/ChatEntry.chat-conversation"),
    ("chat-turn", "clause/north/ChatEntry.chat-turn"),
    ("chat-key", "clause/north/ChatEntry.chat-key"),
    ("chat-kind", "clause/north/ChatEntry.chat-kind"),
    ("chat-text", "clause/north/ChatEntry.chat-text"),
    ("chat-status", "clause/north/ChatEntry.chat-status"),
    ("chat-style", "clause/north/ChatEntry.chat-style"),
    ("chat-order", "clause/north/ChatEntry.chat-order"),
    ("active-delegated-child", "clause/north/Conversation.active-delegated-child"),
    ("terminal-delegated-child", "clause/north/Conversation.terminal-delegated-child"),
    ("turn-readiness", "clause/north/Conversation.turn-readiness"),
    ("conversation-change", "clause/north/North.conversation-change"),
    ("active-conversation", "clause/north/North.active-conversation"),
    ("pending-conversation", "clause/north/North.pending-conversation"),
    ("known-conversation", "clause/north/known-conversation"),
    ("draft-number", "clause/north/North.draft-number"),
    ("next-attachment-number", "clause/north/North.next-attachment-number"),
    ("draft-attachment", "clause/north/draft-attachment"),
    ("submitted-attachment", "clause/north/submitted-attachment"),
    ("known-goal", "clause/north/known-goal"),
    ("active-goal", "clause/north/North.active-goal"),
    ("goal-title", "clause/north/Goal.title"),
    ("goal-objective", "clause/north/Goal.objective"),
    ("goal-status", "clause/north/Goal.status"),
    ("goal-order", "clause/north/Goal.order"),
    ("known-goal-revision", "clause/north/known-goal-revision"),
    ("revision-objective", "clause/north/GoalRevision.revision-objective"),
    ("revision-order", "clause/north/GoalRevision.revision-order"),
    ("next-goal-revision", "clause/north/Goal.next-goal-revision"),
    ("next-goal-order", "clause/north/North.next-goal-order"),
    ("initialized", "clause/north/North.initialized"),
    ("known-command", "clause/north/known-command"),
    ("command-name", "clause/north/Command.name"),
    ("command-description", "clause/north/Command.description"),
    ("command-handler", "clause/north/Command.handler"),
    ("command-order", "clause/north/Command.order"),
    ("input-handler", "clause/north/North.input-handler"),
    ("host-effect", "clause/north/North.host-effect"),
    ("effect-payload", "clause/north/North.effect-payload"),
    ("notice", "clause/north/North.notice"),
    ("active-view", "clause/north/North.active-view"),
    ("known-view", "clause/north/known-view"),
    ("view-name", "clause/north/View.name"),
    ("view-label", "clause/north/View.label"),
    ("view-order", "clause/north/View.order"),
    ("next-view-handler", "clause/north/North.next-view-handler"),
    ("previous-view-handler", "clause/north/North.previous-view-handler"),
    ("pending-goal-title", "clause/north/North.pending-goal-title"),
    ("input-dispatch", "clause/north/North.input-dispatch"),
    ("input-payload", "clause/north/North.input-payload"),
    ("input-kind", "clause/north/North.input-kind"),
];

#[cfg(test)]
pub fn mismatched_save() -> Vec<u8> {
    let mut changed = (*program().unwrap().program).clone();
    let membership = changed.relation_id("clause/north/is-a-North").unwrap();
    changed.facts.push(galileo_core::Fact::new(membership, [StateValue::entity("clause/north/North", "changed-program")]));
    let changed = Program::from(changed);
    Session::new(&changed).save().concat()
}
