//! The composer's submit path: a gate answer, a tuning request, or a message to the selected session.

use crate::fleet::gates::{Decision, Gate};
use crate::fleet::model::{Delivery, Effort};

use super::super::help_panel::Tab;
use super::super::slash;
use super::{composer_area, Command, Notice, Screen};

const NO_SESSION: &str = "no session selected; Ctrl+N starts one";

impl Screen {
    pub fn submit(&mut self, delivery: Delivery) -> Option<Command> {
        let text = self.composer.lines().join("\n");
        if text.trim().is_empty() {
            return None;
        }
        if self.digest.shown
            && let Some(gate) = self.digest.selected()
        {
            let id = gate.id;
            let option = chosen_option(gate, &text);
            return self.answer_gate(id, option, text);
        }
        if let Some(command) = Slash::parse(&text) {
            self.composer = composer_area();
            return self.handle_slash(command);
        }
        let Some(session) = self.selected_session() else {
            self.notice = Some(Notice::Error(NO_SESSION.into()));
            return None;
        };
        let name = session.record.name.clone();
        let text = self.attachments.expand(&text);
        self.composer = composer_area();
        self.scroll = 0;
        Some(Command::Send { name, text, delivery, deepen: false })
    }

    fn handle_slash(&mut self, command: Slash<'_>) -> Option<Command> {
        match command {
            Slash::Help => {
                self.composer.insert_str("?");
                self.help.open(Tab::General);
                None
            }
            Slash::Tuning(tuning) => self.tune(tuning),
            Slash::Unknown(word) => {
                let commands: Vec<&str> = slash::COMMANDS.iter().map(|(name, _)| *name).collect();
                self.notice = Some(Notice::Error(format!("{word} is not a command here; the commands are {}", commands.join(", "))));
                None
            }
        }
    }

    fn answer_gate(&mut self, id: i64, option: Option<usize>, text: String) -> Option<Command> {
        if let Some(number) = option {
            self.held_answer = None;
            self.composer = composer_area();
            return self.decide(Decision::Option(number));
        }
        if !self.held_answer.as_ref().is_some_and(|(held, typed)| *held == id && *typed == text) {
            self.held_answer = Some((id, text));
            return None;
        }
        self.held_answer = None;
        self.composer = composer_area();
        self.decide(Decision::Text(text))
    }

    pub fn confirmation(&self) -> Option<String> {
        if !self.digest.shown {
            return None;
        }
        let gate = self.digest.selected()?;
        let (held, text) = self.held_answer.as_ref()?;
        let typed = self.composer.lines().join("\n");
        (*held == gate.id && *text == typed).then(|| format!("Enter sends · Esc drops · gate {}: {} ← {:?}", gate.id, gate.request.question, text))
    }

    fn tune(&mut self, tuning: Tuning<'_>) -> Option<Command> {
        let Some(session) = self.selected_session() else {
            self.notice = Some(Notice::Error(NO_SESSION.into()));
            return None;
        };
        let record = session.record.clone();
        let choices = self.choices.iter().find(|choices| choices.provider == record.provider);
        let name = record.name.clone();
        match tuning {
            Tuning::Model(Some(model)) => return Some(Command::Relaunch { name, model: Some(model.to_owned()), effort: None }),
            Tuning::Effort(Some(level)) => match Effort::parse(level) {
                Ok(effort) => return Some(Command::Relaunch { name, model: None, effort: Some(effort) }),
                Err(error) => self.notice = Some(Notice::Error(error.user_message())),
            },
            Tuning::Model(None) => {
                let models = choices.map_or_else(String::new, |choices| choices.models.join(", "));
                self.notice = Some(Notice::Info(format!("{name} runs {}; /model NAME restarts it with one of: {models}", record.model.as_deref().unwrap_or("the account default model"))));
            }
            Tuning::Effort(None) if choices.is_some_and(|choices| !choices.takes_effort) => {
                self.notice = Some(Notice::Info(format!("{} sessions take no effort", record.provider.label())));
            }
            Tuning::Effort(None) => {
                let levels = Effort::ALL.map(Effort::label).join(", ");
                self.notice = Some(Notice::Info(format!("{name} runs at {} effort; /effort LEVEL restarts it with one of: {levels}", record.effort.map_or("the account default", Effort::label))));
            }
        }
        None
    }

    pub fn deepen(&mut self) -> Option<Command> {
        let mut command = self.submit(Delivery::Queued)?;
        if let Command::Send { deepen, .. } = &mut command {
            *deepen = true;
        }
        Some(command)
    }
}

fn chosen_option(gate: &Gate, text: &str) -> Option<usize> {
    let text = text.trim();
    if let Ok(number) = text.parse::<usize>()
        && gate.option(number).is_some()
    {
        return Some(number);
    }
    gate.request.options.iter().position(|option| option.trim().eq_ignore_ascii_case(text)).map(|index| index + 1)
}

enum Slash<'a> {
    Help,
    Tuning(Tuning<'a>),
    Unknown(&'a str),
}

impl<'a> Slash<'a> {
    fn parse(text: &'a str) -> Option<Self> {
        let text = text.trim();
        let (word, argument) = text.split_once(char::is_whitespace).map_or((text, None), |(word, rest)| (word, Some(rest.trim()).filter(|rest| !rest.is_empty())));
        if !is_command_word(word) {
            return None;
        }
        Some(match word {
            "/help" => Self::Help,
            "/model" => Self::Tuning(Tuning::Model(argument)),
            "/effort" => Self::Tuning(Tuning::Effort(argument)),
            _ => Self::Unknown(word),
        })
    }
}

fn is_command_word(word: &str) -> bool {
    word.strip_prefix('/').is_some_and(|name| name.chars().all(|c| c.is_ascii_alphabetic() || c == '-'))
}

enum Tuning<'a> {
    Model(Option<&'a str>),
    Effort(Option<&'a str>),
}
