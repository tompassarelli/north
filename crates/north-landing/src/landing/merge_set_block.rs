use super::{Entry, PLACEHOLDER, line_entry, merge_entries, text_merge};

struct Block {
    skeleton: String,
    entries: Vec<Entry>,
}

fn parse_block(text: &str, start: &str, end: &str) -> Result<Block, String> {
    let mut skeleton = String::new();
    let mut entries = Vec::new();
    let mut lines = text.lines();
    let mut opened = false;
    for line in lines.by_ref() {
        skeleton.push_str(line);
        skeleton.push('\n');
        if line.trim() == start {
            opened = true;
            break;
        }
    }
    if !opened {
        return Err(format!("no line {start:?} opens the block"));
    }
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim() == end {
            skeleton.push(PLACEHOLDER);
            skeleton.push('\n');
            skeleton.push_str(line);
            skeleton.push('\n');
            closed = true;
            break;
        }
        entries.push(line_entry(line));
    }
    if !closed {
        return Err(format!("no line {end:?} closes the block"));
    }
    for line in lines {
        skeleton.push_str(line);
        skeleton.push('\n');
    }
    Ok(Block { skeleton, entries })
}

pub(super) fn merge_block(base: &str, ours: &str, theirs: &str, start: &str, end: &str) -> Result<String, String> {
    let (base, ours, theirs) = (parse_block(base, start, end)?, parse_block(ours, start, end)?, parse_block(theirs, start, end)?);
    let (merged, conflicts) = text_merge(&base.skeleton, &ours.skeleton, &theirs.skeleton)?;
    if conflicts || merged.matches(PLACEHOLDER).count() != 1 {
        return Err("the text around the block conflicts".to_owned());
    }
    let entries = merge_entries(&base.entries, &ours.entries, &theirs.entries)?;
    let block = entries.iter().map(|entry| format!("{}\n", entry.text)).collect::<String>();
    Ok(merged.replace(&format!("{PLACEHOLDER}\n"), &block))
}
