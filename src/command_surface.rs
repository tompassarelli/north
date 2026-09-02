use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::agent_catalog::ActivationUnit;
use crate::codex::{ModelOption, ReasoningOption};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SlashCommand {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
}

pub(crate) const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/model",
        description: "choose the active model and reasoning effort",
    },
    SlashCommand {
        name: "/effort",
        description: "change reasoning effort for the active model",
    },
    SlashCommand {
        name: "/config",
        description: "toggle the context switchboard",
    },
    SlashCommand {
        name: "/delegate",
        description: "delegate one explicit task",
    },
    SlashCommand {
        name: "/q",
        description: "quit North",
    },
];

pub(crate) fn matching_commands(input: &str) -> Vec<SlashCommand> {
    let input = input.trim();
    if !input.starts_with('/') || input.contains(char::is_whitespace) {
        return Vec::new();
    }
    SLASH_COMMANDS
        .iter()
        .copied()
        .filter(|command| command.name.starts_with(input))
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) enum Picker {
    Switchboard {
        units: Vec<ActivationUnit>,
        index: usize,
    },
    Models {
        models: Vec<ModelOption>,
        index: usize,
    },
    Efforts {
        models: Vec<ModelOption>,
        model: ModelOption,
        model_index: usize,
        standard: Vec<ReasoningOption>,
        advanced: Vec<ReasoningOption>,
        index: usize,
        return_to_models: bool,
    },
    AdvancedEfforts {
        models: Vec<ModelOption>,
        model: ModelOption,
        model_index: usize,
        standard_index: usize,
        options: Vec<ReasoningOption>,
        index: usize,
        return_to_models: bool,
    },
}

impl Picker {
    pub(crate) fn switchboard(units: Vec<ActivationUnit>) -> Self {
        Self::Switchboard { units, index: 0 }
    }

    pub(crate) fn models(models: Vec<ModelOption>, current_model: &str) -> Option<Self> {
        if models.is_empty() {
            return None;
        }
        let index = models
            .iter()
            .position(|model| model.model == current_model)
            .or_else(|| models.iter().position(|model| model.is_default))
            .unwrap_or(0);
        Some(Self::Models { models, index })
    }

    pub(crate) fn efforts(
        models: Vec<ModelOption>,
        model_index: usize,
        current_model: &str,
        current_effort: &str,
        return_to_models: bool,
    ) -> Self {
        let model = models[model_index].clone();
        let (mut standard, advanced): (Vec<_>, Vec<_>) = model
            .reasoning
            .iter()
            .cloned()
            .partition(|option| !is_advanced_effort(&option.effort));
        if standard.is_empty() && advanced.is_empty() {
            standard.push(ReasoningOption {
                effort: model.default_effort.clone(),
                description: String::new(),
            });
        }
        let index = if model.model == current_model {
            standard
                .iter()
                .position(|option| option.effort == current_effort)
                .or_else(|| {
                    (!advanced.is_empty()
                        && advanced
                            .iter()
                            .any(|option| option.effort == current_effort))
                    .then_some(standard.len())
                })
        } else {
            standard
                .iter()
                .position(|option| option.effort == model.default_effort)
        }
        .unwrap_or(0);
        Self::Efforts {
            models,
            model,
            model_index,
            standard,
            advanced,
            index,
            return_to_models,
        }
    }

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let (index, len) = match self {
            Self::Switchboard { units, index } => (index, units.len()),
            Self::Models { models, index } => (index, models.len()),
            Self::Efforts {
                standard,
                advanced,
                index,
                ..
            } => (index, standard.len() + usize::from(!advanced.is_empty())),
            Self::AdvancedEfforts { options, index, .. } => (index, options.len()),
        };
        if len == 0 {
            return;
        }
        *index = ((*index as isize + delta).rem_euclid(len as isize)) as usize;
    }

    pub(crate) fn back(self) -> Option<Self> {
        match self {
            Self::Switchboard { .. } | Self::Models { .. } => None,
            Self::Efforts {
                models,
                model: _,
                model_index,
                return_to_models,
                ..
            } => return_to_models.then_some(Self::Models {
                models,
                index: model_index,
            }),
            Self::AdvancedEfforts {
                models,
                model,
                model_index,
                standard_index,
                options,
                return_to_models,
                ..
            } => {
                let standard = model
                    .reasoning
                    .iter()
                    .filter(|option| !is_advanced_effort(&option.effort))
                    .cloned()
                    .collect();
                Some(Self::Efforts {
                    models,
                    model,
                    model_index,
                    standard,
                    advanced: options,
                    index: standard_index,
                    return_to_models,
                })
            }
        }
    }
}

pub(crate) fn is_advanced_effort(effort: &str) -> bool {
    matches!(effort, "max" | "ultra")
}

pub(crate) fn effort_label(effort: &str) -> String {
    match effort {
        "none" => "None".into(),
        "minimal" => "Minimal".into(),
        "low" => "Low".into(),
        "medium" => "Medium".into(),
        "high" => "High".into(),
        "xhigh" => "Extra high".into(),
        "max" => "Max".into(),
        "ultra" => "Ultra".into(),
        other => other.to_owned(),
    }
}

pub(crate) fn render_slash_menu(
    frame: &mut Frame<'_>,
    composer: Rect,
    input: &str,
    selected: usize,
) {
    let commands = matching_commands(input);
    if commands.is_empty() || composer.y == 0 {
        return;
    }
    let height = (commands.len() as u16 + 2).min(composer.y);
    let width = composer.width.min(64).max(1);
    let area = Rect::new(
        composer.x.saturating_add(2),
        composer.y.saturating_sub(height),
        width.saturating_sub(2),
        height,
    );
    let lines = commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            let style = if index == selected.min(commands.len() - 1) {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(vec![
                Span::styled(format!("  {:<12}", command.name), style),
                Span::styled(command.description, style),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .style(Style::default().bg(Color::Rgb(24, 26, 31))),
        ),
        area,
    );
}

pub(crate) fn render_picker(
    frame: &mut Frame<'_>,
    area: Rect,
    picker: &Picker,
    current_model: &str,
    current_effort: &str,
) {
    let lines = match picker {
        Picker::Switchboard { units, index } => switchboard_lines(units, *index, area.height),
        Picker::Models { models, index } => {
            let mut lines = picker_header(
                "Select Model and Effort",
                Some("Choose a model, then choose its reasoning level"),
            );
            for (at, model) in models.iter().enumerate() {
                lines.extend(selection_lines(
                    at,
                    *index,
                    &model.model,
                    &model.description,
                    (model.model == current_model)
                        .then_some("current")
                        .or_else(|| model.is_default.then_some("default")),
                    area.width,
                ));
            }
            lines.extend(picker_footer());
            lines
        }
        Picker::Efforts {
            model,
            standard,
            advanced,
            index,
            ..
        } => {
            let mut lines =
                picker_header(&format!("Select Reasoning Level for {}", model.model), None);
            for (at, option) in standard.iter().enumerate() {
                let marker = (model.model == current_model && option.effort == current_effort)
                    .then_some("current")
                    .or_else(|| (option.effort == model.default_effort).then_some("default"));
                lines.extend(selection_lines(
                    at,
                    *index,
                    &effort_label(&option.effort),
                    &option.description,
                    marker,
                    area.width,
                ));
            }
            if !advanced.is_empty() {
                let names = advanced
                    .iter()
                    .map(|option| effort_label(&option.effort))
                    .collect::<Vec<_>>()
                    .join(" and ");
                let verb = if advanced.len() == 1 {
                    "consumes"
                } else {
                    "consume"
                };
                lines.extend(selection_lines(
                    standard.len(),
                    *index,
                    "More reasoning…",
                    &format!("{names} {verb} usage limits faster"),
                    (model.model == current_model
                        && advanced
                            .iter()
                            .any(|option| option.effort == current_effort))
                    .then_some("current"),
                    area.width,
                ));
            }
            lines.extend(picker_footer());
            lines
        }
        Picker::AdvancedEfforts {
            model,
            options,
            index,
            ..
        } => {
            let mut lines =
                picker_header("Advanced Reasoning", Some("⚠ Consumes usage limits faster"));
            for (at, option) in options.iter().enumerate() {
                lines.extend(selection_lines(
                    at,
                    *index,
                    &effort_label(&option.effort),
                    &option.description,
                    (model.model == current_model && option.effort == current_effort)
                        .then_some("current"),
                    area.width,
                ));
            }
            lines.extend(picker_footer());
            lines
        }
    };
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}

fn picker_header<'a>(title: &str, subtitle: Option<&str>) -> Vec<Line<'a>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("  {title}"),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if let Some(subtitle) = subtitle {
        lines.push(Line::from(Span::styled(
            format!("  {subtitle}"),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::default());
    lines
}

fn picker_footer<'a>() -> Vec<Line<'a>> {
    vec![
        Line::default(),
        Line::from(Span::styled(
            "  Press enter to confirm or esc to go back",
            Style::default().fg(Color::DarkGray),
        )),
    ]
}

fn selection_lines<'a>(
    at: usize,
    selected: usize,
    name: &str,
    description: &str,
    marker: Option<&str>,
    width: u16,
) -> Vec<Line<'a>> {
    let style = if at == selected {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let label = marker.map_or_else(|| name.to_owned(), |marker| format!("{name} ({marker})"));
    let leader = format!("{}{}. ", if at == selected { "› " } else { "  " }, at + 1);
    let description_column = leader.chars().count() + 28;
    let description_width = usize::from(width)
        .saturating_sub(description_column)
        .max(12);
    let wrapped = wrap_words(description, description_width);
    let first_description = wrapped.first().cloned().unwrap_or_default();
    let mut lines = vec![Line::from(vec![
        Span::styled(leader, style),
        Span::styled(format!("{label:<28}"), style),
        Span::styled(first_description, style),
    ])];
    lines.extend(wrapped.into_iter().skip(1).map(|continuation| {
        Line::from(Span::styled(
            format!("{}{continuation}", " ".repeat(description_column)),
            style,
        ))
    }));
    lines
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line.push_str(word);
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(line);
            line = word.to_owned();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn switchboard_lines<'a>(units: &[ActivationUnit], selected: usize, height: u16) -> Vec<Line<'a>> {
    let mut all = picker_header("Switchboard", Some("↑/↓ move · space toggle · esc close"));
    let mut selected_line = 0;
    let mut previous_kind = "";
    for (index, unit) in units.iter().enumerate() {
        if unit.kind != previous_kind {
            all.push(Line::from(Span::styled(
                format!("  {}", unit.kind.to_uppercase()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            previous_kind = &unit.kind;
        }
        if index == selected {
            selected_line = all.len();
        }
        let style = if index == selected {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else if unit.active {
            Style::default().fg(Color::Gray)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        all.push(Line::from(vec![
            Span::styled(if index == selected { "›   " } else { "    " }, style),
            Span::styled(format!("{}: ", unit.id), style),
            Span::styled(if unit.active { "on" } else { "off" }, style),
            Span::styled(
                if unit.detail.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", unit.detail)
                },
                style,
            ),
        ]));
    }
    let visible = usize::from(height.max(1));
    if all.len() <= visible {
        return all;
    }
    let start = selected_line
        .saturating_sub(visible / 2)
        .min(all.len() - visible);
    all.into_iter().skip(start).take(visible).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_palette_filters_without_swallowing_command_arguments() {
        assert_eq!(
            matching_commands("/m")
                .into_iter()
                .map(|command| command.name)
                .collect::<Vec<_>>(),
            vec!["/model"]
        );
        assert!(matching_commands("/delegate task").is_empty());
    }

    #[test]
    fn model_picker_routes_advanced_efforts_through_more_reasoning() {
        let model = ModelOption {
            model: "gpt-example".into(),
            description: "Example".into(),
            reasoning: vec![
                ReasoningOption {
                    effort: "low".into(),
                    description: "Fast".into(),
                },
                ReasoningOption {
                    effort: "max".into(),
                    description: "Deep".into(),
                },
            ],
            default_effort: "low".into(),
            is_default: true,
        };
        let Picker::Efforts {
            standard, advanced, ..
        } = Picker::efforts(vec![model], 0, "gpt-example", "max", true)
        else {
            panic!("expected effort picker");
        };
        assert_eq!(standard[0].effort, "low");
        assert_eq!(advanced[0].effort, "max");
    }

    #[test]
    fn effort_fast_path_escapes_to_chat_instead_of_models() {
        let model = ModelOption {
            model: "gpt-example".into(),
            description: String::new(),
            reasoning: vec![ReasoningOption {
                effort: "low".into(),
                description: String::new(),
            }],
            default_effort: "low".into(),
            is_default: true,
        };
        let picker = Picker::efforts(vec![model], 0, "gpt-example", "low", false);

        assert!(picker.back().is_none());
    }

    #[test]
    fn picker_descriptions_wrap_under_the_description_column() {
        let lines = selection_lines(
            0,
            0,
            "gpt-example",
            "Balanced reasoning depth for ordinary everyday work",
            Some("current"),
            60,
        );

        assert!(lines.len() > 1);
        assert!(lines[1].to_string().starts_with(&" ".repeat(33)));
        assert!(!lines[1].to_string().trim().is_empty());
    }
}
