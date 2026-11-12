use super::boxes;

/// The fewest leading words two boxes must share to be one box when one of them carries more text, as a copy that
/// gained its evidence does.
const FEWEST_SHARED: usize = 8;

/// The open issue that holds a box's text, and whether the box is ticked there.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpenCopy {
    pub issue: u64,
    pub ticked: bool,
}

/// An unticked box a closed issue still carries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Leftover {
    pub issue: u64,
    pub text: String,
    pub copy: Option<OpenCopy>,
}

fn words(text: &str) -> Vec<String> {
    text.split(|letter: char| !letter.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_lowercase).collect()
}

fn same(left: &[String], right: &[String]) -> bool {
    let shared = left.len().min(right.len());
    shared > 0 && (left == right || (shared >= FEWEST_SHARED && left[..shared] == right[..shared]))
}

/// Every unticked box of the `closed` issues with the first of the `open` issues that holds the same text.
pub fn leftovers(closed: &[(u64, &str)], open: &[(u64, &str)]) -> Vec<Leftover> {
    let held: Vec<(OpenCopy, Vec<String>)> =
        open.iter().flat_map(|(issue, body)| boxes::checkboxes(body).into_iter().map(move |checkbox| (OpenCopy { issue: *issue, ticked: checkbox.ticked }, words(&checkbox.first)))).collect();
    let mut found = Vec::new();
    for (issue, body) in closed {
        for checkbox in boxes::checkboxes(body).into_iter().filter(|checkbox| !checkbox.ticked) {
            let wanted = words(&checkbox.first);
            let copy = held.iter().find(|(_, held)| same(held, &wanted)).map(|(copy, _)| *copy);
            found.push(Leftover { issue: *issue, text: checkbox.first, copy });
        }
    }
    found
}
