use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuRow {
    pub key: String,
    pub label: String,
    pub description: String,
    pub annotation: String,
    pub position: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Menu {
    pub kind: String,
    pub title: String,
    pub query: String,
    pub help: String,
    pub empty: String,
    pub selection: i64,
    pub rows: Arc<[MenuRow]>,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            kind: String::new(), title: String::new(), query: String::new(),
            help: help("").into(), empty: "No matching conversations".into(),
            selection: 0, rows: Arc::from([]),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MenuEvent<'a> {
    ClearRows,
    OpenHistory { kind: &'a str },
    OpenSettings { kind: &'a str, title: &'a str },
    OfferRow { key: &'a str, label: &'a str, description: &'a str, annotation: &'a str },
    Query(&'a str),
    Move(i64),
    Close,
    Accept,
    ClearEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuError {
    NoMatchingMove,
    ConflictingEffects,
    MissingCount,
}

impl fmt::Display for MenuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NoMatchingMove => "Menu move has no matching clause",
            Self::ConflictingEffects => "Menu selection requests conflicting effects",
            Self::MissingCount => "Menu kind has no derived row count",
        })
    }
}

impl std::error::Error for MenuError {}

#[derive(Clone, Debug)]
struct OfferedRow {
    row: MenuRow,
    order: i64,
}

#[derive(Clone, Debug, Default)]
pub struct MenuSession {
    menu: Menu,
    offered: Vec<OfferedRow>,
    row_count: i64,
    active_conversation: String,
    effect: Option<(String, String)>,
    revision: u64,
    effects: Vec<(String, String)>,
}

impl MenuSession {
    pub fn menu(&self) -> &Menu { &self.menu }
    pub fn revision(&self) -> u64 { self.revision }
    pub fn effects(&self) -> &[(String, String)] { &self.effects }
    pub fn effect(&self) -> Option<(&str, &str)> {
        self.effect.as_ref().map(|(effect, payload)| (effect.as_str(), payload.as_str()))
    }

    pub fn apply(&mut self, event: MenuEvent<'_>) -> Result<(), MenuError> {
        self.effects.clear();
        self.step(event)?;
        self.revision += 1;
        Ok(())
    }

    pub fn run_all(&mut self, events: &[MenuEvent<'_>]) -> Result<(), MenuError> {
        self.effects.clear();
        let previous = self.clone();
        for &event in events {
            if let Err(error) = self.step(event) {
                *self = previous;
                return Err(error);
            }
        }
        self.effects.sort();
        self.effects.dedup();
        self.revision += 1;
        Ok(())
    }

    fn step(&mut self, event: MenuEvent<'_>) -> Result<(), MenuError> {
        let rows_changed = matches!(event, MenuEvent::ClearRows | MenuEvent::OfferRow { .. } | MenuEvent::Query(_));
        match event {
            MenuEvent::ClearRows => self.offered.clear(),
            MenuEvent::OpenHistory { kind } => {
                if kind == "usage" { return Err(MenuError::MissingCount); }
                self.menu.kind = kind.into();
                self.menu.title = if kind == "archived-history" { "Archived Conversations" } else { "Resume Conversation" }.into();
                self.row_count = 0;
            }
            MenuEvent::OpenSettings { kind, title } => {
                if kind == "usage" { return Err(MenuError::MissingCount); }
                self.menu.kind = kind.into(); self.menu.title = title.into();
                self.row_count = 0;
                self.menu.help = help(kind).into();
                self.menu.empty = if kind == "work" { "No matching work items" } else { "No matching entries" }.into();
            }
            MenuEvent::OfferRow { key, label, description, annotation } => {
                let annotation = if matches!(self.menu.kind.as_str(), "history" | "archived-history") && key == self.active_conversation { "current" } else { annotation };
                self.offered.push(OfferedRow {
                    row: MenuRow { key: key.into(), label: label.into(), description: description.into(), annotation: annotation.into(), position: 0 },
                    order: self.row_count,
                });
                self.row_count += 1;
            }
            MenuEvent::Query(query) => { self.menu.query = query.into(); self.menu.selection = 0; }
            MenuEvent::Move(delta) => {
                if self.menu.kind == "usage" || self.menu.rows.is_empty() { return Err(MenuError::NoMatchingMove); }
                let next = self.menu.selection + delta;
                self.menu.selection = if next < 0 { self.menu.rows.len() as i64 - 1 } else if next >= self.menu.rows.len() as i64 { 0 } else { next };
            }
            MenuEvent::Close => self.menu.kind.clear(),
            MenuEvent::Accept => {
                if self.effect.is_none() {
                    let action = match self.menu.kind.as_str() {
                        "work" => Some("work-show"), "history" => Some("switch-conversation"),
                        "archived-history" => Some("restore-conversation"), "models" => Some("select-model"),
                        "efforts" => Some("select-effort"), "config" => Some("toggle-config"), _ => None,
                    };
                    if let Some(action) = action {
                        let mut selected = self.menu.rows.iter().filter(|row| row.position == self.menu.selection);
                        if let Some(row) = selected.next() {
                            if selected.any(|other| other.key != row.key) { return Err(MenuError::ConflictingEffects); }
                            self.effects.push((action.into(), row.key.clone()));
                            self.effect = Some((action.into(), row.key.clone()));
                            if self.menu.kind != "work" { self.menu.kind.clear(); }
                        }
                    }
                }
            }
            MenuEvent::ClearEffect => self.effect = None,
        }
        if rows_changed { self.project(); }
        Ok(())
    }

    fn project(&mut self) {
        let query = self.menu.query.to_lowercase();
        let mut visible = self.offered.iter().filter(|offered| {
            let row = &offered.row;
            format!("{} {} {}", row.label, row.description, row.key).to_lowercase().contains(&query)
        }).collect::<Vec<_>>();
        visible.sort_by_key(|offered| offered.order);
        let mut position = 0;
        let mut rows = visible.iter().enumerate().map(|(index, offered)| {
            if index > 0 && visible[index - 1].order < offered.order { position = index as i64; }
            let mut row = offered.row.clone(); row.position = position; row
        }).collect::<Vec<_>>();
        rows.sort_by(|left, right| (left.position, &left.annotation, &left.description, &left.key, &left.label).cmp(&(right.position, &right.annotation, &right.description, &right.key, &right.label)));
        self.menu.rows = rows.into();
    }

    pub fn open_history(&mut self, conversations: &[(&str, &str, &str)], query: &str, archived: bool) -> Result<(), MenuError> {
        let mut events = vec![MenuEvent::ClearRows, MenuEvent::OpenHistory { kind: if archived { "archived-history" } else { "history" } }];
        events.extend(conversations.iter().map(|&(key, label, description)| MenuEvent::OfferRow { key, label, description, annotation: "" }));
        events.push(MenuEvent::Query(query));
        self.run_all(&events)
    }

    pub fn open_settings_menu(&mut self, kind: &str, title: &str, rows: &[(&str, &str, &str, &str)]) -> Result<(), MenuError> {
        self.settings_menu(kind, title, rows, false)
    }

    pub fn finish_settings_effect(&mut self, kind: &str, title: &str, rows: &[(&str, &str, &str, &str)]) -> Result<(), MenuError> {
        self.settings_menu(kind, title, rows, true)
    }

    fn settings_menu(&mut self, kind: &str, title: &str, rows: &[(&str, &str, &str, &str)], finish_effect: bool) -> Result<(), MenuError> {
        let mut events = vec![MenuEvent::ClearRows, MenuEvent::OpenSettings { kind, title }];
        events.extend(rows.iter().map(|&(key, label, description, annotation)| MenuEvent::OfferRow { key, label, description, annotation }));
        events.push(MenuEvent::Query(""));
        if finish_effect { events.push(MenuEvent::ClearEffect); }
        self.run_all(&events)
    }

    pub fn work_menu(&mut self, rows: &[(&str, &str, &str, &str, &str, &str, bool)]) -> Result<(), MenuError> {
        let descriptions = rows.iter().map(|(_, _, _, holder, clock, eta, _)| {
            if holder.is_empty() { "No holder".to_owned() } else { format!("{holder} · {clock} held / {eta} ETA") }
        }).collect::<Vec<_>>();
        let labels = rows.iter().map(|(reference, title, ..)| format!("{reference}  {title}")).collect::<Vec<_>>();
        let offered = rows.iter().enumerate().map(|(index, (reference, _, state, holder, _, _, blocked))| {
            let annotation = if *state == "closed" { "closed" } else if *blocked { "blocked" } else if holder.is_empty() { "ready" } else { "held" };
            (*reference, labels[index].as_str(), descriptions[index].as_str(), annotation)
        }).collect::<Vec<_>>();
        self.open_settings_menu("work", "Work items", &offered)
    }

    pub fn query_menu(&mut self, query: &str) -> Result<(), MenuError> { self.apply(MenuEvent::Query(query)) }
    pub fn move_menu(&mut self, delta: isize) -> Result<(), MenuError> { self.apply(MenuEvent::Move(delta as i64)) }
    pub fn close_menu(&mut self) -> Result<(), MenuError> { self.apply(MenuEvent::Close) }
    pub fn accept_menu(&mut self) -> Result<(), MenuError> { self.apply(MenuEvent::Accept) }
}

fn help(kind: &str) -> &'static str {
    match kind {
        "document" => "↑/↓ scroll · Esc close · /work claim, release, need, unneed or close to edit",
        "work" => "Type to filter · ↑/↓ choose · Enter open · Esc close · /work search TEXT includes closed issues",
        _ => "Type to search · ↑/↓ choose · Enter open · Esc close",
    }
}

#[cfg(test)]
#[path = "fixtures/menu_replay.rs"]
mod replay;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_north_menu_reference_replay() {
        let mut session = MenuSession::default();
        let mut actual = String::new();
        for batch in replay::corpus() {
            let result = session.run_all(&batch);
            let menu = session.menu();
            actual.push_str(&format!("{:?}\n", (result.is_ok(), session.revision(),
                (&menu.kind, &menu.title, &menu.query, &menu.help, &menu.empty, menu.selection),
                menu.rows.iter().map(|row| (&row.key, &row.label, &row.description, &row.annotation, row.position)).collect::<Vec<_>>(),
                session.effect(), session.effects())));
        }
        assert_eq!(actual, include_str!("fixtures/menu_replay.txt"));
    }
}
