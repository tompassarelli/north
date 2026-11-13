/// What a claim key carries between an issue and the text of the box it covers, as `north#9@the box`; a key without it covers the whole issue.
const BOX_MARK: char = '@';

/// The key a launch's claim on `task` has: the task alone, or the box of it `box_text` names.
pub fn claim_key(task: &str, box_text: Option<&str>) -> String {
    box_text.map_or_else(|| task.to_owned(), |text| format!("{task}{BOX_MARK}{text}"))
}

/// The issue and the box text a claim key names.
pub fn split_key(key: &str) -> (&str, Option<&str>) {
    key.split_once(BOX_MARK).map_or((key, None), |(task, text)| (task, Some(text)))
}

/// The keys of the claims a launch of `tasks` makes: the box applies to the first task, whose issue it is a box of, and `others` are the boxes its brief names of other issues.
pub fn claim_keys(tasks: &[String], box_text: Option<&str>, others: &[String]) -> Vec<String> {
    tasks.iter().enumerate().map(|(index, task)| claim_key(task, box_text.filter(|_| index == 0))).chain(others.iter().cloned()).collect()
}
