use super::{Category, Span, check_id};
use crate::land::telemetry::Stages;

pub const ROOT: &str = "landing";

pub struct Landed<'a> {
    pub ticket: &'a str,
    pub branch: &'a str,
    pub queued: f64,
    pub landed: &'a str,
    pub batch: usize,
    pub hand_in: Option<HandIn>,
}

pub struct HandIn {
    pub called: f64,
    pub gates: f64,
    pub asked: f64,
    pub verdict: Option<f64>,
    pub judge: Option<String>,
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
    let mut spans = vec![root, queue];
    if let Some(hand_in) = &landed.hand_in {
        let mut worker = Span::new(&trace, "hand-in", Category::Queue, hand_in.called, landed.queued).under(&id).with("gates_done", hand_in.gates.to_string()).with("verdict_asked", hand_in.asked.to_string()).with("ticket_written", landed.queued.to_string());
        if let Some(verdict) = hand_in.verdict {
            worker = worker.with("verdict_in", verdict.to_string());
            spans.push(Span::new(&trace, "private-kind verdict", Category::Scans, hand_in.asked, verdict).under(&id).with("judge", hand_in.judge.clone().unwrap_or_else(|| north_base::platform::private_text::CACHED.to_owned())));
        }
        spans.push(worker);
    }
    spans
}
