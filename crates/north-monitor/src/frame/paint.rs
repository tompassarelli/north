use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::budget::{Budgets, Tone};
use crate::snapshot::Reading;
use crate::text::duration;

pub fn style(tone: Tone) -> Style {
    match tone {
        Tone::Green => Style::new().fg(Color::Green),
        Tone::Amber => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        Tone::Red => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        Tone::Plain => Style::new(),
    }
}

pub fn dim() -> Style {
    Style::new().fg(Color::DarkGray)
}

pub fn trend() -> Style {
    Style::new().fg(Color::Cyan)
}

pub fn label(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(Color::Gray))
}

pub enum Fresh {
    Ok(u64),
    Stale(u64),
    Unknown(String),
}

impl Fresh {
    pub fn is_ok(&self) -> bool {
        matches!(self, Fresh::Ok(_))
    }
}

pub type Boxed = (Line<'static>, Vec<Line<'static>>);

pub struct Painter<'a> {
    pub budgets: &'a Budgets,
    pub now: u64,
    pub alerts: Vec<(Tone, String)>,
}

impl Painter<'_> {
    pub fn metric(&mut self, id: &str, value: f64, text: impl Into<String>) -> Span<'static> {
        self.metric_of("", id, value, text)
    }

    pub fn metric_of(&mut self, subject: &str, id: &str, value: f64, text: impl Into<String>) -> Span<'static> {
        let text = text.into();
        let tone = self.budgets.tone(id, value);
        if matches!(tone, Tone::Amber | Tone::Red) {
            let alert = self.budgets.get(id).map_or_else(|| format!("{subject}{id} {}", text.trim()), |budget| {
                let limit = if tone == Tone::Red { budget.red } else { budget.amber };
                format!("{subject}{} {} (budget {limit} {})", budget.label, text.trim(), budget.unit)
            });
            self.alerts.push((tone, alert));
        }
        Span::styled(text, style(tone))
    }

    pub fn alert(&mut self, tone: Tone, text: String) {
        self.alerts.push((tone, text));
    }

    pub fn ago(&self, at: u64) -> String {
        duration(self.now.saturating_sub(at))
    }

    pub fn fresh<T>(&mut self, name: &str, reading: &Reading<T>, bound: u64) -> Fresh {
        let age = reading.at.map_or(0, |at| self.now.saturating_sub(at));
        if reading.value.is_none() {
            self.alert(Tone::Red, format!("{name} UNKNOWN"));
            return Fresh::Unknown(reading.error.clone().unwrap_or_else(|| "no reading yet".into()));
        }
        if age <= bound {
            return Fresh::Ok(age);
        }
        let tone = if age > bound * 10 { Tone::Red } else { Tone::Amber };
        self.alert(tone, format!("{name} STALE {}", duration(age)));
        Fresh::Stale(age)
    }
}

pub fn mark(fresh: &Fresh) -> Vec<Span<'static>> {
    match fresh {
        Fresh::Ok(_) => Vec::new(),
        Fresh::Stale(age) => vec![Span::styled(format!(" STALE {}", duration(*age)), style(Tone::Amber))],
        Fresh::Unknown(_) => vec![Span::styled(" UNKNOWN", style(Tone::Red))],
    }
}

pub fn unknown_line(name: &str, fresh: &Fresh) -> Option<Line<'static>> {
    let Fresh::Unknown(why) = fresh else { return None };
    Some(Line::from(vec![Span::styled(format!("{name} UNKNOWN "), style(Tone::Red).add_modifier(Modifier::REVERSED)), Span::styled(why.chars().take(70).collect::<String>(), dim())]))
}

pub fn title(name: &str, source: &str, fresh: &Fresh) -> Line<'static> {
    let head = Span::styled(format!(" {name} "), Style::new().add_modifier(Modifier::BOLD));
    let tail = match fresh {
        Fresh::Ok(age) => Span::styled(format!("{source} {} ", duration(*age)), dim()),
        Fresh::Stale(age) => Span::styled(format!("{source} STALE {} ", duration(*age)), style(Tone::Amber).add_modifier(Modifier::REVERSED)),
        Fresh::Unknown(_) => Span::styled(format!("{source} UNKNOWN "), style(Tone::Red).add_modifier(Modifier::REVERSED)),
    };
    Line::from(vec![head, tail])
}

pub fn tone_of(word: &str) -> Tone {
    match word {
        "landed" | "live" | "active" => Tone::Green,
        "failed" | "rolled" => Tone::Red,
        "conflict" | "paged" | "waiting" => Tone::Amber,
        _ => Tone::Plain,
    }
}
