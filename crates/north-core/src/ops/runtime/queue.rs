pub(super) struct Entry {
    pub sha: String,
    pub failed: bool,
}

pub(super) fn next(entries: &[Entry], contains: impl Fn(&str, &str) -> bool) -> Option<&str> {
    let waiting = || entries.iter().filter(|entry| !entry.failed);
    waiting()
        .find(|entry| !waiting().any(|newer| newer.sha != entry.sha && contains(&entry.sha, &newer.sha)))
        .map(|entry| entry.sha.as_str())
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
