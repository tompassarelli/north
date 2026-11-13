use super::MARKER;

const FIELDS: &str = "Category: bug-known-cause\nSpec: measured\nScope: cross-module\nClass: a gate that runs after landing failed on a landed commit: every section below.\n";
const HEADER: &str = "Gates that run after landing and failed; fix forward in the next landing of the same lane; each row is one landed commit.";
const DROPPED: &str = "Older sections dropped: ";
const DONE_WHEN: &str = "\n## Done when\n- [ ] No section remains below: each failed gate is fixed forward on main and its section deleted.";
const CHECK: &str = " Done when: `gh issue list --state open --search 'in:title \"After-landing gate debt\"' --json body -q '.[].body' | rg -qF '<!-- after-landing-gate: '; test $? -eq 1` exits 0.";
const SPLIT_ONLY_GATE: &str = "crate-lines";

fn is_box(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("- [ ] ") || line.starts_with("- [x] ")
}

pub(super) fn split_sections(body: &str) -> Vec<String> {
    let mut sections: Vec<String> = Vec::new();
    let mut after_marker = false;
    let mut fenced = false;
    for line in body.lines() {
        let is_marker = line.starts_with(MARKER);
        if is_marker && !after_marker {
            sections.push(String::new());
        }
        fenced ^= line.starts_with("```");
        if let Some(last) = sections.last_mut()
            && (fenced || !is_box(line))
        {
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

/// The boxes beside the first, whether written above the sections or appended under one.
pub(super) fn kept_boxes(body: &str) -> String {
    let Some((_, after)) = body.split_once(DONE_WHEN) else { return String::new() };
    let after = after.split_once('\n').map_or("", |(_, rest)| rest);
    let (above, sections) = after.find(MARKER).map_or((after, ""), |at| after.split_at(at));
    let mut fenced = false;
    let appended = sections.lines().filter(|line| {
        fenced ^= line.starts_with("```");
        !fenced && is_box(line)
    });
    let mut kept = above.trim().to_owned();
    for line in appended {
        if !kept.is_empty() {
            kept.push('\n');
        }
        kept.push_str(line);
    }
    kept
}

/// Which boxes the first box waits on: the kept ones, when every gate left in every section is a crate that only its own split clears, so no section can be fixed forward in a landing.
fn waits_on_boxes(kept: &str, sections: &[String]) -> Option<String> {
    let count = kept.lines().filter(|line| is_box(line)).count();
    let split_only = |section: &String| section.lines().find_map(|line| line.split_once(") gates: ")).is_some_and(|(_, gates)| gates.split(", ").all(|gate| gate.trim() == SPLIT_ONLY_GATE));
    if count == 0 || sections.is_empty() || !sections.iter().all(split_only) {
        return None;
    }
    let positions: Vec<String> = (2..=count + 1).map(|position| position.to_string()).collect();
    let (last, rest) = positions.split_last()?;
    Some(if rest.is_empty() { format!("box {last}") } else { format!("boxes {} and {last}", rest.join(", ")) })
}

pub(super) fn render(dropped: usize, kept: &str, sections: &[String]) -> String {
    let mut body = format!("{FIELDS}{HEADER}\n");
    if dropped > 0 {
        body.push_str(&format!("{DROPPED}{dropped}.\n"));
    }
    body.push_str(DONE_WHEN);
    body.push_str(CHECK);
    if let Some(boxes) = waits_on_boxes(kept, sections) {
        body.push_str(&format!(" Waits on {boxes}."));
    }
    body.push('\n');
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
