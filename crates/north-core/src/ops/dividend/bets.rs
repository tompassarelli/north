//! A bet issue's Dividend section: the `**Dividend:**` lines and the lines under a `Dividend`
//! heading, and the `north ops dividend CHECK` commands they name.
use super::checks::Check;

const COMMAND: &str = "north ops dividend ";

fn heading(line: &str) -> Option<&str> {
    line.trim_start()
        .strip_prefix('#')
        .map(|rest| rest.trim_start_matches('#').trim())
}

/// The lines of the body that belong to a Dividend section.
fn section(body: &str) -> Vec<&str> {
    let mut inside = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        if let Some(title) = heading(line) {
            inside = title.to_ascii_lowercase().starts_with("dividend");
        } else if line.trim_start().starts_with("**Dividend:**") || inside {
            lines.push(line);
        }
    }
    lines
}

/// The checks the Dividend section names, in first-seen order. A bet whose section names none has
/// a metric nobody measures yet.
pub fn checks(body: &str) -> Vec<Check> {
    let mut found = Vec::new();
    for line in section(body) {
        for (at, _) in line.match_indices(COMMAND) {
            let name: String = line[at + COMMAND.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            if let Some(check) = Check::parse(&name).filter(|check| !found.contains(check)) {
                found.push(check);
            }
        }
    }
    found
}
