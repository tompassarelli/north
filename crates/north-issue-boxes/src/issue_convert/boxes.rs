use std::sync::LazyLock;

use regex::Regex;

static CHECKBOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-*] \[([ xX])\]\s*(.*)$").expect("checkbox pattern"));
static UNTICKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*] \[ \]").expect("unticked pattern"));

/// One checkbox line of an issue body with the lines written under it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkbox {
    pub ticked: bool,
    /// The checkbox line after its mark: what names the box.
    pub first: String,
    /// `first` and every line indented under it that is not itself a checkbox, word for word.
    pub text: String,
    /// The checkbox this one is indented under, as an index into the same list.
    pub parent: Option<usize>,
}

/// The unticked checkboxes of `body` as the tracker's own count reads them.
pub fn unticked(body: &str) -> usize {
    UNTICKED.find_iter(body).count()
}

/// `body` with the first unticked checkbox whose text is `first` ticked and `note` written after it; None when no box reads so.
pub fn tick(body: &str, first: &str, note: &str) -> Option<String> {
    let mut done = false;
    let lines: Vec<String> = body
        .lines()
        .map(|line| match CHECKBOX.captures(line) {
            Some(marked) if !done && &marked[1] == " " && marked[2].trim() == first => {
                done = true;
                format!("{} {note}", line.replacen("[ ]", "[x]", 1))
            }
            _ => line.to_owned(),
        })
        .collect();
    done.then(|| {
        let mut text = lines.join("\n");
        if body.ends_with('\n') {
            text.push('\n');
        }
        text
    })
}

/// Every checkbox of `body`, ticked or not, in its order.
pub fn checkboxes(body: &str) -> Vec<Checkbox> {
    let mut found: Vec<Checkbox> = Vec::new();
    let mut above: Vec<(usize, usize)> = Vec::new();
    let mut blank = 0;
    for line in body.lines() {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        while above.last().is_some_and(|(at, _)| *at >= indent) {
            above.pop();
        }
        if let Some(marked) = CHECKBOX.captures(line) {
            let first = marked[2].trim().to_owned();
            found.push(Checkbox { ticked: &marked[1] != " ", text: first.clone(), first, parent: above.last().map(|(_, index)| *index) });
            above.push((indent, found.len() - 1));
        } else if let Some((at, index)) = above.last() {
            let text = &mut found[*index].text;
            text.push_str(&"\n".repeat(blank + 1));
            text.push_str(line.get((at + 2).min(indent)..).unwrap_or_else(|| line.trim_start()));
        }
        blank = 0;
    }
    found
}
