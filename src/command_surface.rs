use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::agent_catalog::ActivationUnit;
use crate::clause_state::CommandSpec;
use crate::codex::{ModelOption, ReasoningOption};

pub(crate) fn menu_direction(key: &crossterm::event::KeyEvent) -> Option<isize> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match key.code {
        KeyCode::Up => Some(-1),
        KeyCode::Down => Some(1),
        KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(-1),
        KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => Some(1),
        _ => None,
    }
}

fn clipped(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else if width == 0 {
        String::new()
    } else {
        text.chars().take(width - 1).chain(['…']).collect()
    }
}

pub(crate) fn render_reference_menu(
    frame: &mut Frame<'_>,
    composer: Rect,
    matches: &[crate::references::Reference],
    selected: usize,
) {
    let height = (matches.len().max(1) as u16 + 4).min(14).min(composer.y);
    if height < 4 || composer.width < 20 {
        return;
    }
    let area = Rect::new(composer.x, composer.y - height, composer.width, height);
    let inner_width = usize::from(area.width.saturating_sub(2));
    let name_width = matches
        .iter()
        .map(|unit| unit.name.len())
        .max()
        .unwrap_or(4)
        .min(inner_width * 2 / 5)
        .max(4);
    let description_width = inner_width.saturating_sub(name_width + 11);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "  {:<name_width$}  {:<description_width$}  Type",
            "Name",
            clipped("Description", description_width)
        ),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    let visible = usize::from(height - 4);
    let selected = selected.min(matches.len().saturating_sub(1));
    let start = selected
        .saturating_sub(visible / 2)
        .min(matches.len().saturating_sub(visible));
    if matches.is_empty() {
        lines.push(Line::from("  No matching files, skills, or hooks"));
    } else {
        for (index, unit) in matches.iter().enumerate().skip(start).take(visible) {
            let style = if index == selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            lines.push(Line::from(Span::styled(
                format!(
                    "{} {:<name_width$}  {:<description_width$}  {}",
                    if index == selected { "›" } else { " " },
                    clipped(&unit.name, name_width),
                    clipped(&unit.description.replace('\n', " "), description_width),
                    match unit.kind.as_str() { "file" => "File", "skill" => "Skill", "hook" => "Hook", _ => "" },
                ),
                style,
            )));
        }
    }
    lines.push(Line::from(Span::styled(
        clipped(
            "  ↑/↓ or ctrl+j/k · enter/tab insert · esc close",
            inner_width,
        ),
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Rgb(24, 26, 31))),
        ),
        area,
    );
}

pub(crate) fn matching_commands<'a>(
    commands: &'a [CommandSpec],
    input: &str,
) -> Vec<&'a CommandSpec> {
    let input = input.trim();
    if !input.starts_with('/') || input.contains(char::is_whitespace) {
        return Vec::new();
    }
    commands
        .iter()
        .filter(|command| command.name().starts_with(input))
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
    catalog: &[CommandSpec],
    input: &str,
    selected: usize,
) {
    let commands = matching_commands(catalog, input);
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
                Span::styled(format!("  {:<12}", command.name()), style),
                Span::styled(command.description(), style),
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

pub(crate) fn render_menu(
    frame: &mut Frame<'_>, area: Rect, menu: &crate::clause_state::MenuState,
    editor: &tui_textarea::TextArea<'_>,
) {
    let rows = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(1),
        ratatui::layout::Constraint::Length(1),
    ]).split(area);
    frame.render_widget(Paragraph::new(menu.title.as_str()).style(Style::default().add_modifier(Modifier::BOLD)), rows[0]);
    frame.render_widget(editor, rows[1]);
    let visible = usize::from(rows[2].height / 2).max(1);
    let start = menu.selection.saturating_sub(visible / 2).min(menu.rows.len().saturating_sub(visible));
    let mut lines = Vec::new();
    for row in menu.rows.iter().skip(start).take(visible) {
        let selected = row.position == menu.selection;
        let style = Style::default().fg(if selected { Color::Green } else { Color::Gray });
        lines.push(Line::styled(format!("{} {} {}", if selected { "›" } else { " " }, clipped(&row.label, usize::from(area.width.saturating_sub(row.annotation.len() as u16 + 3))), row.annotation), style));
        lines.push(Line::styled(format!("  {}", clipped(&row.description.replace('\n', " "), usize::from(area.width.saturating_sub(2)))), Style::default().fg(Color::DarkGray)));
    }
    if menu.rows.is_empty() { lines.push(Line::from(menu.empty.as_str())); }
    frame.render_widget(Paragraph::new(lines), rows[2]);
    frame.render_widget(Paragraph::new(menu.help.as_str()).style(Style::default().fg(Color::DarkGray)), rows[3]);
}

pub(crate) fn render_picker(
    frame: &mut Frame<'_>,
    area: Rect,
    picker: &Picker,
    current_model: &str,
    current_effort: &str,
) {
    let lines = match picker {
        Picker::Switchboard { units, index } => {
            switchboard_lines(units, *index, area.width, area.height)
        }
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

fn switchboard_lines<'a>(
    units: &[ActivationUnit],
    selected: usize,
    width: u16,
    height: u16,
) -> Vec<Line<'a>> {
    let mut header = picker_header(
        "Switchboard",
        Some("↑/↓ or ctrl+j/k move · space toggle · enter edit · @ reference · esc close"),
    );
    let name_width = units
        .iter()
        .map(|unit| unit.id.len())
        .max()
        .unwrap_or(4)
        .min(usize::from(width.saturating_sub(12)))
        .max(4);
    header.push(Line::from(Span::styled(
        format!("    {:<name_width$}  Status", "Name"),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    header.truncate(usize::from(height));
    let visible = usize::from(height).saturating_sub(header.len());
    if visible == 0 {
        return header;
    }
    let mut all = Vec::new();
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
            Span::styled(
                format!("{:<name_width$}  ", clipped(&unit.id, name_width)),
                style,
            ),
            Span::styled(if unit.active { "on" } else { "off" }, style),
        ]));
    }
    let start = selected_line
        .saturating_sub(visible / 2)
        .min(all.len().saturating_sub(visible));
    header.extend(all.into_iter().skip(start).take(visible));
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switchboard_keeps_column_headers_visible_when_scrolled() {
        let units = (0..30)
            .map(|index| ActivationUnit {
                id: format!("skill-{index:02}"),
                kind: "skill".into(),
                active: true,
                description: "Never a duplicate third column".into(),
                source: format!("/tmp/skill-{index:02}/SKILL.md").into(),
            })
            .collect::<Vec<_>>();
        let lines = switchboard_lines(&units, 29, 100, 9);
        assert_eq!(lines.len(), 9);
        let text = lines
            .iter()
            .map(Line::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Name"));
        assert!(text.contains("Status"));
        assert!(text.contains("skill-29"));
        assert!(!text.contains("third column"));
    }

    #[test]
    fn menus_share_arrow_and_control_jk_navigation() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        for (code, modifiers, expected) in [
            (KeyCode::Up, KeyModifiers::NONE, Some(-1)),
            (KeyCode::Down, KeyModifiers::NONE, Some(1)),
            (KeyCode::Char('j'), KeyModifiers::CONTROL, Some(1)),
            (KeyCode::Char('k'), KeyModifiers::CONTROL, Some(-1)),
            (KeyCode::Char('j'), KeyModifiers::NONE, None),
        ] {
            assert_eq!(menu_direction(&KeyEvent::new(code, modifiers)), expected);
        }
    }

    #[test]
    fn slash_palette_filters_without_swallowing_command_arguments() {
        let state = crate::clause_state::NorthState::open().expect("Clause projects commands");
        assert_eq!(
            matching_commands(state.commands(), "/m")
                .into_iter()
                .map(CommandSpec::name)
                .collect::<Vec<_>>(),
            vec!["/model"]
        );
        assert!(matching_commands(state.commands(), "/delegate task").is_empty());
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
