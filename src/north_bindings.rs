use galileo_core::{Entity, EvalError, Value};
use galileo_runtime::{Program, State};

fn invalid(at: &str) -> EvalError { EvalError::Type { at: at.into(), message: "value does not match generated binding".into() } }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatEntry(pub Entity);
impl ChatEntry {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/ChatEntry", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/ChatEntry" => Ok(Self(entity.clone())), _ => Err(invalid("ChatEntry")) } }
    pub fn chat_conversation(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/ChatEntry.chat-conversation") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn chat_key(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/ChatEntry.chat-key") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn chat_order(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/ChatEntry.chat-order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command(pub Entity);
impl Command {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/Command", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/Command" => Ok(Self(entity.clone())), _ => Err(invalid("Command")) } }
    pub fn name(&self, state: &State, program: &Program) -> Result<Option<String>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Command.name") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
    pub fn description(&self, state: &State, program: &Program) -> Result<Option<String>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Command.description") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
    pub fn handler(&self, state: &State, program: &Program) -> Result<Option<String>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Command.handler") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
    pub fn order(&self, state: &State, program: &Program) -> Result<Option<i64>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Command.order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conversation(pub Entity);
impl Conversation {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/Conversation", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/Conversation" => Ok(Self(entity.clone())), _ => Err(invalid("Conversation")) } }
    pub fn id(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Conversation.id") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Goal(pub Entity);
impl Goal {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/Goal", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/Goal" => Ok(Self(entity.clone())), _ => Err(invalid("Goal")) } }
    pub fn title(&self, state: &State, program: &Program) -> Result<Option<String>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Goal.title") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
    pub fn order(&self, state: &State, program: &Program) -> Result<Option<i64>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Goal.order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalRevision(pub Entity);
impl GoalRevision {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/GoalRevision", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/GoalRevision" => Ok(Self(entity.clone())), _ => Err(invalid("GoalRevision")) } }
    pub fn revision_objective(&self, state: &State, program: &Program) -> Result<Option<String>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/GoalRevision.revision-objective") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
    pub fn revision_order(&self, state: &State, program: &Program) -> Result<Option<i64>, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/GoalRevision.revision-order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Null => Ok(None), item => (match item { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }).map(Some) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuRow(pub Entity);
impl MenuRow {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/MenuRow", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/MenuRow" => Ok(Self(entity.clone())), _ => Err(invalid("MenuRow")) } }
    pub fn key(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/MenuRow.key") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn label(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/MenuRow.label") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn description(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/MenuRow.description") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn order(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/MenuRow.order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn annotation(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/MenuRow.annotation") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct North(pub Entity);
impl North {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/North", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/North" => Ok(Self(entity.clone())), _ => Err(invalid("North")) } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingInput(pub Entity);
impl PendingInput {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/PendingInput", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/PendingInput" => Ok(Self(entity.clone())), _ => Err(invalid("PendingInput")) } }
    pub fn number(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PendingInput.number") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn conversation(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PendingInput.conversation") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn text(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PendingInput.text") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt(pub Entity);
impl Prompt {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/Prompt", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/Prompt" => Ok(Self(entity.clone())), _ => Err(invalid("Prompt")) } }
    pub fn number(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Prompt.number") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn request(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Prompt.request") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn conversation(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Prompt.conversation") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn turn(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Prompt.turn") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn kind(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Prompt.kind") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn cancellation(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Prompt.cancellation") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptOption(pub Entity);
impl PromptOption {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/PromptOption", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/PromptOption" => Ok(Self(entity.clone())), _ => Err(invalid("PromptOption")) } }
    pub fn option_number(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptOption.option-number") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn option_label(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptOption.option-label") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn option_description(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptOption.option-description") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn option_value(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptOption.option-value") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptQuestion(pub Entity);
impl PromptQuestion {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/PromptQuestion", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/PromptQuestion" => Ok(Self(entity.clone())), _ => Err(invalid("PromptQuestion")) } }
    pub fn question_number(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-number") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_key(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-key") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_header(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-header") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_text(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-text") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_secret(&self, state: &State, program: &Program) -> Result<bool, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-secret") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_other(&self, state: &State, program: &Program) -> Result<bool, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-other") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_codec(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-codec") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_multiple(&self, state: &State, program: &Program) -> Result<bool, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-multiple") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn question_optional(&self, state: &State, program: &Program) -> Result<bool, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/PromptQuestion.question-optional") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference(pub Entity);
impl Reference {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/Reference", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/Reference" => Ok(Self(entity.clone())), _ => Err(invalid("Reference")) } }
    pub fn name(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Reference.name") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn description(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Reference.description") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn kind(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Reference.kind") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn path(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Reference.path") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn number(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/Reference.number") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageRow(pub Entity);
impl UsageRow {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/UsageRow", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/UsageRow" => Ok(Self(entity.clone())), _ => Err(invalid("UsageRow")) } }
    pub fn key(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.key") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn owner(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.owner") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn label(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.label") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn value(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.value") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn known(&self, state: &State, program: &Program) -> Result<bool, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.known") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn unit(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.unit") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
    pub fn order(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("clause/north/UsageRow.order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).unwrap_or(&Value::Null);
        match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View(pub Entity);
impl View {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("clause/north/View", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: &Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "clause/north/View" => Ok(Self(entity.clone())), _ => Err(invalid("View")) } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionLost {
    pub north: North,
}
impl galileo_runtime::Event for ConnectionLost {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ConnectionLost { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseDisconnectedInputs {
    pub north: North,
}
impl galileo_runtime::Event for PauseDisconnectedInputs {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PauseDisconnectedInputs { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisconnectPrompts {
    pub north: North,
}
impl galileo_runtime::Event for DisconnectPrompts {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::DisconnectPrompts { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginReconnect {
    pub north: North,
}
impl galileo_runtime::Event for BeginReconnect {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::BeginReconnect { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishReconnect {
    pub north: North,
    pub success: bool,
}
impl galileo_runtime::Event for FinishReconnect {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FinishReconnect { north: self.north.clone(), success: self.success.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandReconnect {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandReconnect {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandReconnect { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeginConversationReconciliation {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for BeginConversationReconciliation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::BeginConversationReconciliation { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountUsage {
    pub north: North,
}
impl galileo_runtime::Event for CountUsage {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CountUsage { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentUsage {
    pub north: North,
}
impl galileo_runtime::Event for PresentUsage {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PresentUsage { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawUsageRow {
    pub north: North,
    pub owner: String,
    pub key: String,
}
impl galileo_runtime::Event for WithdrawUsageRow {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::WithdrawUsageRow { north: self.north.clone(), owner: self.owner.clone(), key: self.key.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveContextUsage {
    pub north: North,
    pub conversation: String,
    pub used: i64,
    pub window: i64,
    pub total: i64,
}
impl galileo_runtime::Event for ObserveContextUsage {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveContextUsage { north: self.north.clone(), conversation: self.conversation.clone(), used: self.used.clone(), window: self.window.clone(), total: self.total.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveQuotaWindow {
    pub north: North,
    pub key: String,
    pub name: String,
    pub used: i64,
    pub duration: i64,
    pub reset: i64,
    pub now: i64,
}
impl galileo_runtime::Event for ObserveQuotaWindow {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveQuotaWindow { north: self.north.clone(), key: self.key.clone(), name: self.name.clone(), used: self.used.clone(), duration: self.duration.clone(), reset: self.reset.clone(), now: self.now.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveResetCredits {
    pub north: North,
    pub count: i64,
    pub known: bool,
}
impl galileo_runtime::Event for ObserveResetCredits {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveResetCredits { north: self.north.clone(), count: self.count.clone(), known: self.known.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandUsage {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandUsage {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandUsage { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactionCompleted {
    pub north: North,
}
impl galileo_runtime::Event for CompactionCompleted {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CompactionCompleted { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandCompact {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandCompact {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandCompact { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearMenuRows {
    pub north: North,
}
impl galileo_runtime::Event for ClearMenuRows {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearMenuRows { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenHistoryMenu {
    pub north: North,
    pub kind: String,
}
impl galileo_runtime::Event for OpenHistoryMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OpenHistoryMenu { north: self.north.clone(), kind: self.kind.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenSettingsMenu {
    pub north: North,
    pub kind: String,
    pub title: String,
}
impl galileo_runtime::Event for OpenSettingsMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OpenSettingsMenu { north: self.north.clone(), kind: self.kind.clone(), title: self.title.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptMenu {
    pub north: North,
}
impl galileo_runtime::Event for AcceptMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AcceptMenu { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryMenu {
    pub north: North,
    pub query: String,
}
impl galileo_runtime::Event for QueryMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::QueryMenu { north: self.north.clone(), query: self.query.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterMenu {
    pub north: North,
}
impl galileo_runtime::Event for FilterMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FilterMenu { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountMenu {
    pub north: North,
}
impl galileo_runtime::Event for CountMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CountMenu { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveMenu {
    pub north: North,
    pub delta: i64,
}
impl galileo_runtime::Event for MoveMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::MoveMenu { north: self.north.clone(), delta: self.delta.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseMenu {
    pub north: North,
}
impl galileo_runtime::Event for CloseMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CloseMenu { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearReferences {
    pub north: North,
}
impl galileo_runtime::Event for ClearReferences {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearReferences { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryReferences {
    pub north: North,
    pub query: String,
}
impl galileo_runtime::Event for QueryReferences {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::QueryReferences { north: self.north.clone(), query: self.query.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferReference {
    pub north: North,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub path: String,
}
impl galileo_runtime::Event for OfferReference {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OfferReference { north: self.north.clone(), name: self.name.clone(), description: self.description.clone(), kind: self.kind.clone(), path: self.path.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveReference {
    pub north: North,
    pub delta: i64,
}
impl galileo_runtime::Event for MoveReference {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::MoveReference { north: self.north.clone(), delta: self.delta.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DismissReferences {
    pub north: North,
}
impl galileo_runtime::Event for DismissReferences {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::DismissReferences { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterTranscript {
    pub north: North,
}
impl galileo_runtime::Event for FilterTranscript {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FilterTranscript { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchTranscript {
    pub north: North,
    pub query: String,
}
impl galileo_runtime::Event for SearchTranscript {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SearchTranscript { north: self.north.clone(), query: self.query.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScrollTranscript {
    pub north: North,
    pub delta: i64,
}
impl galileo_runtime::Event for ScrollTranscript {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ScrollTranscript { north: self.north.clone(), delta: self.delta.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SizeTranscript {
    pub north: North,
    pub limit: i64,
}
impl galileo_runtime::Event for SizeTranscript {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SizeTranscript { north: self.north.clone(), limit: self.limit.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToggleChanges {
    pub north: North,
}
impl galileo_runtime::Event for ToggleChanges {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ToggleChanges { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveSettings {
    pub north: North,
    pub conversation: String,
    pub model: String,
    pub effort: String,
}
impl galileo_runtime::Event for ObserveSettings {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveSettings { north: self.north.clone(), conversation: self.conversation.clone(), model: self.model.clone(), effort: self.effort.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnsureContext {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for EnsureContext {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::EnsureContext { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveDraft {
    pub north: North,
    pub text: String,
}
impl galileo_runtime::Event for SaveDraft {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SaveDraft { north: self.north.clone(), text: self.text.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservePrompt {
    pub north: North,
    pub request: String,
    pub conversation: String,
    pub turn: String,
    pub kind: String,
    pub cancellation: String,
}
impl galileo_runtime::Event for ObservePrompt {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObservePrompt { north: self.north.clone(), request: self.request.clone(), conversation: self.conversation.clone(), turn: self.turn.clone(), kind: self.kind.clone(), cancellation: self.cancellation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservePromptQuestion {
    pub north: North,
    pub number: i64,
    pub position: i64,
    pub key: String,
    pub header: String,
    pub text: String,
    pub secret: bool,
    pub other: bool,
    pub codec: String,
    pub multiple: bool,
    pub optional: bool,
}
impl galileo_runtime::Event for ObservePromptQuestion {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObservePromptQuestion { north: self.north.clone(), number: self.number.clone(), position: self.position.clone(), key: self.key.clone(), header: self.header.clone(), text: self.text.clone(), secret: self.secret.clone(), other: self.other.clone(), codec: self.codec.clone(), multiple: self.multiple.clone(), optional: self.optional.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservePromptOption {
    pub north: North,
    pub number: i64,
    pub position: i64,
    pub index: i64,
    pub label: String,
    pub description: String,
    pub value: String,
}
impl galileo_runtime::Event for ObservePromptOption {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObservePromptOption { north: self.north.clone(), number: self.number.clone(), position: self.position.clone(), index: self.index.clone(), label: self.label.clone(), description: self.description.clone(), value: self.value.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FocusPrompts {
    pub north: North,
}
impl galileo_runtime::Event for FocusPrompts {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FocusPrompts { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentPromptQuestions {
    pub north: North,
}
impl galileo_runtime::Event for PresentPromptQuestions {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PresentPromptQuestions { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigatePrompt {
    pub north: North,
    pub direction: i64,
}
impl galileo_runtime::Event for NavigatePrompt {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::NavigatePrompt { north: self.north.clone(), direction: self.direction.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerPromptChoice {
    pub north: North,
}
impl galileo_runtime::Event for AnswerPromptChoice {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AnswerPromptChoice { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TogglePromptOption {
    pub north: North,
}
impl galileo_runtime::Event for TogglePromptOption {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::TogglePromptOption { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerPromptMultiple {
    pub north: North,
    pub text: String,
}
impl galileo_runtime::Event for AnswerPromptMultiple {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AnswerPromptMultiple { north: self.north.clone(), text: self.text.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkipPromptQuestion {
    pub north: North,
}
impl galileo_runtime::Event for SkipPromptQuestion {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SkipPromptQuestion { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptAnswerProblem {
    pub north: North,
    pub problem: String,
}
impl galileo_runtime::Event for PromptAnswerProblem {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PromptAnswerProblem { north: self.north.clone(), problem: self.problem.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnswerPromptText {
    pub north: North,
    pub text: String,
}
impl galileo_runtime::Event for AnswerPromptText {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AnswerPromptText { north: self.north.clone(), text: self.text.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparePromptResponse {
    pub north: North,
}
impl galileo_runtime::Event for PreparePromptResponse {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PreparePromptResponse { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelPrompt {
    pub north: North,
}
impl galileo_runtime::Event for CancelPrompt {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CancelPrompt { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptResponseWritten {
    pub north: North,
    pub number: i64,
    pub result: String,
}
impl galileo_runtime::Event for PromptResponseWritten {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PromptResponseWritten { north: self.north.clone(), number: self.number.clone(), result: self.result.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvePrompt {
    pub north: North,
    pub conversation: String,
    pub request: String,
}
impl galileo_runtime::Event for ResolvePrompt {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ResolvePrompt { north: self.north.clone(), conversation: self.conversation.clone(), request: self.request.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseTurnPrompts {
    pub north: North,
    pub conversation: String,
    pub turn: String,
}
impl galileo_runtime::Event for CloseTurnPrompts {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CloseTurnPrompts { north: self.north.clone(), conversation: self.conversation.clone(), turn: self.turn.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearResolvedAnswers {
    pub north: North,
}
impl galileo_runtime::Event for ClearResolvedAnswers {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearResolvedAnswers { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveTurn {
    pub north: North,
    pub conversation: String,
    pub turn: String,
}
impl galileo_runtime::Event for ObserveTurn {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveTurn { north: self.north.clone(), conversation: self.conversation.clone(), turn: self.turn.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveStoredTurn {
    pub north: North,
    pub conversation: String,
    pub turn: String,
    pub status: String,
}
impl galileo_runtime::Event for ObserveStoredTurn {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveStoredTurn { north: self.north.clone(), conversation: self.conversation.clone(), turn: self.turn.clone(), status: self.status.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearTurn {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for ClearTurn {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearTurn { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinishObservedTurn {
    pub north: North,
    pub conversation: String,
    pub turn: String,
}
impl galileo_runtime::Event for FinishObservedTurn {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FinishObservedTurn { north: self.north.clone(), conversation: self.conversation.clone(), turn: self.turn.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpireSteering {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for ExpireSteering {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ExpireSteering { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainInput {
    pub north: North,
    pub text: String,
    pub mode: String,
}
impl galileo_runtime::Event for RetainInput {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::RetainInput { north: self.north.clone(), text: self.text.clone(), mode: self.mode.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindInputClientId {
    pub north: North,
    pub number: i64,
    pub client: String,
}
impl galileo_runtime::Event for BindInputClientId {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::BindInputClientId { north: self.north.clone(), number: self.number.clone(), client: self.client.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveInputAcceptance {
    pub north: North,
    pub conversation: String,
    pub client: String,
}
impl galileo_runtime::Event for ObserveInputAcceptance {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveInputAcceptance { north: self.north.clone(), conversation: self.conversation.clone(), client: self.client.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordInputAcceptance {
    pub north: North,
    pub number: i64,
    pub result: String,
}
impl galileo_runtime::Event for RecordInputAcceptance {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::RecordInputAcceptance { north: self.north.clone(), number: self.number.clone(), result: self.result.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputReceipt {
    pub north: North,
    pub number: i64,
    pub result: String,
}
impl galileo_runtime::Event for InputReceipt {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::InputReceipt { north: self.north.clone(), number: self.number.clone(), result: self.result.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareSteering {
    pub north: North,
}
impl galileo_runtime::Event for PrepareSteering {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PrepareSteering { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetainQueuedAttachment {
    pub north: North,
    pub number: i64,
    pub attachment: i64,
}
impl galileo_runtime::Event for RetainQueuedAttachment {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::RetainQueuedAttachment { north: self.north.clone(), number: self.number.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareQueuedInput {
    pub north: North,
}
impl galileo_runtime::Event for PrepareQueuedInput {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PrepareQueuedInput { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitQueued {
    pub north: North,
    pub number: i64,
}
impl galileo_runtime::Event for SubmitQueued {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SubmitQueued { north: self.north.clone(), number: self.number.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyQueuedToSubmitted {
    pub north: North,
    pub number: i64,
    pub attachment: i64,
}
impl galileo_runtime::Event for CopyQueuedToSubmitted {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CopyQueuedToSubmitted { north: self.north.clone(), number: self.number.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareInputEdit {
    pub north: North,
}
impl galileo_runtime::Event for PrepareInputEdit {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::PrepareInputEdit { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoreInputAttachment {
    pub north: North,
    pub number: i64,
    pub attachment: i64,
}
impl galileo_runtime::Event for RestoreInputAttachment {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::RestoreInputAttachment { north: self.north.clone(), number: self.number.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgetInputAttachment {
    pub north: North,
    pub number: i64,
    pub attachment: i64,
}
impl galileo_runtime::Event for ForgetInputAttachment {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ForgetInputAttachment { north: self.north.clone(), number: self.number.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForgetInput {
    pub north: North,
    pub number: i64,
}
impl galileo_runtime::Event for ForgetInput {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ForgetInput { north: self.north.clone(), number: self.number.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendChat {
    pub north: North,
    pub conversation: String,
    pub kind: String,
    pub text: String,
}
impl galileo_runtime::Event for AppendChat {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AppendChat { north: self.north.clone(), conversation: self.conversation.clone(), kind: self.kind.clone(), text: self.text.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveChatItem {
    pub north: North,
    pub conversation: String,
    pub turn: String,
    pub key: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub mode: String,
}
impl galileo_runtime::Event for ObserveChatItem {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveChatItem { north: self.north.clone(), conversation: self.conversation.clone(), turn: self.turn.clone(), key: self.key.clone(), kind: self.kind.clone(), text: self.text.clone(), status: self.status.clone(), mode: self.mode.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearChat {
    pub north: North,
}
impl galileo_runtime::Event for ClearChat {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearChat { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initialize {
    pub north: North,
}
impl galileo_runtime::Event for Initialize {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::Initialize { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandWork {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandWork {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandWork { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspect {
    pub north: North,
}
impl galileo_runtime::Event for Inspect {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::Inspect { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveInput {
    pub north: North,
    pub input: String,
}
impl galileo_runtime::Event for ResolveInput {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ResolveInput { north: self.north.clone(), input: self.input.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectGoal {
    pub north: North,
    pub goal: Goal,
}
impl galileo_runtime::Event for SelectGoal {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SelectGoal { north: self.north.clone(), goal: self.goal.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandNew {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandNew {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandNew { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResume {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandResume {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandResume { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRename {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandRename {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandRename { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandFork {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandFork {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandFork { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandArchived {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandArchived {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandArchived { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandArchive {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandArchive {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandArchive { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveArchived {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for ObserveArchived {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveArchived { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveRestored {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for ObserveRestored {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveRestored { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryActionCompleted {
    pub north: North,
    pub action: String,
}
impl galileo_runtime::Event for HistoryActionCompleted {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::HistoryActionCompleted { north: self.north.clone(), action: self.action.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandModel {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandModel {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandModel { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandEffort {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandEffort {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandEffort { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandConfig {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandConfig {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandConfig { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandQuit {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandQuit {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandQuit { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandDelegate {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandDelegate {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandDelegate { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegateTask {
    pub north: North,
    pub task: String,
}
impl galileo_runtime::Event for DelegateTask {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::DelegateTask { north: self.north.clone(), task: self.task.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitInput {
    pub north: North,
    pub input: String,
}
impl galileo_runtime::Event for SubmitInput {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SubmitInput { north: self.north.clone(), input: self.input.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearHostEffect {
    pub north: North,
}
impl galileo_runtime::Event for ClearHostEffect {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearHostEffect { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandGoal {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandGoal {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandGoal { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalTitle {
    pub north: North,
    pub title: String,
}
impl galileo_runtime::Event for GoalTitle {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::GoalTitle { north: self.north.clone(), title: self.title.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalObjective {
    pub north: North,
    pub objective: String,
}
impl galileo_runtime::Event for GoalObjective {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::GoalObjective { north: self.north.clone(), objective: self.objective.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoalEditInput {
    pub north: North,
    pub objective: String,
}
impl galileo_runtime::Event for GoalEditInput {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::GoalEditInput { north: self.north.clone(), objective: self.objective.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandGoals {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandGoals {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandGoals { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandAgents {
    pub north: North,
    pub command: String,
}
impl galileo_runtime::Event for CommandAgents {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CommandAgents { north: self.north.clone(), command: self.command.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowChat {
    pub north: North,
}
impl galileo_runtime::Event for ShowChat {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ShowChat { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewChatNext {
    pub north: North,
}
impl galileo_runtime::Event for ViewChatNext {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ViewChatNext { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewChatPrevious {
    pub north: North,
}
impl galileo_runtime::Event for ViewChatPrevious {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ViewChatPrevious { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewGoalsNext {
    pub north: North,
}
impl galileo_runtime::Event for ViewGoalsNext {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ViewGoalsNext { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewGoalsPrevious {
    pub north: North,
}
impl galileo_runtime::Event for ViewGoalsPrevious {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ViewGoalsPrevious { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObserveConversation {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for ObserveConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ObserveConversation { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachImage {
    pub north: North,
}
impl galileo_runtime::Event for AttachImage {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AttachImage { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetachImage {
    pub north: North,
    pub attachment: i64,
}
impl galileo_runtime::Event for DetachImage {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::DetachImage { north: self.north.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopySubmissionAttachment {
    pub north: North,
    pub attachment: i64,
}
impl galileo_runtime::Event for CopySubmissionAttachment {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::CopySubmissionAttachment { north: self.north.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClearSubmittedAttachment {
    pub north: North,
    pub conversation: String,
    pub attachment: i64,
}
impl galileo_runtime::Event for ClearSubmittedAttachment {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearSubmittedAttachment { north: self.north.clone(), conversation: self.conversation.clone(), attachment: self.attachment.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestNewConversation {
    pub north: North,
}
impl galileo_runtime::Event for RequestNewConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::RequestNewConversation { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleNewConversation {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for SettleNewConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SettleNewConversation { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailNewConversation {
    pub north: North,
}
impl galileo_runtime::Event for FailNewConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FailNewConversation { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestSwitchConversation {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for RequestSwitchConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::RequestSwitchConversation { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleSwitchConversation {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for SettleSwitchConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SettleSwitchConversation { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailSwitchConversation {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for FailSwitchConversation {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FailSwitchConversation { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submit {
    pub north: North,
}
impl galileo_runtime::Event for Submit {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::Submit { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegate {
    pub north: North,
}
impl galileo_runtime::Event for Delegate {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::Delegate { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildSpawned {
    pub north: North,
    pub conversation: String,
    pub child: String,
}
impl galileo_runtime::Event for ChildSpawned {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ChildSpawned { north: self.north.clone(), conversation: self.conversation.clone(), child: self.child.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleSuccess {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for SettleSuccess {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SettleSuccess { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AbandonTurn {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for AbandonTurn {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AbandonTurn { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleInterrupted {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for SettleInterrupted {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SettleInterrupted { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleFailure {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for SettleFailure {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SettleFailure { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettleDelegationSuccess {
    pub north: North,
    pub conversation: String,
    pub child: String,
}
impl galileo_runtime::Event for SettleDelegationSuccess {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::SettleDelegationSuccess { north: self.north.clone(), conversation: self.conversation.clone(), child: self.child.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailDelegationBeforeChild {
    pub north: North,
    pub conversation: String,
}
impl galileo_runtime::Event for FailDelegationBeforeChild {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FailDelegationBeforeChild { north: self.north.clone(), conversation: self.conversation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailDelegationAfterChild {
    pub north: North,
    pub conversation: String,
    pub child: String,
}
impl galileo_runtime::Event for FailDelegationAfterChild {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::FailDelegationAfterChild { north: self.north.clone(), conversation: self.conversation.clone(), child: self.child.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferRow {
    pub north: North,
    pub key: String,
    pub label: String,
    pub description: String,
    pub annotation: String,
}
impl galileo_runtime::Event for OfferRow {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OfferRow { north: self.north.clone(), key: self.key.clone(), label: self.label.clone(), description: self.description.clone(), annotation: self.annotation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ConnectionLost { north: North },
    PauseDisconnectedInputs { north: North },
    DisconnectPrompts { north: North },
    BeginReconnect { north: North },
    FinishReconnect { north: North, success: bool },
    CommandReconnect { north: North, command: String },
    BeginConversationReconciliation { north: North, conversation: String },
    CountUsage { north: North },
    PresentUsage { north: North },
    WithdrawUsageRow { north: North, owner: String, key: String },
    ObserveContextUsage { north: North, conversation: String, used: i64, window: i64, total: i64 },
    ObserveQuotaWindow { north: North, key: String, name: String, used: i64, duration: i64, reset: i64, now: i64 },
    ObserveResetCredits { north: North, count: i64, known: bool },
    CommandUsage { north: North, command: String },
    CompactionCompleted { north: North },
    CommandCompact { north: North, command: String },
    ClearMenuRows { north: North },
    OpenHistoryMenu { north: North, kind: String },
    OpenSettingsMenu { north: North, kind: String, title: String },
    AcceptMenu { north: North },
    QueryMenu { north: North, query: String },
    FilterMenu { north: North },
    CountMenu { north: North },
    MoveMenu { north: North, delta: i64 },
    CloseMenu { north: North },
    ClearReferences { north: North },
    QueryReferences { north: North, query: String },
    OfferReference { north: North, name: String, description: String, kind: String, path: String },
    MoveReference { north: North, delta: i64 },
    DismissReferences { north: North },
    FilterTranscript { north: North },
    SearchTranscript { north: North, query: String },
    ScrollTranscript { north: North, delta: i64 },
    SizeTranscript { north: North, limit: i64 },
    ToggleChanges { north: North },
    ObserveSettings { north: North, conversation: String, model: String, effort: String },
    EnsureContext { north: North, conversation: String },
    SaveDraft { north: North, text: String },
    ObservePrompt { north: North, request: String, conversation: String, turn: String, kind: String, cancellation: String },
    ObservePromptQuestion { north: North, number: i64, position: i64, key: String, header: String, text: String, secret: bool, other: bool, codec: String, multiple: bool, optional: bool },
    ObservePromptOption { north: North, number: i64, position: i64, index: i64, label: String, description: String, value: String },
    FocusPrompts { north: North },
    PresentPromptQuestions { north: North },
    NavigatePrompt { north: North, direction: i64 },
    AnswerPromptChoice { north: North },
    TogglePromptOption { north: North },
    AnswerPromptMultiple { north: North, text: String },
    SkipPromptQuestion { north: North },
    PromptAnswerProblem { north: North, problem: String },
    AnswerPromptText { north: North, text: String },
    PreparePromptResponse { north: North },
    CancelPrompt { north: North },
    PromptResponseWritten { north: North, number: i64, result: String },
    ResolvePrompt { north: North, conversation: String, request: String },
    CloseTurnPrompts { north: North, conversation: String, turn: String },
    ClearResolvedAnswers { north: North },
    ObserveTurn { north: North, conversation: String, turn: String },
    ObserveStoredTurn { north: North, conversation: String, turn: String, status: String },
    ClearTurn { north: North, conversation: String },
    FinishObservedTurn { north: North, conversation: String, turn: String },
    ExpireSteering { north: North, conversation: String },
    RetainInput { north: North, text: String, mode: String },
    BindInputClientId { north: North, number: i64, client: String },
    ObserveInputAcceptance { north: North, conversation: String, client: String },
    RecordInputAcceptance { north: North, number: i64, result: String },
    InputReceipt { north: North, number: i64, result: String },
    PrepareSteering { north: North },
    RetainQueuedAttachment { north: North, number: i64, attachment: i64 },
    PrepareQueuedInput { north: North },
    SubmitQueued { north: North, number: i64 },
    CopyQueuedToSubmitted { north: North, number: i64, attachment: i64 },
    PrepareInputEdit { north: North },
    RestoreInputAttachment { north: North, number: i64, attachment: i64 },
    ForgetInputAttachment { north: North, number: i64, attachment: i64 },
    ForgetInput { north: North, number: i64 },
    AppendChat { north: North, conversation: String, kind: String, text: String },
    ObserveChatItem { north: North, conversation: String, turn: String, key: String, kind: String, text: String, status: String, mode: String },
    ClearChat { north: North },
    Initialize { north: North },
    CommandWork { north: North, command: String },
    Inspect { north: North },
    ResolveInput { north: North, input: String },
    SelectGoal { north: North, goal: Goal },
    CommandNew { north: North, command: String },
    CommandResume { north: North, command: String },
    CommandRename { north: North, command: String },
    CommandFork { north: North, command: String },
    CommandArchived { north: North, command: String },
    CommandArchive { north: North, command: String },
    ObserveArchived { north: North, conversation: String },
    ObserveRestored { north: North, conversation: String },
    HistoryActionCompleted { north: North, action: String },
    CommandModel { north: North, command: String },
    CommandEffort { north: North, command: String },
    CommandConfig { north: North, command: String },
    CommandQuit { north: North, command: String },
    CommandDelegate { north: North, command: String },
    DelegateTask { north: North, task: String },
    SubmitInput { north: North, input: String },
    ClearHostEffect { north: North },
    CommandGoal { north: North, command: String },
    GoalTitle { north: North, title: String },
    GoalObjective { north: North, objective: String },
    GoalEditInput { north: North, objective: String },
    CommandGoals { north: North, command: String },
    CommandAgents { north: North, command: String },
    ShowChat { north: North },
    ViewChatNext { north: North },
    ViewChatPrevious { north: North },
    ViewGoalsNext { north: North },
    ViewGoalsPrevious { north: North },
    ObserveConversation { north: North, conversation: String },
    AttachImage { north: North },
    DetachImage { north: North, attachment: i64 },
    CopySubmissionAttachment { north: North, attachment: i64 },
    ClearSubmittedAttachment { north: North, conversation: String, attachment: i64 },
    RequestNewConversation { north: North },
    SettleNewConversation { north: North, conversation: String },
    FailNewConversation { north: North },
    RequestSwitchConversation { north: North, conversation: String },
    SettleSwitchConversation { north: North, conversation: String },
    FailSwitchConversation { north: North, conversation: String },
    Submit { north: North },
    Delegate { north: North },
    ChildSpawned { north: North, conversation: String, child: String },
    SettleSuccess { north: North, conversation: String },
    AbandonTurn { north: North, conversation: String },
    SettleInterrupted { north: North, conversation: String },
    SettleFailure { north: North, conversation: String },
    SettleDelegationSuccess { north: North, conversation: String, child: String },
    FailDelegationBeforeChild { north: North, conversation: String },
    FailDelegationAfterChild { north: North, conversation: String, child: String },
    OfferRow { north: North, key: String, label: String, description: String, annotation: String },
}
impl Event {
    pub fn into_tuple(self, program: &Program) -> galileo_runtime::Tuple { match self {
        Self::ConnectionLost { north } => (program.relation_id("clause/north/connection-lost").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::PauseDisconnectedInputs { north } => (program.relation_id("clause/north/pause-disconnected-inputs").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::DisconnectPrompts { north } => (program.relation_id("clause/north/disconnect-prompts").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::BeginReconnect { north } => (program.relation_id("clause/north/begin-reconnect").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::FinishReconnect { north, success } => (program.relation_id("clause/north/finish-reconnect").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Bool(success)]),
        Self::CommandReconnect { north, command } => (program.relation_id("clause/north/command-reconnect").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::BeginConversationReconciliation { north, conversation } => (program.relation_id("clause/north/begin-conversation-reconciliation").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::CountUsage { north } => (program.relation_id("clause/north/count-usage").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::PresentUsage { north } => (program.relation_id("clause/north/present-usage").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::WithdrawUsageRow { north, owner, key } => (program.relation_id("clause/north/withdraw-usage-row").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(owner), Value::Text(key)]),
        Self::ObserveContextUsage { north, conversation, used, window, total } => (program.relation_id("clause/north/observe-context-usage").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Int(used), Value::Int(window), Value::Int(total)]),
        Self::ObserveQuotaWindow { north, key, name, used, duration, reset, now } => (program.relation_id("clause/north/observe-quota-window").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(key), Value::Text(name), Value::Int(used), Value::Int(duration), Value::Int(reset), Value::Int(now)]),
        Self::ObserveResetCredits { north, count, known } => (program.relation_id("clause/north/observe-reset-credits").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(count), Value::Bool(known)]),
        Self::CommandUsage { north, command } => (program.relation_id("clause/north/command-usage").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CompactionCompleted { north } => (program.relation_id("clause/north/compaction-completed").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::CommandCompact { north, command } => (program.relation_id("clause/north/command-compact").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::ClearMenuRows { north } => (program.relation_id("clause/north/clear-menu-rows").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::OpenHistoryMenu { north, kind } => (program.relation_id("clause/north/open-history-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(kind)]),
        Self::OpenSettingsMenu { north, kind, title } => (program.relation_id("clause/north/open-settings-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(kind), Value::Text(title)]),
        Self::AcceptMenu { north } => (program.relation_id("clause/north/accept-menu").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::QueryMenu { north, query } => (program.relation_id("clause/north/query-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(query)]),
        Self::FilterMenu { north } => (program.relation_id("clause/north/filter-menu").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::CountMenu { north } => (program.relation_id("clause/north/count-menu").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::MoveMenu { north, delta } => (program.relation_id("clause/north/move-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(delta)]),
        Self::CloseMenu { north } => (program.relation_id("clause/north/close-menu").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ClearReferences { north } => (program.relation_id("clause/north/clear-references").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::QueryReferences { north, query } => (program.relation_id("clause/north/query-references").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(query)]),
        Self::OfferReference { north, name, description, kind, path } => (program.relation_id("clause/north/offer-reference").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(name), Value::Text(description), Value::Text(kind), Value::Text(path)]),
        Self::MoveReference { north, delta } => (program.relation_id("clause/north/move-reference").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(delta)]),
        Self::DismissReferences { north } => (program.relation_id("clause/north/dismiss-references").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::FilterTranscript { north } => (program.relation_id("clause/north/filter-transcript").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::SearchTranscript { north, query } => (program.relation_id("clause/north/search-transcript").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(query)]),
        Self::ScrollTranscript { north, delta } => (program.relation_id("clause/north/scroll-transcript").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(delta)]),
        Self::SizeTranscript { north, limit } => (program.relation_id("clause/north/size-transcript").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(limit)]),
        Self::ToggleChanges { north } => (program.relation_id("clause/north/toggle-changes").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ObserveSettings { north, conversation, model, effort } => (program.relation_id("clause/north/observe-settings").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(model), Value::Text(effort)]),
        Self::EnsureContext { north, conversation } => (program.relation_id("clause/north/ensure-context").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::SaveDraft { north, text } => (program.relation_id("clause/north/save-draft").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(text)]),
        Self::ObservePrompt { north, request, conversation, turn, kind, cancellation } => (program.relation_id("clause/north/observe-prompt").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(request), Value::Text(conversation), Value::Text(turn), Value::Text(kind), Value::Text(cancellation)]),
        Self::ObservePromptQuestion { north, number, position, key, header, text, secret, other, codec, multiple, optional } => (program.relation_id("clause/north/observe-prompt-question").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Int(position), Value::Text(key), Value::Text(header), Value::Text(text), Value::Bool(secret), Value::Bool(other), Value::Text(codec), Value::Bool(multiple), Value::Bool(optional)]),
        Self::ObservePromptOption { north, number, position, index, label, description, value } => (program.relation_id("clause/north/observe-prompt-option").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Int(position), Value::Int(index), Value::Text(label), Value::Text(description), Value::Text(value)]),
        Self::FocusPrompts { north } => (program.relation_id("clause/north/focus-prompts").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::PresentPromptQuestions { north } => (program.relation_id("clause/north/present-prompt-questions").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::NavigatePrompt { north, direction } => (program.relation_id("clause/north/navigate-prompt").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(direction)]),
        Self::AnswerPromptChoice { north } => (program.relation_id("clause/north/answer-prompt-choice").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::TogglePromptOption { north } => (program.relation_id("clause/north/toggle-prompt-option").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::AnswerPromptMultiple { north, text } => (program.relation_id("clause/north/answer-prompt-multiple").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(text)]),
        Self::SkipPromptQuestion { north } => (program.relation_id("clause/north/skip-prompt-question").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::PromptAnswerProblem { north, problem } => (program.relation_id("clause/north/prompt-answer-problem").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(problem)]),
        Self::AnswerPromptText { north, text } => (program.relation_id("clause/north/answer-prompt-text").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(text)]),
        Self::PreparePromptResponse { north } => (program.relation_id("clause/north/prepare-prompt-response").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::CancelPrompt { north } => (program.relation_id("clause/north/cancel-prompt").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::PromptResponseWritten { north, number, result } => (program.relation_id("clause/north/prompt-response-written").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Text(result)]),
        Self::ResolvePrompt { north, conversation, request } => (program.relation_id("clause/north/resolve-prompt").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(request)]),
        Self::CloseTurnPrompts { north, conversation, turn } => (program.relation_id("clause/north/close-turn-prompts").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(turn)]),
        Self::ClearResolvedAnswers { north } => (program.relation_id("clause/north/clear-resolved-answers").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ObserveTurn { north, conversation, turn } => (program.relation_id("clause/north/observe-turn").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(turn)]),
        Self::ObserveStoredTurn { north, conversation, turn, status } => (program.relation_id("clause/north/observe-stored-turn").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(turn), Value::Text(status)]),
        Self::ClearTurn { north, conversation } => (program.relation_id("clause/north/clear-turn").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::FinishObservedTurn { north, conversation, turn } => (program.relation_id("clause/north/finish-observed-turn").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(turn)]),
        Self::ExpireSteering { north, conversation } => (program.relation_id("clause/north/expire-steering").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::RetainInput { north, text, mode } => (program.relation_id("clause/north/retain-input").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(text), Value::Text(mode)]),
        Self::BindInputClientId { north, number, client } => (program.relation_id("clause/north/bind-input-client-id").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Text(client)]),
        Self::ObserveInputAcceptance { north, conversation, client } => (program.relation_id("clause/north/observe-input-acceptance").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(client)]),
        Self::RecordInputAcceptance { north, number, result } => (program.relation_id("clause/north/record-input-acceptance").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Text(result)]),
        Self::InputReceipt { north, number, result } => (program.relation_id("clause/north/input-receipt").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Text(result)]),
        Self::PrepareSteering { north } => (program.relation_id("clause/north/prepare-steering").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::RetainQueuedAttachment { north, number, attachment } => (program.relation_id("clause/north/retain-queued-attachment").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Int(attachment)]),
        Self::PrepareQueuedInput { north } => (program.relation_id("clause/north/prepare-queued-input").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::SubmitQueued { north, number } => (program.relation_id("clause/north/submit-queued").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number)]),
        Self::CopyQueuedToSubmitted { north, number, attachment } => (program.relation_id("clause/north/copy-queued-to-submitted").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Int(attachment)]),
        Self::PrepareInputEdit { north } => (program.relation_id("clause/north/prepare-input-edit").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::RestoreInputAttachment { north, number, attachment } => (program.relation_id("clause/north/restore-input-attachment").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Int(attachment)]),
        Self::ForgetInputAttachment { north, number, attachment } => (program.relation_id("clause/north/forget-input-attachment").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number), Value::Int(attachment)]),
        Self::ForgetInput { north, number } => (program.relation_id("clause/north/forget-input").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(number)]),
        Self::AppendChat { north, conversation, kind, text } => (program.relation_id("clause/north/append-chat").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(kind), Value::Text(text)]),
        Self::ObserveChatItem { north, conversation, turn, key, kind, text, status, mode } => (program.relation_id("clause/north/observe-chat-item").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(turn), Value::Text(key), Value::Text(kind), Value::Text(text), Value::Text(status), Value::Text(mode)]),
        Self::ClearChat { north } => (program.relation_id("clause/north/clear-chat").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::Initialize { north } => (program.relation_id("clause/north/initialize").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::CommandWork { north, command } => (program.relation_id("clause/north/command-work").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::Inspect { north } => (program.relation_id("clause/north/inspect").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ResolveInput { north, input } => (program.relation_id("clause/north/resolve-input").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(input)]),
        Self::SelectGoal { north, goal } => (program.relation_id("clause/north/select-goal").expect("generated relation belongs to this program"), vec![north.into_value(), goal.into_value()]),
        Self::CommandNew { north, command } => (program.relation_id("clause/north/command-new").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandResume { north, command } => (program.relation_id("clause/north/command-resume").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandRename { north, command } => (program.relation_id("clause/north/command-rename").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandFork { north, command } => (program.relation_id("clause/north/command-fork").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandArchived { north, command } => (program.relation_id("clause/north/command-archived").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandArchive { north, command } => (program.relation_id("clause/north/command-archive").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::ObserveArchived { north, conversation } => (program.relation_id("clause/north/observe-archived").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::ObserveRestored { north, conversation } => (program.relation_id("clause/north/observe-restored").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::HistoryActionCompleted { north, action } => (program.relation_id("clause/north/history-action-completed").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(action)]),
        Self::CommandModel { north, command } => (program.relation_id("clause/north/command-model").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandEffort { north, command } => (program.relation_id("clause/north/command-effort").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandConfig { north, command } => (program.relation_id("clause/north/command-config").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandQuit { north, command } => (program.relation_id("clause/north/command-quit").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandDelegate { north, command } => (program.relation_id("clause/north/command-delegate").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::DelegateTask { north, task } => (program.relation_id("clause/north/delegate-task").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(task)]),
        Self::SubmitInput { north, input } => (program.relation_id("clause/north/submit-input").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(input)]),
        Self::ClearHostEffect { north } => (program.relation_id("clause/north/clear-host-effect").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::CommandGoal { north, command } => (program.relation_id("clause/north/command-goal").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::GoalTitle { north, title } => (program.relation_id("clause/north/goal-title").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(title)]),
        Self::GoalObjective { north, objective } => (program.relation_id("clause/north/goal-objective").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(objective)]),
        Self::GoalEditInput { north, objective } => (program.relation_id("clause/north/goal-edit-input").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(objective)]),
        Self::CommandGoals { north, command } => (program.relation_id("clause/north/command-goals").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::CommandAgents { north, command } => (program.relation_id("clause/north/command-agents").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(command)]),
        Self::ShowChat { north } => (program.relation_id("clause/north/show-chat").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ViewChatNext { north } => (program.relation_id("clause/north/view-chat-next").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ViewChatPrevious { north } => (program.relation_id("clause/north/view-chat-previous").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ViewGoalsNext { north } => (program.relation_id("clause/north/view-goals-next").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ViewGoalsPrevious { north } => (program.relation_id("clause/north/view-goals-previous").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ObserveConversation { north, conversation } => (program.relation_id("clause/north/observe-conversation").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::AttachImage { north } => (program.relation_id("clause/north/attach-image").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::DetachImage { north, attachment } => (program.relation_id("clause/north/detach-image").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(attachment)]),
        Self::CopySubmissionAttachment { north, attachment } => (program.relation_id("clause/north/copy-submission-attachment").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(attachment)]),
        Self::ClearSubmittedAttachment { north, conversation, attachment } => (program.relation_id("clause/north/clear-submitted-attachment").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Int(attachment)]),
        Self::RequestNewConversation { north } => (program.relation_id("clause/north/request-new-conversation").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::SettleNewConversation { north, conversation } => (program.relation_id("clause/north/settle-new-conversation").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::FailNewConversation { north } => (program.relation_id("clause/north/fail-new-conversation").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::RequestSwitchConversation { north, conversation } => (program.relation_id("clause/north/request-switch-conversation").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::SettleSwitchConversation { north, conversation } => (program.relation_id("clause/north/settle-switch-conversation").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::FailSwitchConversation { north, conversation } => (program.relation_id("clause/north/fail-switch-conversation").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::Submit { north } => (program.relation_id("clause/north/submit").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::Delegate { north } => (program.relation_id("clause/north/delegate").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ChildSpawned { north, conversation, child } => (program.relation_id("clause/north/child-spawned").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(child)]),
        Self::SettleSuccess { north, conversation } => (program.relation_id("clause/north/settle-success").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::AbandonTurn { north, conversation } => (program.relation_id("clause/north/abandon-turn").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::SettleInterrupted { north, conversation } => (program.relation_id("clause/north/settle-interrupted").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::SettleFailure { north, conversation } => (program.relation_id("clause/north/settle-failure").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::SettleDelegationSuccess { north, conversation, child } => (program.relation_id("clause/north/settle-delegation-success").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(child)]),
        Self::FailDelegationBeforeChild { north, conversation } => (program.relation_id("clause/north/fail-delegation-before-child").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation)]),
        Self::FailDelegationAfterChild { north, conversation, child } => (program.relation_id("clause/north/fail-delegation-after-child").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(conversation), Value::Text(child)]),
        Self::OfferRow { north, key, label, description, annotation } => (program.relation_id("clause/north/offer-row").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(key), Value::Text(label), Value::Text(description), Value::Text(annotation)]),
    } }
    pub fn from_tuple(tuple: galileo_runtime::Tuple, program: &Program) -> Result<Self, EvalError> {
        let (relation, values) = tuple;
        match program.relation(relation).map(|relation| relation.name.as_str()) {
        Some("clause/north/connection-lost") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ConnectionLost {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/pause-disconnected-inputs") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PauseDisconnectedInputs {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/disconnect-prompts") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::DisconnectPrompts {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/begin-reconnect") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::BeginReconnect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/finish-reconnect") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::FinishReconnect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            success: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-reconnect") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandReconnect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/begin-conversation-reconciliation") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::BeginConversationReconciliation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/count-usage") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::CountUsage {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/present-usage") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PresentUsage {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/withdraw-usage-row") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::WithdrawUsageRow {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            owner: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-context-usage") if values.len() == 5 => { let mut values = values.iter(); Ok(Self::ObserveContextUsage {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            used: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            window: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            total: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-quota-window") if values.len() == 7 => { let mut values = values.iter(); Ok(Self::ObserveQuotaWindow {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            name: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            used: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            duration: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            reset: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            now: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-reset-credits") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ObserveResetCredits {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            count: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            known: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-usage") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandUsage {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/compaction-completed") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::CompactionCompleted {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/command-compact") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandCompact {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/clear-menu-rows") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ClearMenuRows {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/open-history-menu") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::OpenHistoryMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/open-settings-menu") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::OpenSettingsMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            title: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/accept-menu") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::AcceptMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/query-menu") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::QueryMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            query: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/filter-menu") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::FilterMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/count-menu") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::CountMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/move-menu") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::MoveMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            delta: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/close-menu") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::CloseMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/clear-references") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ClearReferences {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/query-references") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::QueryReferences {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            query: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/offer-reference") if values.len() == 5 => { let mut values = values.iter(); Ok(Self::OfferReference {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            name: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            description: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            path: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/move-reference") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::MoveReference {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            delta: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/dismiss-references") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::DismissReferences {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/filter-transcript") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::FilterTranscript {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/search-transcript") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SearchTranscript {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            query: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/scroll-transcript") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ScrollTranscript {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            delta: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/size-transcript") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SizeTranscript {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            limit: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/toggle-changes") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ToggleChanges {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/observe-settings") if values.len() == 4 => { let mut values = values.iter(); Ok(Self::ObserveSettings {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            model: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            effort: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/ensure-context") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::EnsureContext {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/save-draft") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SaveDraft {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-prompt") if values.len() == 6 => { let mut values = values.iter(); Ok(Self::ObservePrompt {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            request: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            turn: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            cancellation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-prompt-question") if values.len() == 11 => { let mut values = values.iter(); Ok(Self::ObservePromptQuestion {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            position: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            header: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            secret: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            other: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            codec: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            multiple: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            optional: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-prompt-option") if values.len() == 7 => { let mut values = values.iter(); Ok(Self::ObservePromptOption {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            position: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            index: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            label: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            description: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            value: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/focus-prompts") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::FocusPrompts {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/present-prompt-questions") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PresentPromptQuestions {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/navigate-prompt") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::NavigatePrompt {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            direction: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/answer-prompt-choice") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::AnswerPromptChoice {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/toggle-prompt-option") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::TogglePromptOption {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/answer-prompt-multiple") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::AnswerPromptMultiple {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/skip-prompt-question") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::SkipPromptQuestion {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/prompt-answer-problem") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::PromptAnswerProblem {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            problem: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/answer-prompt-text") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::AnswerPromptText {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/prepare-prompt-response") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PreparePromptResponse {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/cancel-prompt") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::CancelPrompt {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/prompt-response-written") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::PromptResponseWritten {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            result: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/resolve-prompt") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ResolvePrompt {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            request: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/close-turn-prompts") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::CloseTurnPrompts {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            turn: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/clear-resolved-answers") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ClearResolvedAnswers {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/observe-turn") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ObserveTurn {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            turn: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-stored-turn") if values.len() == 4 => { let mut values = values.iter(); Ok(Self::ObserveStoredTurn {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            turn: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            status: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/clear-turn") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ClearTurn {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/finish-observed-turn") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::FinishObservedTurn {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            turn: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/expire-steering") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ExpireSteering {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/retain-input") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::RetainInput {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            mode: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/bind-input-client-id") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::BindInputClientId {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            client: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-input-acceptance") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ObserveInputAcceptance {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            client: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/record-input-acceptance") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::RecordInputAcceptance {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            result: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/input-receipt") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::InputReceipt {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            result: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/prepare-steering") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PrepareSteering {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/retain-queued-attachment") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::RetainQueuedAttachment {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/prepare-queued-input") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PrepareQueuedInput {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/submit-queued") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SubmitQueued {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/copy-queued-to-submitted") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::CopyQueuedToSubmitted {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/prepare-input-edit") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::PrepareInputEdit {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/restore-input-attachment") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::RestoreInputAttachment {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/forget-input-attachment") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ForgetInputAttachment {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/forget-input") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ForgetInput {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            number: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/append-chat") if values.len() == 4 => { let mut values = values.iter(); Ok(Self::AppendChat {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-chat-item") if values.len() == 8 => { let mut values = values.iter(); Ok(Self::ObserveChatItem {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            turn: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            text: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            status: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            mode: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/clear-chat") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ClearChat {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/initialize") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::Initialize {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/command-work") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandWork {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/inspect") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::Inspect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/resolve-input") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ResolveInput {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            input: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/select-goal") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SelectGoal {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            goal: (Goal::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/command-new") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandNew {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-resume") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandResume {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-rename") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandRename {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-fork") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandFork {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-archived") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandArchived {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-archive") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandArchive {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-archived") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ObserveArchived {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/observe-restored") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ObserveRestored {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/history-action-completed") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::HistoryActionCompleted {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            action: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-model") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandModel {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-effort") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandEffort {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-config") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandConfig {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-quit") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandQuit {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-delegate") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandDelegate {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/delegate-task") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::DelegateTask {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            task: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/submit-input") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SubmitInput {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            input: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/clear-host-effect") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ClearHostEffect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/command-goal") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandGoal {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/goal-title") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::GoalTitle {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            title: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/goal-objective") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::GoalObjective {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            objective: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/goal-edit-input") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::GoalEditInput {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            objective: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-goals") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandGoals {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/command-agents") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CommandAgents {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            command: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/show-chat") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ShowChat {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/view-chat-next") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ViewChatNext {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/view-chat-previous") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ViewChatPrevious {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/view-goals-next") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ViewGoalsNext {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/view-goals-previous") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::ViewGoalsPrevious {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/observe-conversation") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::ObserveConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/attach-image") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::AttachImage {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/detach-image") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::DetachImage {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/copy-submission-attachment") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::CopySubmissionAttachment {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/clear-submitted-attachment") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ClearSubmittedAttachment {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            attachment: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/request-new-conversation") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::RequestNewConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/settle-new-conversation") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SettleNewConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/fail-new-conversation") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::FailNewConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/request-switch-conversation") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::RequestSwitchConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/settle-switch-conversation") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SettleSwitchConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/fail-switch-conversation") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::FailSwitchConversation {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/submit") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::Submit {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/delegate") if values.len() == 1 => { let mut values = values.iter(); Ok(Self::Delegate {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("clause/north/child-spawned") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::ChildSpawned {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            child: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/settle-success") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SettleSuccess {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/abandon-turn") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::AbandonTurn {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/settle-interrupted") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SettleInterrupted {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/settle-failure") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::SettleFailure {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/settle-delegation-success") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::SettleDelegationSuccess {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            child: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/fail-delegation-before-child") if values.len() == 2 => { let mut values = values.iter(); Ok(Self::FailDelegationBeforeChild {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/fail-delegation-after-child") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::FailDelegationAfterChild {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            conversation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            child: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        Some("clause/north/offer-row") if values.len() == 5 => { let mut values = values.iter(); Ok(Self::OfferRow {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            label: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            description: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            annotation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        _ => Err(invalid("tuple")),
    } }
}
impl galileo_runtime::Event for Event {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { self.clone().into_tuple(program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    MenuHostEffect { north: North, effect: String, payload: String },
}
impl Effect {
    pub fn into_tuple(self, program: &Program) -> galileo_runtime::Tuple { match self {
        Self::MenuHostEffect { north, effect, payload } => (program.relation_id("clause/north/menu-host-effect").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(effect), Value::Text(payload)]),
    } }
    pub fn from_tuple(tuple: galileo_runtime::Tuple, program: &Program) -> Result<Self, EvalError> {
        let (relation, values) = tuple;
        match program.relation(relation).map(|relation| relation.name.as_str()) {
        Some("clause/north/menu-host-effect") if values.len() == 3 => { let mut values = values.iter(); Ok(Self::MenuHostEffect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            effect: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            payload: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        }) },
        _ => Err(invalid("tuple")),
    } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuRows {
    pub annotation: String,
    pub description: String,
    pub key: String,
    pub label: String,
    pub position: i64,
    pub row: MenuRow,
}
impl MenuRows {
    pub fn from_value(value: &Value) -> Result<Self, EvalError> {
        let Value::Record(fields) = value else { return Err(invalid("MenuRows")); };
        let mut fields = fields.iter();
        Ok(Self {
            annotation: (match fields.find(|(name, _)| name.as_str() == "annotation").map(|(_, value)| value).ok_or_else(|| invalid("annotation"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            description: (match fields.find(|(name, _)| name.as_str() == "description").map(|(_, value)| value).ok_or_else(|| invalid("description"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            key: (match fields.find(|(name, _)| name.as_str() == "key").map(|(_, value)| value).ok_or_else(|| invalid("key"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            label: (match fields.find(|(name, _)| name.as_str() == "label").map(|(_, value)| value).ok_or_else(|| invalid("label"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            position: (match fields.find(|(name, _)| name.as_str() == "position").map(|(_, value)| value).ok_or_else(|| invalid("position"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            row: (MenuRow::from_value(fields.find(|(name, _)| name.as_str() == "row").map(|(_, value)| value).ok_or_else(|| invalid("row"))?))?,
        })
    }
    pub fn into_value(self) -> Value { Value::Record(std::collections::BTreeMap::from([
        ("annotation".into(), Value::Text(self.annotation)),
        ("description".into(), Value::Text(self.description)),
        ("key".into(), Value::Text(self.key)),
        ("label".into(), Value::Text(self.label)),
        ("position".into(), Value::Int(self.position)),
        ("row".into(), self.row.into_value()),
    ])) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub empty: String,
    pub help: String,
    pub kind: String,
    pub query: String,
    pub rows: std::sync::Arc<[MenuRows]>,
    pub selection: i64,
    pub title: String,
}
impl Menu {
    pub fn from_value(value: &Value) -> Result<Self, EvalError> {
        let Value::Record(fields) = value else { return Err(invalid("Menu")); };
        let mut fields = fields.iter();
        Ok(Self {
            empty: (match fields.find(|(name, _)| name.as_str() == "empty").map(|(_, value)| value).ok_or_else(|| invalid("empty"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            help: (match fields.find(|(name, _)| name.as_str() == "help").map(|(_, value)| value).ok_or_else(|| invalid("help"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            kind: (match fields.find(|(name, _)| name.as_str() == "kind").map(|(_, value)| value).ok_or_else(|| invalid("kind"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            query: (match fields.find(|(name, _)| name.as_str() == "query").map(|(_, value)| value).ok_or_else(|| invalid("query"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            rows: (match fields.find(|(name, _)| name.as_str() == "rows").map(|(_, value)| value).ok_or_else(|| invalid("rows"))? { Value::List(items) => items.iter().map(|item| MenuRows::from_value(item)).collect::<Result<Vec<_>, EvalError>>().map(std::sync::Arc::from), _ => Err(invalid("decode")) })?,
            selection: (match fields.find(|(name, _)| name.as_str() == "selection").map(|(_, value)| value).ok_or_else(|| invalid("selection"))? { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            title: (match fields.find(|(name, _)| name.as_str() == "title").map(|(_, value)| value).ok_or_else(|| invalid("title"))? { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        })
    }
    fn from_fields(fields: &mut galileo_runtime::Fields<'_>) -> Result<Self, EvalError> {
        Ok(Self {
            empty: fields.field::<String>("empty", |value| match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            help: fields.field::<String>("help", |value| match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            kind: fields.field::<String>("kind", |value| match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            query: fields.field::<String>("query", |value| match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            rows: fields.field::<std::sync::Arc<[MenuRows]>>("rows", |value| match value { Value::List(items) => items.iter().map(|item| MenuRows::from_value(item)).collect::<Result<Vec<_>, EvalError>>().map(std::sync::Arc::from), _ => Err(invalid("decode")) })?,
            selection: fields.field::<i64>("selection", |value| match value { Value::Int(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
            title: fields.field::<String>("title", |value| match value { Value::Text(item) => Ok(item.clone()), _ => Err(invalid("decode")) })?,
        })
    }
    pub fn into_value(self) -> Value { Value::Record(std::collections::BTreeMap::from([
        ("empty".into(), Value::Text(self.empty)),
        ("help".into(), Value::Text(self.help)),
        ("kind".into(), Value::Text(self.kind)),
        ("query".into(), Value::Text(self.query)),
        ("rows".into(), Value::List(self.rows.iter().cloned().into_iter().map(|item| item.into_value()).collect())),
        ("selection".into(), Value::Int(self.selection)),
        ("title".into(), Value::Text(self.title)),
    ])) }
}
impl galileo_runtime::Projection for Menu {
    type Arguments = (North,);
    fn view(program: &Program) -> &galileo_core::View { program.views.iter().find(|view| view.name == "clause/north/Menu").expect("generated view belongs to this program") }
    fn arguments(arguments: Self::Arguments) -> Vec<Value> { let (north,) = arguments; vec![north.into_value()] }
    fn decode(fields: &mut galileo_runtime::Fields<'_>) -> Result<Self, EvalError> { Self::from_fields(fields) }
}
