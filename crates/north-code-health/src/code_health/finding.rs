use super::fix::Fix;

#[derive(Debug, PartialEq)]
pub(super) struct Finding {
    pub(super) rule: &'static str,
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) item: String,
    pub(super) text: String,
    pub(super) fix: Option<Fix>,
}

impl Finding {
    pub(super) fn render(&self) -> String {
        format!("{} {}:{} {}", self.rule, self.path, self.line, self.text)
    }
}
