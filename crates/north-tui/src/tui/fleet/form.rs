use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Modifier, Style};
use tui_textarea::TextArea;

use crate::fleet::model::{Account, Effort, LaunchRequest, ProviderName};
use crate::fleet::prompt::PromptMode;

use super::desk::AccountRoom;
use super::editor::edit_textarea;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    Provider,
    Account,
    Name,
    Directory,
    Model,
    Effort,
    Task,
    Box,
    Prompt,
}

impl Field {
    pub const ORDER: [Field; 9] = [Field::Provider, Field::Account, Field::Name, Field::Directory, Field::Model, Field::Effort, Field::Task, Field::Box, Field::Prompt];

    pub fn label(self) -> &'static str {
        match self {
            Field::Provider => "provider",
            Field::Account => "account",
            Field::Name => "name",
            Field::Directory => "directory",
            Field::Model => "model",
            Field::Effort => "effort",
            Field::Task => "task",
            Field::Box => "box",
            Field::Prompt => "prompt",
        }
    }

    fn step(self, forward: bool) -> Field {
        let index = Field::ORDER.iter().position(|field| *field == self).unwrap_or(0);
        let count = Field::ORDER.len();
        Field::ORDER[if forward { (index + 1) % count } else { (index + count - 1) % count }]
    }
}

pub enum FormOutcome {
    Editing,
    Cancelled,
    Launch(Box<LaunchRequest>),
}

pub struct LaunchForm {
    pub provider: ProviderName,
    pub accounts: Vec<Account>,
    pub account: usize,
    pub rooms: Vec<AccountRoom>,
    pub effort: Option<Effort>,
    pub field: Field,
    pub name: TextArea<'static>,
    pub directory: TextArea<'static>,
    pub model: TextArea<'static>,
    pub task: TextArea<'static>,
    pub claim_box: TextArea<'static>,
    pub prompt: TextArea<'static>,
    pub problem: Option<String>,
    parent: Option<String>,
}

impl LaunchForm {
    pub fn new(accounts: Vec<Account>, cwd: &Path, taken: &[String], parent: Option<String>) -> Self {
        let provider = accounts.first().map_or_else(|| ProviderName::new(""), |account| account.provider.clone());
        Self {
            provider,
            accounts,
            account: 0,
            rooms: Vec::new(),
            effort: None,
            field: Field::ORDER[0],
            name: line(&unique_name(cwd, taken), ""),
            directory: line(&cwd.display().to_string(), ""),
            model: line("", "default"),
            task: line("", "REPO#N, none by default"),
            claim_box: line("", "the box text, when the issue has more than one open box"),
            prompt: prompt_area(),
            problem: None,
            parent: parent.filter(|parent| !parent.is_empty()),
        }
    }

    pub fn provider_accounts(&self) -> Vec<&Account> {
        self.accounts.iter().filter(|account| account.provider == self.provider).collect()
    }

    pub fn chosen_account(&self) -> Option<&Account> {
        self.provider_accounts().get(self.account).copied().filter(|account| !self.is_out(account))
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> FormOutcome {
        match key.code {
            KeyCode::Esc => return FormOutcome::Cancelled,
            KeyCode::Tab => self.field = self.field.step(true),
            KeyCode::BackTab => self.field = self.field.step(false),
            KeyCode::Enter if self.field == Field::Prompt && newline_modifier(key) => self.prompt.insert_newline(),
            KeyCode::Enter => return self.submit(),
            _ => self.edit(key),
        }
        FormOutcome::Editing
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(area) = self.text_field() {
            area.insert_str(text);
        }
    }

    pub fn text_area(&self, field: Field) -> Option<&TextArea<'static>> {
        match field {
            Field::Name => Some(&self.name),
            Field::Directory => Some(&self.directory),
            Field::Model => Some(&self.model),
            Field::Task => Some(&self.task),
            Field::Box => Some(&self.claim_box),
            Field::Prompt => Some(&self.prompt),
            Field::Provider | Field::Account | Field::Effort => None,
        }
    }

    fn text_field(&mut self) -> Option<&mut TextArea<'static>> {
        match self.field {
            Field::Name => Some(&mut self.name),
            Field::Directory => Some(&mut self.directory),
            Field::Model => Some(&mut self.model),
            Field::Task => Some(&mut self.task),
            Field::Box => Some(&mut self.claim_box),
            Field::Prompt => Some(&mut self.prompt),
            Field::Provider | Field::Account | Field::Effort => None,
        }
    }

    fn edit(&mut self, key: KeyEvent) {
        let forward = matches!(key.code, KeyCode::Right | KeyCode::Char(' ') | KeyCode::Down);
        let backward = matches!(key.code, KeyCode::Left | KeyCode::Up);
        match self.field {
            Field::Provider if forward || backward => self.step_provider(forward),
            Field::Account if forward || backward => self.cycle_account(forward),
            Field::Effort if forward || backward => self.step_effort(forward),
            Field::Provider | Field::Account | Field::Effort => {}
            _ => {
                if let Some(area) = self.text_field() {
                    edit_textarea(area, key);
                }
            }
        }
    }

    fn step_provider(&mut self, forward: bool) {
        let mut providers: Vec<&ProviderName> = Vec::new();
        for account in &self.accounts {
            if !providers.contains(&&account.provider) {
                providers.push(&account.provider);
            }
        }
        let count = providers.len().max(1);
        let current = providers.iter().position(|provider| **provider == self.provider).unwrap_or(0);
        let next = if forward { (current + 1) % count } else { (current + count - 1) % count };
        if let Some(next) = providers.get(next) {
            self.provider = (*next).clone();
        }
        self.account = self.first_offered();
    }

    fn step_effort(&mut self, forward: bool) {
        let choices: Vec<Option<Effort>> = std::iter::once(None).chain(Effort::ALL.map(Some)).collect();
        let current = choices.iter().position(|choice| *choice == self.effort).unwrap_or(0);
        let count = choices.len();
        self.effort = choices[if forward { (current + 1) % count } else { (current + count - 1) % count }];
    }

    fn cycle_account(&mut self, forward: bool) {
        let count = self.provider_accounts().len();
        for step in 1..count {
            let next = if forward { (self.account + step) % count } else { (self.account + count - step) % count };
            if self.provider_accounts().get(next).is_some_and(|account| !self.is_out(account)) {
                self.account = next;
                return;
            }
        }
    }

    fn first_offered(&self) -> usize {
        self.provider_accounts().iter().position(|account| !self.is_out(account)).unwrap_or(0)
    }

    pub fn is_out(&self, account: &Account) -> bool {
        self.week_left(account).is_some_and(|left| left <= 0.0)
    }

    pub fn week_left(&self, account: &Account) -> Option<f64> {
        self.rooms.iter().find(|room| room.label == account.label).and_then(|room| room.week_left)
    }

    pub fn with_rooms(mut self, rooms: &[AccountRoom]) -> Self {
        let rank = |account: &Account| rooms.iter().position(|room| room.label == account.label).unwrap_or(rooms.len());
        self.accounts.sort_by_key(rank);
        self.rooms = rooms.to_vec();
        if let Some(first) = self.accounts.first() {
            self.provider = first.provider.clone();
        }
        self.account = self.first_offered();
        self
    }

    fn submit(&mut self) -> FormOutcome {
        match self.request() {
            Ok(request) => FormOutcome::Launch(Box::new(request)),
            Err(problem) => {
                self.problem = Some(problem);
                FormOutcome::Editing
            }
        }
    }

    fn request(&self) -> Result<LaunchRequest, String> {
        let account = self.chosen_account().ok_or_else(|| {
            let label = self.provider.label();
            if self.provider_accounts().is_empty() { format!("no {label} account is configured") } else { format!("every {label} account has nothing left of its week") }
        })?;
        let name = text(&self.name);
        let directory = text(&self.directory);
        let prompt = self.prompt.lines().join("\n");
        if name.is_empty() {
            return Err("name is empty".into());
        }
        if directory.is_empty() {
            return Err("directory is empty".into());
        }
        if prompt.trim().is_empty() {
            return Err("prompt is empty".into());
        }
        let model = text(&self.model);
        let task = text(&self.task);
        let claim_box = text(&self.claim_box);
        Ok(LaunchRequest {
            verification: false,
            gates_aside: false,
            urgent: false,
            container_shell: false,
            container_dir: None,
            name,
            provider: self.provider.clone(),
            account: account.label.clone(),
            cwd: PathBuf::from(directory),
            model: (!model.is_empty()).then_some(model),
            effort: self.effort,
            prompt,
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: self.parent.clone(),
            task_ref: (!task.is_empty()).then_some(task),
            claim_box: (!claim_box.is_empty()).then_some(claim_box),
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
            dispatched: false,
            surface: Default::default(),
            route: None,
        })
    }
}

pub fn newline_modifier(key: KeyEvent) -> bool {
    key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
}

fn text(area: &TextArea<'static>) -> String {
    area.lines().join("").trim().to_owned()
}

fn line(value: &str, placeholder: &str) -> TextArea<'static> {
    let mut area = TextArea::new(vec![value.to_owned()]);
    area.move_cursor(tui_textarea::CursorMove::End);
    area.set_placeholder_text(placeholder);
    area.set_cursor_line_style(Style::default());
    area.set_cursor_style(Style::default().add_modifier(Modifier::REVERSED));
    area
}

fn prompt_area() -> TextArea<'static> {
    let mut area = line("", "what should the agent do?");
    area.set_wrap_mode(tui_textarea::WrapMode::WordOrGlyph);
    area
}

pub fn unique_name(cwd: &Path, taken: &[String]) -> String {
    let base: String = cwd
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() { character } else { '-' })
        .collect();
    let base = if base.trim_matches('-').is_empty() { "agent".to_owned() } else { base.trim_matches('-').to_owned() };
    (1..)
        .map(|index| if index == 1 { base.clone() } else { format!("{base}-{index}") })
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or(base)
}
