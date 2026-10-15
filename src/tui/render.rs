use std::env;
use std::path::PathBuf;
use std::time::Duration;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};
use ratatui::{Frame, layout::Rect};

use crate::command_surface::{render_picker, render_reference_menu, render_slash_menu};
use crate::north_state::NorthState;
use crate::{command_surface, prompts};

use super::app::{App, Speaker, TranscriptViewport};

#[cfg(test)]
pub(super) fn render(frame: &mut Frame<'_>, app: &mut App) {
    render_application(frame, app, true);
}

pub(super) fn render_application(frame: &mut Frame<'_>, app: &mut App, slash_menu: bool) {
    app.slash_menu.observe_input(&app.composer.text());
    let area = padded(frame.area());
    let editor_width = area.width.saturating_sub(2).max(1);
    let composer_height = app
        .composer
        .measure(editor_width)
        .min(area.height.saturating_sub(2).max(1));
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(composer_height),
            Constraint::Length(1),
        ])
        .split(area);

    app.sync_prompt_editor();
    if app.state.menu().kind == "usage" {
        command_surface::render_usage(frame, rows[0], &app.state.menu().title, app.state.usage(), app.state.menu().selection as usize);
    } else if !app.state.menu().kind.is_empty() {
        command_surface::render_menu(frame, rows[0], app.state.menu(), &app.menu_editor);
    } else if let Some(picker) = app.picker.as_ref() {
        render_picker(frame, rows[0], picker, &app.model, &app.reasoning_effort);
    } else if let Some(prompt) = app.state.active_prompt() {
        prompts::render(frame, rows[0], prompt, &app.prompt_editor.lines().join("\n"), app.prompt_scroll);
    } else {
        match app.state.active_view() {
            "chat" => {
                let context = app.state.conversation(app.state.active_conversation().unwrap_or_default());
                let filtered = context.is_some_and(|context| context.transcript_changes || !context.transcript_query.is_empty());
                if app.transcript.is_empty() && app.state.pending_inputs().is_empty() && !filtered {
                    render_welcome(frame, rows[0], app);
                } else if app.transcript.is_empty() && filtered {
                    frame.render_widget(Paragraph::new("No matching messages. Ctrl+F edits the search; Ctrl+D toggles changes."), rows[0]);
                } else {
                    let transcript_area = if filtered { Rect {y: rows[0].y.saturating_add(1), height: rows[0].height.saturating_sub(1), ..rows[0]} } else { rows[0] };
                    let id = app.state.active_conversation().unwrap_or_default().to_owned();
                    let offset = app.state.conversation(&id).map(|context| context.transcript_offset).unwrap_or_default();
                    let limit = app.render_transcript(frame, transcript_area, offset);
                    if app.state.conversation(&id).is_some_and(|context| context.transcript_limit != u64::from(limit)) {
                        if let Err(error) = app.state.size_transcript(u64::from(limit)) { app.record_error(error); }
                    }
                    if let Some(context) = app.state.conversation(&id) {
                        let mut heading = Vec::new();
                        if context.transcript_changes { heading.push("Changes only · Ctrl+D shows all".to_owned()); }
                        if !context.transcript_query.is_empty() { heading.push(format!("Search: {} · Ctrl+F edits", context.transcript_query)); }
                        if !heading.is_empty() { frame.render_widget(Paragraph::new(heading.join(" · ")).style(Style::default().fg(Color::Yellow)), Rect {height:1, ..rows[0]}); }
                    }
                }
            }
            "goals" => frame.render_widget(
                Paragraph::new(goals_text(&app.state)).wrap(Wrap { trim: false }),
                rows[0],
            ),
            unknown => frame.render_widget(
                Paragraph::new(format!("Unknown projected view: {unknown}")),
                rows[0],
            ),
        }
    }

    let composer_style = Style::default()
        .fg(Color::Rgb(229, 231, 235))
        .bg(Color::Rgb(37, 39, 45));
    frame.render_widget(Paragraph::new("").style(composer_style), rows[1]);
    frame.render_widget(
        Paragraph::new(Span::styled("❯ ", composer_style.fg(Color::Cyan))),
        Rect {
            width: rows[1].width.min(2),
            ..rows[1]
        },
    );
    if rows[1].width > 2 {
        frame.render_widget(
            app.composer.textarea(),
            Rect {
                x: rows[1].x.saturating_add(2),
                width: rows[1].width.saturating_sub(2),
                ..rows[1]
            },
        );
    }
    if app.picker.is_none() && app.state.menu().kind.is_empty() && app.state.active_prompt().is_none() {
        if app.reference_query().is_some() {
            render_reference_menu(
                frame,
                rows[1],
                app.state.references(),
                app.state.reference_selection(),
            );
        } else if slash_menu && app.slash_menu.visible() {
            render_slash_menu(
                frame,
                rows[1],
                app.state.commands(),
                &app.composer.text(),
                app.slash_menu.selected,
            );
        }
    }

    let directory = env::var_os("HOME").map(PathBuf::from)
        .and_then(|home| app.cwd.strip_prefix(home).ok().map(|relative| {
            if relative.as_os_str().is_empty() { "~".to_owned() }
            else { format!("~/{}", relative.display()) }
        }))
        .unwrap_or_else(|| app.cwd.display().to_string());
    let muted = Style::default().fg(Color::DarkGray);
    let mut context = vec![
        Span::raw(format!("{} {}", app.model, app.reasoning_effort)),
        Span::styled(" · ", muted),
        Span::styled(directory, Style::default().fg(Color::Green)),
    ];
    if !app.branch.is_empty() {
        context.push(Span::styled(format!(" · {}", app.branch), muted));
    }
    let mut tabs = Vec::new();
    for (index, view) in app.state.views().iter().enumerate() {
        if index > 0 { tabs.push(Span::styled(" | ", muted)); }
        let style = if app.state.active_view() == view.name {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else { muted };
        tabs.push(Span::styled(view.label.as_str(), style));
    }
    tabs.push(Span::styled(" > ", muted));
    if app.state.active_view() == "chat" {
        tabs.extend(context);
    } else {
        tabs.push(Span::raw("desired outcomes"));
    }
    if app.turns.len() > 1 {
        tabs.push(Span::styled(format!(" · {} working", app.turns.len()), muted));
    }
    if app.status == "failed" {
        tabs.push(Span::styled(" · failed", Style::default().fg(Color::Red)));
    }
    frame.render_widget(Paragraph::new(Line::from(tabs)), rows[2]);
    if let Some(editor) = app.transcript_search.as_ref() {
        frame.render_widget(Paragraph::new(format!("Find: {} · Enter filter · Esc cancel", editor.lines().join(" ")))
            .style(Style::default().fg(Color::Yellow)), rows[2]);
    } else if app.status == "Copied displayed messages" {
        frame.render_widget(Paragraph::new("Copied displayed messages"), rows[2]);
    }
}

pub(super) fn render_welcome(frame: &mut Frame<'_>, area: Rect, app: &App) {
    if area.is_empty() {
        return;
    }
    let card = Rect {
        x: area.x,
        y: area.y,
        width: area.width.min(72),
        height: area.height.min(7),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(card);
    frame.render_widget(block, card);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(">_ ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!("North (v{})", env!("CARGO_PKG_VERSION")),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::default(),
            welcome_field("model:", &format!("{} {}", app.model, app.reasoning_effort)),
            welcome_field("directory:", &app.cwd.display().to_string()),
            welcome_field("permissions:", "workspace write; approvals follow your settings"),
        ]),
        inner,
    );
}

pub(super) fn welcome_field<'a>(label: &'a str, value: &'a str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{label:<13}"), Style::default().fg(Color::DarkGray)),
        Span::raw(value),
    ])
}

pub(super) fn goals_text(state: &NorthState) -> Text<'static> {
    if state.goals().is_empty() {
        return Text::from(vec![
            Line::from("No Goals"),
            Line::from(""),
            Line::from(Span::styled(
                "Use /goal to create one.",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
    }
    let active_order = state.active_goal().map(|goal| goal.order());
    let mut lines = Vec::new();
    for (index, goal) in state.goals().iter().enumerate() {
        if index > 0 {
            lines.push(Line::from(""));
        }
        let active = active_order == Some(goal.order());
        lines.push(Line::from(vec![
            Span::styled(
                if active { "● " } else { "○ " },
                Style::default().fg(if active {
                    Color::Green
                } else {
                    Color::DarkGray
                }),
            ),
            Span::styled(
                goal.title().to_owned(),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  #{} · {}", goal.order(), goal.status()),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(format!("  {}", goal.objective())));
        if !goal.prior_objectives().is_empty() {
            lines.push(Line::from(Span::styled(
                "  Previous objectives",
                Style::default().fg(Color::DarkGray),
            )));
            lines.extend(goal.prior_objectives().iter().map(|objective| {
                Line::from(Span::styled(
                    format!("    · {objective}"),
                    Style::default().fg(Color::DarkGray),
                ))
            }));
        }
    }
    Text::from(lines)
}

#[cfg(test)]
pub(super) fn conversation_text(app: &App, width: usize) -> Text<'_> {
    let mut text = transcript_text(app, width);
    append_conversation_status(app, &mut text.lines);
    text
}

pub(super) fn transcript_text(app: &App, width: usize) -> Text<'_> {
    let mut lines = Vec::new();
    for (index, (speaker, message)) in app.transcript.iter().enumerate() {
        if index > 0 {
            lines.push(Line::default());
        }
        match speaker {
            Speaker::Operator => {
                let style = Style::default()
                    .fg(Color::Rgb(226, 220, 199))
                    .bg(Color::Rgb(52, 58, 62));
                for (line_index, line) in wrap_operator_message(message, width).iter().enumerate() {
                    let marker = if line_index == 0 { "› " } else { "  " };
                    let content = format!(
                        "{marker}{line:<padding$}",
                        padding = width.saturating_sub(2)
                    );
                    lines.push(Line::from(content).style(style));
                }
            }
            Speaker::North => {
                let options = tui_markdown::Options::default()
                    .image_fallback(tui_markdown::ImageFallback::AltTextAndUrl);
                let mut markdown = tui_markdown::from_str_with_options(message, &options);
                if let Some(first_content) = markdown
                    .lines
                    .iter()
                    .position(|line| !line.spans.iter().all(|span| span.content.trim().is_empty()))
                {
                    for (line_index, line) in markdown.lines.iter_mut().enumerate() {
                        let prefix = if line_index == first_content {
                            Span::styled("• ", Style::default().fg(Color::Gray))
                        } else if line_index > first_content {
                            Span::raw("  ")
                        } else {
                            continue;
                        };
                        line.spans.insert(0, prefix);
                    }
                }
                lines.extend(markdown.lines);
            }
            Speaker::CommandSuccess | Speaker::CommandFailure | Speaker::CommandRunning => {
                let color = if matches!(speaker, Speaker::CommandSuccess) {
                    Color::Green
                } else if matches!(speaker, Speaker::CommandRunning) {
                    Color::Yellow
                } else {
                    Color::Red
                };
                lines.push(Line::from(vec![
                    Span::styled("• ", Style::default().fg(color)),
                    Span::styled(if matches!(speaker, Speaker::CommandRunning) { "Running " } else { "Ran " }, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(message.lines().next().unwrap_or_default()),
                ]));
                lines.extend(message.lines().skip(1).map(|line| Line::from(format!("  {line}"))));
            }
            Speaker::FileChange => {
                lines.extend(message.lines().map(|line| Line::from(line).style(Style::default().fg(
                    if line.starts_with('+') { Color::Green } else if line.starts_with('-') { Color::Red } else { Color::Gray }
                ))));
            }
            Speaker::Notice => lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Gray)),
                Span::styled(message, Style::default().add_modifier(Modifier::DIM)),
            ])),
            Speaker::System => lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Red)),
                Span::styled(message, Style::default().fg(Color::Red)),
            ])),
        }
    }
    Text::from(lines)
}

pub(super) fn append_conversation_status(app: &App, lines: &mut Vec<Line<'_>>) {
    if app.is_working() {
        if !app.transcript.is_empty() {
            lines.push(Line::default());
        }
        lines.push(working_line(app));
    }
    let conversation = app.state.active_conversation().unwrap_or_default();
    for input in app.state.pending_inputs().iter().filter(|input| input.conversation == conversation && input.status != "accepted") {
        lines.push(Line::from(format!("{} #{}: {}", input.status, input.number, input.text))
            .style(Style::default().fg(Color::Yellow)));
    }
}

pub(super) fn wrap_operator_message(message: &str, width: usize) -> Vec<String> {
    let content_width = width.saturating_sub(2).max(1);
    let mut wrapped = Vec::new();
    for source_line in message.lines() {
        if source_line.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in source_line.split_whitespace() {
            let separator = usize::from(!line.is_empty());
            if line.chars().count() + separator + word.chars().count() <= content_width {
                if separator == 1 {
                    line.push(' ');
                }
                line.push_str(word);
                continue;
            }
            if !line.is_empty() {
                wrapped.push(std::mem::take(&mut line));
            }
            let mut remaining = word;
            while remaining.chars().count() > content_width {
                let split = remaining
                    .char_indices()
                    .nth(content_width)
                    .map_or(remaining.len(), |(index, _)| index);
                wrapped.push(remaining[..split].to_owned());
                remaining = &remaining[split..];
            }
            line.push_str(remaining);
        }
        wrapped.push(line);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

pub(super) fn working_line(app: &App) -> Line<'static> {
    let elapsed = app
        .turns.get(app.state.active_conversation().unwrap_or_default())
        .map(|turn| turn.started.elapsed())
        .unwrap_or_default();
    let mut spans = shimmer_spans("•", elapsed);
    spans.push(Span::raw(" "));
    spans.extend(shimmer_spans("Working", elapsed));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        format!(
            "({} • esc to interrupt)",
            fmt_elapsed_compact(elapsed.as_secs())
        ),
        Style::default().add_modifier(Modifier::DIM),
    ));
    Line::from(spans)
}

pub(super) fn fmt_elapsed_compact(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        return format!("{elapsed_secs}s");
    }
    if elapsed_secs < 3600 {
        return format!("{}m {:02}s", elapsed_secs / 60, elapsed_secs % 60);
    }
    format!(
        "{}h {:02}m {:02}s",
        elapsed_secs / 3600,
        (elapsed_secs % 3600) / 60,
        elapsed_secs % 60
    )
}

pub(super) fn shimmer_spans(text: &str, elapsed: Duration) -> Vec<Span<'static>> {
    let characters = text.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return Vec::new();
    }
    let padding = 10;
    let period = characters.len() + padding * 2;
    let position = ((elapsed.as_secs_f32() % 2.0) / 2.0 * period as f32) as isize;
    characters
        .into_iter()
        .enumerate()
        .map(|(index, character)| {
            let distance = (index as isize + padding as isize - position).unsigned_abs() as f32;
            let intensity = if distance <= 5.0 {
                let x = std::f32::consts::PI * (distance / 5.0);
                0.5 * (1.0 + x.cos())
            } else {
                0.0
            };
            let style = if intensity < 0.2 {
                Style::default().add_modifier(Modifier::DIM)
            } else if intensity < 0.6 {
                Style::default()
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            Span::styled(character.to_string(), style)
        })
        .collect()
}

pub(super) fn padded(area: Rect) -> Rect {
    if area.width > 2 && area.height > 2 {
        Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 1)
    } else {
        area
    }
}

impl App {
    pub(super) fn cached_transcript_text(&mut self, width: usize) -> Text<'static> {
        if let Some((revision, cached_width, text)) = &self.rendered_transcript
            && *revision == self.transcript_revision && *cached_width == width
        {
            return text.clone();
        }
        let text = transcript_text(self, width);
        let owned = Text {
            lines: text.lines.into_iter().map(|line| Line {
                spans: line.spans.into_iter().map(|span| Span::styled(span.content.to_string(), span.style)).collect(),
                style: line.style,
                alignment: line.alignment,
            }).collect(),
            style: text.style,
            alignment: text.alignment,
        };
        self.rendered_transcript = Some((self.transcript_revision, width, owned.clone()));
        owned
    }

    pub(super) fn render_transcript(&mut self, frame: &mut Frame<'_>, area: Rect, offset: u64) -> u16 {
        use ratatui::widgets::Widget;
        let mut status = Vec::new();
        append_conversation_status(self, &mut status);
        if !self.rendered_transcript_viewport.as_ref().is_some_and(|cached| {
            cached.revision == self.transcript_revision && cached.offset == offset
                && cached.buffer.area == area && cached.status == status
        }) {
            let width = usize::from(area.width.max(1));
            let mut text = self.cached_transcript_text(width);
            text.lines.extend(status.iter().cloned());
            let count = text.lines.iter().map(|line| line.width().max(1).div_ceil(width)).sum::<usize>();
            let limit = count.saturating_sub(usize::from(area.height)).min(u16::MAX as usize) as u16;
            let hidden = limit.saturating_sub(offset.min(u16::MAX as u64) as u16);
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            Paragraph::new(text).wrap(Wrap { trim: false }).scroll((hidden, 0)).render(area, &mut buffer);
            self.rendered_transcript_viewport = Some(TranscriptViewport {
                revision: self.transcript_revision, offset, status, limit, buffer,
            });
        }
        let cached = self.rendered_transcript_viewport.as_ref().expect("rendered transcript viewport");
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                frame.buffer_mut()[(x, y)] = cached.buffer[(x, y)].clone();
            }
        }
        cached.limit
    }
}
