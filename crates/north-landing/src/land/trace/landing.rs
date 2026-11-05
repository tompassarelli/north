use super::{Category, Span, check_id};
use crate::land::telemetry::Stages;

pub const ROOT: &str = "landing";

pub struct Landed<'a> {
    pub ticket: &'a str,
    pub branch: &'a str,
    pub queued: f64,
    pub landed: &'a str,
    pub batch: usize,
}

pub fn trace_of(stages: &Stages, landed: &str) -> String {
    stages.checked_head().unwrap_or(landed).to_owned()
}

pub fn root_id(ticket: &str) -> String {
    format!("{ROOT}:{ticket}")
}

pub fn shared(stages: &Stages, landed: &str) -> Vec<Span> {
    let trace = trace_of(stages, landed);
    let mut spans = Vec::new();
    if let (Some(head), Some((start, end))) = (stages.checked_head(), stages.check_window()) {
        spans.push(Span::new(&trace, "check", Category::Lander, start, end).id(check_id(head)).with("candidate", head));
    }
    if let Some((start, end)) = stages.window("push") {
        let pushing = stages.pushing_at().map_or(start, |at| at.clamp(start, end));
        if pushing > start {
            spans.push(Span::new(&trace, "ready to push", Category::Queue, start, pushing));
        }
        spans.push(Span::new(&trace, "git push", Category::Git, pushing, end).with("landed", landed));
    }
    spans
}

pub fn spans(stages: &Stages, landed: &Landed<'_>) -> Vec<Span> {
    let trace = trace_of(stages, landed.landed);
    let id = root_id(landed.ticket);
    let started = stages.started_at();
    let end = stages.window("push").map_or_else(super::super::telemetry::epoch, |(_, end)| end);
    let root = Span::new(&trace, landed.branch, Category::Landing, landed.queued, end)
        .id(id.clone())
        .with("ticket", landed.ticket)
        .with("landed", landed.landed)
        .with("batch", landed.batch.to_string())
        .with("attempts", stages.attempts.to_string());
    let queue = Span::new(&trace, "queued", Category::Queue, landed.queued, started.max(landed.queued)).under(&id);
    vec![root, queue]
}
