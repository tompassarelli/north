//! The checks run on a DeepSeek lane before a reviewer launches (north#1020): the review classes a script can see, so a
//! lane failing one goes back to its session with the check and spends no Sonnet. A box whose Done-when test already
//! stood at the lane's base is oracle-locked: the lane may not edit the file defining it.

pub struct Lane<'a> {
    /// `git diff origin/main...HEAD -U0` of the lane.
    pub diff: &'a str,
    /// The subject of the lane's head commit.
    pub subject: &'a str,
    /// The subjects of the commits on origin/main.
    pub main_subjects: &'a [String],
    /// The files that define the box's Done-when tests as of the lane's base.
    pub oracle_files: &'a [String],
}

/// A failed check: the class its verdict names and what the session is told.
#[derive(Debug, Eq, PartialEq)]
pub struct Failure {
    pub class: &'static str,
    pub why: String,
}

fn failure(class: &'static str, why: impl Into<String>) -> Option<Failure> {
    Some(Failure { class, why: why.into() })
}

fn is_test_file(path: &str) -> bool {
    path.contains("tests") || path.contains("/test")
}

fn changed(diff: &str, mark: char) -> impl Iterator<Item = &str> {
    let header = if mark == '+' { "+++" } else { "---" };
    diff.lines().filter(move |line| line.starts_with(mark) && !line.starts_with(header)).map(|line| &line[1..])
}

fn files(diff: &str) -> Vec<&str> {
    diff.lines().filter_map(|line| line.strip_prefix("+++ b/")).collect()
}

fn count(diff: &str, mark: char) -> usize {
    changed(diff, mark).filter(|line| line.trim() == "#[test]").count()
}

fn host_reading(diff: &str) -> Option<&str> {
    let mut file = "";
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = path;
        } else if line.starts_with('+') && is_test_file(file) && ["\"/proc", "SystemTime::now", "Instant::now"].iter().any(|needle| line.contains(needle)) {
            return Some(file);
        }
    }
    None
}

/// The first mechanical check `lane` fails, if any.
pub fn failing(lane: &Lane) -> Option<Failure> {
    if lane.main_subjects.iter().any(|subject| subject == lane.subject) {
        return failure("duplicate", format!("origin/main already holds a commit named `{}`", lane.subject));
    }
    if lane.main_subjects.iter().any(|subject| subject.strip_prefix("Revert \"").and_then(|rest| rest.strip_suffix('"')) == Some(lane.subject)) {
        return failure("restored-revert", format!("origin/main reverted a commit named `{}`; do not restore it", lane.subject));
    }
    if let Some(edited) = files(lane.diff).into_iter().find(|file| lane.oracle_files.iter().any(|oracle| oracle == file)) {
        return failure("oracle-edit", format!("{edited} defines the box's Done-when test, which stood before staffing; the lane may not edit it"));
    }
    if let Some(file) = host_reading(lane.diff) {
        return failure("test-reads-host", format!("a test in {file} reads /proc or the wall clock"));
    }
    let (added, removed) = (count(lane.diff, '+'), count(lane.diff, '-'));
    if added > removed {
        return failure("test-budget", format!("the lane adds {added} tests and deletes {removed}; delete as many as it adds"));
    }
    None
}
