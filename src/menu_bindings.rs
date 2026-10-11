use galileo_core::{Entity, EvalError, Value};
use galileo_runtime::{Program, State};

fn invalid(at: &str) -> EvalError { EvalError::Type { at: at.into(), message: "value does not match generated binding".into() } }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuRow(pub Entity);
impl MenuRow {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("examples/north-menu.MenuRow", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "examples/north-menu.MenuRow" => Ok(Self(entity)), _ => Err(invalid("MenuRow")) } }
    pub fn key(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("examples/north-menu/MenuRow.key") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).cloned().unwrap_or(Value::Null);
        match value { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) }
    }
    pub fn label(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("examples/north-menu/MenuRow.label") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).cloned().unwrap_or(Value::Null);
        match value { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) }
    }
    pub fn description(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("examples/north-menu/MenuRow.description") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).cloned().unwrap_or(Value::Null);
        match value { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) }
    }
    pub fn annotation(&self, state: &State, program: &Program) -> Result<String, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("examples/north-menu/MenuRow.annotation") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).cloned().unwrap_or(Value::Null);
        match value { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) }
    }
    pub fn order(&self, state: &State, program: &Program) -> Result<i64, EvalError> {
        let value = state.facts.iter().find(|(relation, values)| Some(*relation) == program.relation_id("examples/north-menu/MenuRow.order") && values.first() == Some(&Value::Entity(self.0.clone()))).and_then(|(_, values)| values.get(1)).cloned().unwrap_or(Value::Null);
        match value { Value::Int(item) => Ok(item), _ => Err(invalid("decode")) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct North(pub Entity);
impl North {
    pub fn named(name: impl Into<String>) -> Self { Self(Entity::named("examples/north-menu.North", name)) }
    pub fn into_value(self) -> Value { Value::Entity(self.0) }
    pub fn from_value(value: Value) -> Result<Self, EvalError> { match value { Value::Entity(entity) if entity.entity_type == "examples/north-menu.North" => Ok(Self(entity)), _ => Err(invalid("North")) } }
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
pub struct OfferWorkRow {
    pub north: North,
    pub reference: String,
    pub title: String,
    pub state: String,
    pub holder: String,
    pub clock: String,
    pub eta: String,
    pub blocked: bool,
}
impl galileo_runtime::Event for OfferWorkRow {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OfferWorkRow { north: self.north.clone(), reference: self.reference.clone(), title: self.title.clone(), state: self.state.clone(), holder: self.holder.clone(), clock: self.clock.clone(), eta: self.eta.clone(), blocked: self.blocked.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptMenu {
    pub north: North,
}
impl galileo_runtime::Event for AcceptMenu {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::AcceptMenu { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferSettingsRow {
    pub north: North,
    pub key: String,
    pub label: String,
    pub description: String,
    pub annotation: String,
}
impl galileo_runtime::Event for OfferSettingsRow {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OfferSettingsRow { north: self.north.clone(), key: self.key.clone(), label: self.label.clone(), description: self.description.clone(), annotation: self.annotation.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferHistoryRow {
    pub north: North,
    pub key: String,
    pub label: String,
    pub description: String,
}
impl galileo_runtime::Event for OfferHistoryRow {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::OfferHistoryRow { north: self.north.clone(), key: self.key.clone(), label: self.label.clone(), description: self.description.clone() }, program) }
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
pub struct ClearHostEffect {
    pub north: North,
}
impl galileo_runtime::Event for ClearHostEffect {
    fn tuple(&self, program: &Program) -> galileo_runtime::Tuple { galileo_runtime::Event::tuple(&Event::ClearHostEffect { north: self.north.clone() }, program) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    ClearMenuRows { north: North },
    OpenHistoryMenu { north: North, kind: String },
    OpenSettingsMenu { north: North, kind: String, title: String },
    OfferWorkRow { north: North, reference: String, title: String, state: String, holder: String, clock: String, eta: String, blocked: bool },
    AcceptMenu { north: North },
    OfferSettingsRow { north: North, key: String, label: String, description: String, annotation: String },
    OfferHistoryRow { north: North, key: String, label: String, description: String },
    QueryMenu { north: North, query: String },
    MoveMenu { north: North, delta: i64 },
    CloseMenu { north: North },
    ClearHostEffect { north: North },
}
impl Event {
    pub fn into_tuple(self, program: &Program) -> galileo_runtime::Tuple { match self {
        Self::ClearMenuRows { north } => (program.relation_id("examples/north-menu/clear-menu-rows").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::OpenHistoryMenu { north, kind } => (program.relation_id("examples/north-menu/open-history-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(kind)]),
        Self::OpenSettingsMenu { north, kind, title } => (program.relation_id("examples/north-menu/open-settings-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(kind), Value::Text(title)]),
        Self::OfferWorkRow { north, reference, title, state, holder, clock, eta, blocked } => (program.relation_id("examples/north-menu/offer-work-row").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(reference), Value::Text(title), Value::Text(state), Value::Text(holder), Value::Text(clock), Value::Text(eta), Value::Bool(blocked)]),
        Self::AcceptMenu { north } => (program.relation_id("examples/north-menu/accept-menu").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::OfferSettingsRow { north, key, label, description, annotation } => (program.relation_id("examples/north-menu/offer-settings-row").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(key), Value::Text(label), Value::Text(description), Value::Text(annotation)]),
        Self::OfferHistoryRow { north, key, label, description } => (program.relation_id("examples/north-menu/offer-history-row").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(key), Value::Text(label), Value::Text(description)]),
        Self::QueryMenu { north, query } => (program.relation_id("examples/north-menu/query-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(query)]),
        Self::MoveMenu { north, delta } => (program.relation_id("examples/north-menu/move-menu").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Int(delta)]),
        Self::CloseMenu { north } => (program.relation_id("examples/north-menu/close-menu").expect("generated relation belongs to this program"), vec![north.into_value()]),
        Self::ClearHostEffect { north } => (program.relation_id("examples/north-menu/clear-host-effect").expect("generated relation belongs to this program"), vec![north.into_value()]),
    } }
    pub fn from_tuple(tuple: galileo_runtime::Tuple, program: &Program) -> Result<Self, EvalError> {
        let (relation, values) = tuple;
        match program.relation(relation).map(|relation| relation.name.as_str()) {
        Some("examples/north-menu/clear-menu-rows") if values.len() == 1 => { let mut values = values.into_iter(); Ok(Self::ClearMenuRows {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("examples/north-menu/open-history-menu") if values.len() == 2 => { let mut values = values.into_iter(); Ok(Self::OpenHistoryMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/open-settings-menu") if values.len() == 3 => { let mut values = values.into_iter(); Ok(Self::OpenSettingsMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            kind: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            title: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/offer-work-row") if values.len() == 8 => { let mut values = values.into_iter(); Ok(Self::OfferWorkRow {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            reference: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            title: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            state: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            holder: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            clock: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            eta: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            blocked: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Bool(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/accept-menu") if values.len() == 1 => { let mut values = values.into_iter(); Ok(Self::AcceptMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("examples/north-menu/offer-settings-row") if values.len() == 5 => { let mut values = values.into_iter(); Ok(Self::OfferSettingsRow {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            label: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            description: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            annotation: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/offer-history-row") if values.len() == 4 => { let mut values = values.into_iter(); Ok(Self::OfferHistoryRow {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            key: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            label: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            description: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/query-menu") if values.len() == 2 => { let mut values = values.into_iter(); Ok(Self::QueryMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            query: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/move-menu") if values.len() == 2 => { let mut values = values.into_iter(); Ok(Self::MoveMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            delta: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Int(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        Some("examples/north-menu/close-menu") if values.len() == 1 => { let mut values = values.into_iter(); Ok(Self::CloseMenu {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
        }) },
        Some("examples/north-menu/clear-host-effect") if values.len() == 1 => { let mut values = values.into_iter(); Ok(Self::ClearHostEffect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
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
        Self::MenuHostEffect { north, effect, payload } => (program.relation_id("examples/north-menu/menu-host-effect").expect("generated relation belongs to this program"), vec![north.into_value(), Value::Text(effect), Value::Text(payload)]),
    } }
    pub fn from_tuple(tuple: galileo_runtime::Tuple, program: &Program) -> Result<Self, EvalError> {
        let (relation, values) = tuple;
        match program.relation(relation).map(|relation| relation.name.as_str()) {
        Some("examples/north-menu/menu-host-effect") if values.len() == 3 => { let mut values = values.into_iter(); Ok(Self::MenuHostEffect {
            north: (North::from_value(values.next().ok_or_else(|| invalid("tuple"))?))?,
            effect: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            payload: (match values.next().ok_or_else(|| invalid("tuple"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
        }) },
        _ => Err(invalid("tuple")),
    } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub kind: String,
    pub query: String,
    pub rows: Vec<MenuRow>,
    pub selection: i64,
}
impl Menu {
    pub fn from_value(value: Value) -> Result<Self, EvalError> {
        let Value::Record(mut fields) = value else { return Err(invalid("Menu")); };
        Ok(Self {
            kind: (match fields.remove("kind").ok_or_else(|| invalid("kind"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            query: (match fields.remove("query").ok_or_else(|| invalid("query"))? { Value::Text(item) => Ok(item), _ => Err(invalid("decode")) })?,
            rows: (match fields.remove("rows").ok_or_else(|| invalid("rows"))? { Value::List(items) => items.into_iter().map(|item| MenuRow::from_value(item)).collect::<Result<Vec<_>, EvalError>>(), _ => Err(invalid("decode")) })?,
            selection: (match fields.remove("selection").ok_or_else(|| invalid("selection"))? { Value::Int(item) => Ok(item), _ => Err(invalid("decode")) })?,
        })
    }
    pub fn into_value(self) -> Value { Value::Record(std::collections::BTreeMap::from([
        ("kind".into(), Value::Text(self.kind)),
        ("query".into(), Value::Text(self.query)),
        ("rows".into(), Value::List(self.rows.into_iter().map(|item| item.into_value()).collect())),
        ("selection".into(), Value::Int(self.selection)),
    ])) }
}
impl galileo_runtime::Projection for Menu {
    type Arguments = (North,);
    fn view(program: &Program) -> &galileo_core::View { program.views.iter().find(|view| view.name == "examples/north-menu/Menu").expect("generated view belongs to this program") }
    fn arguments(arguments: Self::Arguments) -> Vec<Value> { let (north,) = arguments; vec![north.into_value()] }
    fn decode(value: Value) -> Result<Self, EvalError> { Self::from_value(value) }
}
