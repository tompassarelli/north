use crate::error::NorthResult;

use super::record::{Answerer, Closure, Decision, GateBook};
use super::request::GateRequest;
use super::settle::settle_as;

/// An open gate's hold on launches: the issue it decides, the one box of it when it names one, and its label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hold {
    pub issue: String,
    pub box_text: Option<String>,
    pub gate: String,
}

impl Hold {
    /// Whether a launch for `issue` whose `Item:` line is `item` waits on this gate: a whole-issue hold refuses every launch on the issue; a box hold only the launch whose line carries the box's text, or one naming no line to tell it apart.
    pub fn refuses(&self, issue: &str, item: Option<&str>) -> bool {
        self.issue == issue && self.box_text.as_deref().is_none_or(|text| item.is_none_or(|line| carries(line, text)))
    }
}

fn carries(line: &str, text: &str) -> bool {
    line.to_lowercase().contains(&text.trim().to_lowercase())
}

/// The ticked box line of the issue `body` that the box-scoped gate `request` decides, once no unticked box carries its text.
pub fn ticked_box<'a>(request: &GateRequest, body: &'a str) -> Option<&'a str> {
    let text = request.box_text.as_deref()?;
    let boxes = |marks: &'static [&'static str]| body.lines().map(str::trim_start).filter(move |line| marks.iter().any(|mark| line.starts_with(mark)) && carries(line, text));
    if boxes(&["- [ ]", "* [ ]"]).next().is_some() {
        return None;
    }
    boxes(&["- [x]", "- [X]", "* [x]", "* [X]"]).next()
}

/// Closes the open box-scoped gate `id` whose box `line` was ticked, with the tick as its answer.
pub fn tick(book: &dyn GateBook, id: i64, line: &str, now_ms: u64, deliver: impl FnOnce(&str, &str) -> NorthResult<()>) -> NorthResult<Closure> {
    let text = format!("its box was ticked, so the work it decided is done: {}", line.trim());
    settle_as(book, id, Decision::Text(text), Answerer::Ticked, None, now_ms, deliver)
}
