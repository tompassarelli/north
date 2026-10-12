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
    pub fn open(checkpoint: Option<&[u8]>) -> NorthResult<Self> {
        let program = &program()?.program;
        let session = if let Some(bytes) = checkpoint {
            Session::open(program, bytes).map_err(|error| NorthError::State(format!("{error:?}")))?
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
            let relation = program.relation_id(&format!("galileo/north/{name}")).ok_or_else(|| NorthError::State(format!("Unknown event {name}")))?;
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
            let relation = program.relation_id(&format!("galileo/north/{name}")).ok_or_else(|| NorthError::State(format!("Unknown maintenance event {name}")))?;
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

    pub fn checkpoint_native_segments(&self) -> NorthResult<Vec<std::sync::Arc<[u8]>>> {
        Ok(self.session.save().into_iter().map(|part| part.into()).collect())
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
    ("connection-state", "galileo/north/North.connection-state"),
    ("known-usage-row", "galileo/north/known-usage-row"),
    ("usage-row-key", "galileo/north/UsageRow.key"),
    ("usage-row-owner", "galileo/north/UsageRow.owner"),
    ("usage-row-label", "galileo/north/UsageRow.label"),
    ("usage-row-value", "galileo/north/UsageRow.value"),
    ("usage-row-known", "galileo/north/UsageRow.known"),
    ("usage-row-unit", "galileo/north/UsageRow.unit"),
    ("usage-row-order", "galileo/north/UsageRow.order"),
    ("usage-row-visible", "galileo/north/UsageRow.visible"),
    ("usage-next-row", "galileo/north/North.usage-next-row"),
    ("usage-empty", "galileo/north/North.usage-empty"),
    ("usage-unknown", "galileo/north/North.usage-unknown"),
    ("usage-help", "galileo/north/North.usage-help"),
    ("menu-kind", "galileo/north/North.menu-kind"),
    ("menu-title", "galileo/north/North.menu-title"),
    ("menu-query", "galileo/north/North.menu-query"),
    ("menu-help", "galileo/north/North.menu-help"),
    ("menu-empty", "galileo/north/North.menu-empty"),
    ("menu-selection", "galileo/north/North.menu-selection"),
    ("menu-count", "galileo/north/North.menu-count"),
    ("menu-row-count", "galileo/north/North.menu-row-count"),
    ("known-menu-row", "galileo/north/known-menu-row"),
    ("menu-row-key", "galileo/north/MenuRow.key"),
    ("menu-row-label", "galileo/north/MenuRow.label"),
    ("menu-row-description", "galileo/north/MenuRow.description"),
    ("menu-row-order", "galileo/north/MenuRow.order"),
    ("menu-row-annotation", "galileo/north/MenuRow.annotation"),
    ("menu-row-position", "galileo/north/MenuRow.position"),
    ("menu-row-visible", "galileo/north/MenuRow.visible"),
    ("known-reference", "galileo/north/known-reference"),
    ("reference-name", "galileo/north/Reference.name"),
    ("reference-description", "galileo/north/Reference.description"),
    ("reference-kind", "galileo/north/Reference.kind"),
    ("reference-path", "galileo/north/Reference.path"),
    ("reference-number", "galileo/north/Reference.number"),
    ("reference-query", "galileo/north/North.reference-query"),
    ("reference-count", "galileo/north/North.reference-count"),
    ("reference-selection", "galileo/north/North.reference-selection"),
    ("references-open", "galileo/north/North.references-open"),
    ("known-context", "galileo/north/known-context"),
    ("conversation-id", "galileo/north/Conversation.id"),
    ("saved-draft", "galileo/north/Conversation.saved-draft"),
    ("transcript-query", "galileo/north/Conversation.transcript-query"),
    ("transcript-offset", "galileo/north/Conversation.transcript-offset"),
    ("transcript-limit", "galileo/north/Conversation.transcript-limit"),
    ("transcript-changes", "galileo/north/Conversation.transcript-changes"),
    ("chat-visible", "galileo/north/ChatEntry.chat-visible"),
    ("context-model", "galileo/north/Conversation.context-model"),
    ("context-effort", "galileo/north/Conversation.context-effort"),
    ("context-attached", "galileo/north/Conversation.context-attached"),
    ("known-prompt", "galileo/north/known-prompt"),
    ("next-prompt-number", "galileo/north/North.next-prompt-number"),
    ("prompt-number", "galileo/north/Prompt.number"),
    ("prompt-request", "galileo/north/Prompt.request"),
    ("prompt-conversation", "galileo/north/Prompt.conversation"),
    ("prompt-turn", "galileo/north/Prompt.turn"),
    ("prompt-kind", "galileo/north/Prompt.kind"),
    ("prompt-status", "galileo/north/Prompt.status"),
    ("prompt-visible", "galileo/north/Prompt.visible"),
    ("prompt-live", "galileo/north/Prompt.live"),
    ("prompt-current", "galileo/north/Prompt.current"),
    ("prompt-question", "galileo/north/prompt-question"),
    ("prompt-cancellation", "galileo/north/Prompt.cancellation"),
    ("prompt-outcome", "galileo/north/Prompt.outcome"),
    ("question-number", "galileo/north/PromptQuestion.question-number"),
    ("question-key", "galileo/north/PromptQuestion.question-key"),
    ("question-header", "galileo/north/PromptQuestion.question-header"),
    ("question-text", "galileo/north/PromptQuestion.question-text"),
    ("question-secret", "galileo/north/PromptQuestion.question-secret"),
    ("question-other", "galileo/north/PromptQuestion.question-other"),
    ("question-selected", "galileo/north/PromptQuestion.question-selected"),
    ("question-answer", "galileo/north/PromptQuestion.question-answer"),
    ("question-answered", "galileo/north/PromptQuestion.question-answered"),
    ("question-option", "galileo/north/question-option"),
    ("question-text-entry", "galileo/north/PromptQuestion.question-text-entry"),
    ("question-codec", "galileo/north/PromptQuestion.question-codec"),
    ("question-multiple", "galileo/north/PromptQuestion.question-multiple"),
    ("question-optional", "galileo/north/PromptQuestion.question-optional"),
    ("question-problem", "galileo/north/PromptQuestion.question-problem"),
    ("option-checked", "galileo/north/PromptOption.option-checked"),
    ("option-number", "galileo/north/PromptOption.option-number"),
    ("option-label", "galileo/north/PromptOption.option-label"),
    ("option-description", "galileo/north/PromptOption.option-description"),
    ("option-value", "galileo/north/PromptOption.option-value"),
    ("known-pending-input", "galileo/north/known-pending-input"),
    ("pending-input-number", "galileo/north/PendingInput.number"),
    ("pending-input-conversation", "galileo/north/PendingInput.conversation"),
    ("pending-input-text", "galileo/north/PendingInput.text"),
    ("pending-input-status", "galileo/north/PendingInput.status"),
    ("pending-input-client-id", "galileo/north/PendingInput.client-id"),
    ("pending-input-attachment", "galileo/north/pending-input-attachment"),
    ("effect-input-number", "galileo/north/North.effect-input-number"),
    ("active-turn", "galileo/north/Conversation.active-turn"),
    ("phase", "galileo/north/Conversation.phase"),
    ("known-chat-entry", "galileo/north/known-chat-entry"),
    ("next-chat-order", "galileo/north/North.next-chat-order"),
    ("chat-conversation", "galileo/north/ChatEntry.chat-conversation"),
    ("chat-turn", "galileo/north/ChatEntry.chat-turn"),
    ("chat-key", "galileo/north/ChatEntry.chat-key"),
    ("chat-kind", "galileo/north/ChatEntry.chat-kind"),
    ("chat-text", "galileo/north/ChatEntry.chat-text"),
    ("chat-status", "galileo/north/ChatEntry.chat-status"),
    ("chat-style", "galileo/north/ChatEntry.chat-style"),
    ("chat-order", "galileo/north/ChatEntry.chat-order"),
    ("active-delegated-child", "galileo/north/Conversation.active-delegated-child"),
    ("terminal-delegated-child", "galileo/north/Conversation.terminal-delegated-child"),
    ("turn-readiness", "galileo/north/Conversation.turn-readiness"),
    ("conversation-change", "galileo/north/North.conversation-change"),
    ("active-conversation", "galileo/north/North.active-conversation"),
    ("pending-conversation", "galileo/north/North.pending-conversation"),
    ("known-conversation", "galileo/north/known-conversation"),
    ("draft-number", "galileo/north/North.draft-number"),
    ("next-attachment-number", "galileo/north/North.next-attachment-number"),
    ("draft-attachment", "galileo/north/draft-attachment"),
    ("submitted-attachment", "galileo/north/submitted-attachment"),
    ("known-goal", "galileo/north/known-goal"),
    ("active-goal", "galileo/north/North.active-goal"),
    ("goal-title", "galileo/north/Goal.title"),
    ("goal-objective", "galileo/north/Goal.objective"),
    ("goal-status", "galileo/north/Goal.status"),
    ("goal-order", "galileo/north/Goal.order"),
    ("known-goal-revision", "galileo/north/known-goal-revision"),
    ("revision-objective", "galileo/north/GoalRevision.revision-objective"),
    ("revision-order", "galileo/north/GoalRevision.revision-order"),
    ("next-goal-revision", "galileo/north/Goal.next-goal-revision"),
    ("next-goal-order", "galileo/north/North.next-goal-order"),
    ("initialized", "galileo/north/North.initialized"),
    ("known-command", "galileo/north/known-command"),
    ("command-name", "galileo/north/Command.name"),
    ("command-description", "galileo/north/Command.description"),
    ("command-handler", "galileo/north/Command.handler"),
    ("command-order", "galileo/north/Command.order"),
    ("input-handler", "galileo/north/North.input-handler"),
    ("host-effect", "galileo/north/North.host-effect"),
    ("effect-payload", "galileo/north/North.effect-payload"),
    ("notice", "galileo/north/North.notice"),
    ("active-view", "galileo/north/North.active-view"),
    ("known-view", "galileo/north/known-view"),
    ("view-name", "galileo/north/View.name"),
    ("view-label", "galileo/north/View.label"),
    ("view-order", "galileo/north/View.order"),
    ("next-view-handler", "galileo/north/North.next-view-handler"),
    ("previous-view-handler", "galileo/north/North.previous-view-handler"),
    ("pending-goal-title", "galileo/north/North.pending-goal-title"),
    ("input-dispatch", "galileo/north/North.input-dispatch"),
    ("input-payload", "galileo/north/North.input-payload"),
    ("input-kind", "galileo/north/North.input-kind"),
];

#[cfg(test)]
pub fn mismatched_save() -> Vec<u8> {
    let mut changed = (*program().unwrap().program).clone();
    let membership = changed.relation_id("galileo/north/is-a-North").unwrap();
    changed.facts.push(galileo_core::Fact::new(membership, [StateValue::entity("galileo/north/North", "changed-program")]));
    let changed = Program::from(changed);
    Session::new(&changed).save().concat()
}
