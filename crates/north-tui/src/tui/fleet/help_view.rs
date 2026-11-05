use std::path::Path;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::help_panel::Tab;
use super::key_table::BINDINGS;
use super::screen::Screen;

const MUTED: Style = Style::new().fg(Color::Gray);
const ACCENT: Style = Style::new().fg(Color::Cyan);
const HEADING: Style = Style::new().add_modifier(Modifier::BOLD);
const CHROME_ROWS: u16 = 5;

const DESCRIPTION: [&str; 2] = [
    "north fleet runs your Claude sessions side by side; read, message and answer them here.",
    "Sessions keep running when the window closes; open it again to pick them up.",
];

pub fn rows(screen: &Screen, ceiling: u16) -> u16 {
    let Some(tab) = screen.help.tab() else { return 0 };
    let wanted = match tab {
        Tab::General => u16::try_from(general_lines(screen).len()).unwrap_or(u16::MAX).saturating_add(3),
        Tab::Commands => CHROME_ROWS.saturating_add(list_length(screen, tab)),
        Tab::Shortcuts => CHROME_ROWS.saturating_add(list_length(screen, tab)),
    };
    wanted.min(ceiling)
}

fn list_length(screen: &Screen, tab: Tab) -> u16 {
    let length = match tab {
        Tab::Commands => screen.help.filtered_commands().len().max(1),
        _ => BINDINGS.len(),
    };
    u16::try_from(length).unwrap_or(u16::MAX)
}

pub fn draw_help(frame: &mut Frame<'_>, screen: &Screen, area: Rect) {
    let Some(tab) = screen.help.tab() else { return };
    if area.height < 2 {
        return;
    }
    let width = usize::from(area.width);
    let mut lines = vec![tab_row(tab)];
    match tab {
        Tab::General => lines.extend(general_lines(screen)),
        Tab::Commands => {
            lines.push(Line::from(Span::styled("Available Commands", HEADING)));
            lines.push(Line::default());
            let commands = screen.help.filtered_commands();
            let rows: Vec<(String, String)> = commands.iter().map(|(name, description)| ((*name).to_owned(), (*description).to_owned())).collect();
            lines.extend(list(&rows, screen.help.cursor(), area.height, "no command matches what you typed"));
        }
        Tab::Shortcuts => {
            lines.push(Line::from(Span::styled("Keyboard Shortcuts", HEADING)));
            lines.push(Line::default());
            let rows: Vec<(String, String)> = BINDINGS.iter().map(|binding| (binding.keys.to_owned(), format!("{} · {}", binding.description, binding.when))).collect();
            lines.extend(list(&rows, screen.help.cursor(), area.height, ""));
        }
    }
    let body = usize::from(area.height).saturating_sub(2);
    lines.truncate(body);
    while lines.len() < body {
        lines.push(Line::default());
    }
    lines.push(Line::default());
    lines.push(footer(width));
    frame.render_widget(Paragraph::new(lines), area);
}

fn tab_row(active: Tab) -> Line<'static> {
    let mut spans = Vec::new();
    for tab in Tab::ALL {
        let style = if tab == active { ACCENT.add_modifier(Modifier::UNDERLINED | Modifier::BOLD) } else { MUTED };
        spans.push(Span::styled(tab.label(), style));
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled("(←/→ or tab to cycle)", MUTED));
    Line::from(spans)
}

fn footer(width: usize) -> Line<'static> {
    let full = "Keyboard: ↑/↓ Navigate  ←/→ ^H/^L Tab/Shift+Tab Switch View  esc Close";
    if full.chars().count() <= width {
        return Line::from(vec![
            Span::styled("Keyboard:", HEADING),
            Span::styled(" ↑/↓", ACCENT),
            Span::styled(" Navigate ", MUTED),
            Span::styled(" ←/→ ^H/^L Tab/Shift+Tab", ACCENT),
            Span::styled(" Switch View ", MUTED),
            Span::styled(" esc", ACCENT),
            Span::styled(" Close", MUTED),
        ]);
    }
    Line::from(vec![Span::styled("Keyboard:", HEADING), Span::styled(" ↑/↓ ←/→ ^H/^L Tab/Shift+Tab esc", ACCENT)])
}

fn list(rows: &[(String, String)], cursor: usize, height: u16, empty: &str) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return vec![Line::from(Span::styled(format!("  {empty}"), MUTED))];
    }
    let room = usize::from(height.saturating_sub(CHROME_ROWS));
    let overflow = rows.len() > room;
    let visible = if overflow { room.saturating_sub(1).max(1) } else { room.max(1) };
    let cursor = cursor.min(rows.len() - 1);
    let start = cursor.saturating_sub(visible - 1);
    let end = (start + visible).min(rows.len());
    let key_width = rows.iter().map(|(key, _)| key.chars().count()).max().unwrap_or(0).min(34) + 3;
    let mut lines: Vec<Line<'static>> = rows[start..end]
        .iter()
        .enumerate()
        .map(|(offset, (key, description))| {
            let selected = start + offset == cursor;
            Line::from(vec![
                Span::styled(if selected { "> " } else { "  " }, ACCENT),
                Span::styled(format!("{key:<key_width$}"), ACCENT),
                Span::styled(description.clone(), if selected { Style::default().fg(Color::White) } else { MUTED }),
            ])
        })
        .collect();
    if overflow {
        lines.push(Line::from(Span::styled(format!("  [{}-{} of {} items]", start + 1, end, rows.len()), MUTED)));
    }
    lines
}

fn general_lines(screen: &Screen) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = DESCRIPTION.iter().map(|text| Line::from(Span::styled(*text, MUTED))).collect();
    lines.push(Line::default());
    let selected = screen.selected_session().map(|session| &session.record);
    let account = match selected {
        Some(record) => format!("{} ({})", record.account, record.provider.label()),
        None => screen.accounts().iter().map(|account| format!("{}·{}", account.provider.label(), account.label)).collect::<Vec<_>>().join(", "),
    };
    let workspace = selected.map_or_else(|| screen.workspace().to_path_buf(), |record| record.cwd.clone());
    let fleet = crate::fleet::registry::root_here().ok();
    let logs = selected.map(|record| record.host_dir.clone()).filter(|dir| !dir.as_os_str().is_empty());
    let facts = [
        ("Version:", Some(format!("north {}", env!("CARGO_PKG_VERSION")))),
        ("Account:", Some(account)),
        ("Workspace:", Some(tilde(&workspace))),
        ("Fleet State:", fleet.as_deref().map(tilde)),
        ("Session Logs:", logs.as_deref().map(tilde)),
    ];
    for (label, value) in facts {
        if let Some(value) = value {
            lines.push(Line::from(vec![Span::styled(format!("{label:<15}"), MUTED), Span::raw(value)]));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("Quick Reference", HEADING)));
    lines.push(Line::from(vec![Span::styled("  /  ", ACCENT), Span::styled("Type / to see available commands", MUTED)]));
    lines.push(Line::from(vec![Span::styled("  ?  ", ACCENT), Span::styled("Type ? in an empty message to see this help", MUTED)]));
    lines
}

fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
