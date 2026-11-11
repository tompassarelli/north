//! The subjects that stay on Claude (north#1020): harness/opencode.toml `[dispatch] claude_only` patterns, matched against
//! an issue's title and the box a launch works. Dispatch reads them to keep such a box off DeepSeek, and `north fleet
//! launch` reads them to refuse a DeepSeek worker on it however it was launched (north#1039).
use regex::Regex;
use toml_edit::{DocumentMut, Item};

pub const SETTINGS: &str = "harness/opencode.toml";

/// The providers whose workers run DeepSeek rather than Claude.
pub const DEEPSEEK_PROVIDERS: [&str; 2] = ["opencode", "pi"];

#[derive(Clone, Debug, Default)]
pub struct ClaudeOnly(Vec<Regex>);

impl ClaudeOnly {
    pub fn read() -> Self {
        Self::parse(crate::platform::harness::document(SETTINGS))
    }

    pub fn parse(document: &DocumentMut) -> Self {
        let patterns = document.get("dispatch").and_then(|table| table.get("claude_only")).and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str());
        Self(patterns.filter_map(|pattern| Regex::new(&format!("(?i){pattern}")).ok()).collect())
    }

    /// The pattern the issue `title` or `box_text` matches, which keeps the box on Claude.
    pub fn matching(&self, title: &str, box_text: Option<&str>) -> Option<&str> {
        let subject = format!("{title}\n{}", box_text.unwrap_or_default());
        self.0.iter().find(|pattern| pattern.is_match(&subject)).map(|pattern| pattern.as_str().trim_start_matches("(?i)"))
    }

    /// Why a `provider` worker may not work `issue` titled `title` on `box_text`, or `None` when it may.
    pub fn refusal(&self, provider: &str, issue: &str, title: &str, box_text: Option<&str>) -> Option<String> {
        if !DEEPSEEK_PROVIDERS.contains(&provider) {
            return None;
        }
        let pattern = self.matching(title, box_text)?;
        Some(format!("{issue} ({title}) matches claude_only pattern {pattern} in {SETTINGS}, so a {provider} DeepSeek worker may not take it; launch it on --provider claude"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_claude_only_issue_refuses_an_opencode_launch() {
        let only = ClaudeOnly::read();
        let title = "Confine the worker sandbox to its lane";
        for provider in DEEPSEEK_PROVIDERS {
            let refusal = only.refusal(provider, "north#1", title, Some("add a column")).unwrap_or_else(|| panic!("{provider} took a sandbox-titled issue"));
            assert!(refusal.contains("sandbox"), "{refusal}");
        }
        assert!(only.refusal("opencode", "north#2", "Add a column", Some("take the session lock first")).is_some(), "a claude_only box refuses whatever the title");
        assert_eq!(only.refusal("claude", "north#1", title, None), None);
        assert_eq!(only.refusal("opencode", "north#3", "Add a column to fleet list", Some("show the host")), None);
        let telemetry = "Capacity from measured usage";
        for text in [
            "from each host's capacity.db, report per hour the batch cores used against the host's threads",
            "read the watchdog samples.jsonl and report the 10-second kernel busy share",
        ] {
            for provider in DEEPSEEK_PROVIDERS {
                let refusal = only.refusal(provider, "north#4", telemetry, Some(text)).unwrap_or_else(|| panic!("{provider} took {:?}, which opens a file a lane-only domain hides", text));
                assert!(refusal.contains("claude_only pattern"), "{refusal}");
            }
            assert_eq!(only.refusal("claude", "north#4", telemetry, Some(text)), None, "a Claude worker reads the host's state, so it takes the box");
        }
        assert_eq!(only.refusal("opencode", "north#5", telemetry, Some("size a run to the measured capacity")), None, "a box about capacity that reads no host file stays available to DeepSeek");
    }
}
