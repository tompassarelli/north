use regex::Regex;
use std::sync::LazyLock;

pub const HEADING: &str = "**Clean-up before closing**";
pub const BOXES: [&str; 3] = [
    "- [ ] Clean-up: development tests turned down. Every test written while building this that pins no guarantee a person would notice is deleted. Each kept test names its guarantee and why it stays: the bug it reproduces, or the contract it pins.",
    "- [ ] Clean-up: scaffolding removed. Debug output, temporary scripts, flags, fixtures, records and shims this change no longer needs are deleted.",
    "- [ ] Clean-up: nothing left for later. No TODOs, follow-up notes or half-removed old paths remain, and the docs describe what shipped.",
];
const REPOSITORIES: [&str; 3] = ["tompassarelli/north", "tompassarelli/makgora", "tompassarelli/wisp"];
const NOT_PRODUCT: [&str; 9] = ["documentation", "research", "question", "idea", "roadmap", "invalid", "duplicate", "wontfix", "superseded-by-fleet"];

static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t>]*[-*][ \t]+\[([ xX])\][ \t]+Clean-up:").expect("clean-up box pattern"));

pub fn group() -> String {
    format!("{HEADING}\n{}", BOXES.join("\n"))
}

pub fn covers(repo: &str) -> bool {
    REPOSITORIES.iter().any(|covered| covered.eq_ignore_ascii_case(repo))
}

pub fn applies(labels: &[String]) -> bool {
    !labels.iter().any(|label| NOT_PRODUCT.contains(&label.to_lowercase().as_str()))
}

pub fn is_box(line: &str) -> bool {
    BOX.is_match(line)
}

pub fn has_group(body: &str) -> bool {
    body.lines().any(is_box)
}

pub fn appended(body: &str) -> Option<String> {
    (!has_group(body)).then(|| format!("{}\n\n{}\n", body.trim_end(), group()))
}

pub fn unticked(body: &str) -> Vec<&str> {
    body.lines().filter(|line| BOX.captures(line).is_some_and(|found| &found[1] == " ")).map(str::trim).collect()
}
