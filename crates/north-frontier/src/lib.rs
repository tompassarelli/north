//! The frontier scoreboard: north against plain Claude Code on the same boxes (docs/rationale/frontier.md).
//! Run rows go in as JSON lines, one per arm and task; the metrics and intervals come out as text.
pub mod report;
pub mod rows;
pub mod stats;

#[cfg(test)]
mod tests;
