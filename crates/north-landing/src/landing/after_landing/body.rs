use super::MARKER;

const FIELDS: &str = "Category: bug-known-cause\nClass: a gate that runs after landing failed on a landed commit: every section below.\n";
const HEADER: &str = "Gates that run after landing and failed; fix forward in the next landing of the same lane; each row is one landed commit.";
const DROPPED: &str = "Older sections dropped: ";
const DONE_WHEN: &str = "\n## Done when\n- [ ] No section remains below: each failed gate is fixed forward on main and its section deleted.\n";

pub(super) fn split_sections(body: &str) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    let mut after_marker = false;
    for line in body.lines() {
        let is_marker = line.starts_with(MARKER);
        if is_marker && !after_marker {
            sections.push(String::new());
        }
        if let Some(last) = sections.last_mut() {
            last.push_str(line);
            last.push('\n');
        }
        after_marker = is_marker;
    }
    sections
}

pub(super) fn dropped_before(body: &str) -> usize {
    body.lines().find_map(|line| line.strip_prefix(DROPPED)?.trim_end_matches('.').parse().ok()).unwrap_or(0)
}

pub(super) fn kept_boxes(body: &str) -> String {
    let Some((_, after)) = body.split_once(DONE_WHEN) else { return String::new() };
    after.find(MARKER).map_or(after, |at| &after[..at]).trim_end().to_owned()
}

pub(super) fn render(dropped: usize, kept: &str, sections: &[String]) -> String {
    let mut body = format!("{FIELDS}{HEADER}\n");
    if dropped > 0 {
        body.push_str(&format!("{DROPPED}{dropped}.\n"));
    }
    body.push_str(DONE_WHEN);
    if !kept.is_empty() {
        body.push_str(kept);
        body.push('\n');
    }
    for section in sections {
        body.push('\n');
        body.push_str(section);
    }
    body
}
