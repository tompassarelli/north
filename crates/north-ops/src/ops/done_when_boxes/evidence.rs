use crate::platform::box_oracles;

use super::{OPEN, Stamp, Verdict};

const KEPT_LINES: usize = 20;
const LINE_CHARS: usize = 160;
const ATTACHED_CHARS: usize = 1_000;

fn clipped(line: &str) -> String {
    let line = line.trim_end();
    if line.chars().count() <= LINE_CHARS { line.to_owned() } else { format!("{}…", line.chars().take(LINE_CHARS - 1).collect::<String>()) }
}

fn attachment(output: &str, indent: &str, budget: usize) -> Vec<String> {
    let fence = if output.contains("```") { "````" } else { "```" };
    let open = format!("{indent}  {fence}text");
    let close = format!("{indent}  {fence}");
    let mut spent = open.chars().count() + close.chars().count() + 2;
    let mut kept = Vec::new();
    for line in output.trim_end().lines().rev().take(KEPT_LINES) {
        let line = format!("{indent}  {}", clipped(line)).trim_end().to_owned();
        spent += line.chars().count() + 1;
        if spent > budget {
            break;
        }
        kept.push(line);
    }
    if kept.iter().all(|line| line.trim().is_empty()) {
        return Vec::new();
    }
    kept.reverse();
    [vec![open], kept, vec![close]].concat()
}

fn chars(text: &str) -> usize {
    text.trim().chars().count()
}

pub fn ticked(body: &str, verdicts: &[Verdict], stamp: &Stamp) -> String {
    let mut lines: Vec<String> = body.lines().map(str::to_owned).collect();
    let mut attached = Vec::new();
    for verdict in verdicts.iter().filter(|verdict| verdict.passed()) {
        let open = format!("{OPEN}{}", verdict.check.text);
        let Some(index) = lines.iter().position(|line| line.trim() == open) else { continue };
        let ticked = lines[index].trim_end().replacen("- [ ]", "- [x]", 1);
        lines[index] = format!("{ticked} {}", box_oracles::evidence(&verdict.check.command, verdict.run.exit, &stamp.sha, &stamp.date));
        attached.push((index, verdict));
    }
    let headroom = (2 * chars(body)).saturating_sub(chars(&lines.join("\n")));
    let budget = ATTACHED_CHARS.min(headroom / attached.len().max(1));
    attached.sort_by_key(|(index, _)| std::cmp::Reverse(*index));
    for (index, verdict) in attached {
        let indent: String = lines[index].chars().take_while(|character| character.is_whitespace()).collect();
        let block = attachment(&verdict.run.output, &indent, budget);
        lines.splice(index + 1..index + 1, block);
    }
    let mut text = lines.join("\n");
    if body.ends_with('\n') {
        text.push('\n');
    }
    text
}
