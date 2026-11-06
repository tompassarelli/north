use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
}

impl Hunk {
    fn old_end(self) -> u32 {
        self.old_start + self.old_count.max(1) - 1
    }

    pub fn touches(self, start: u32, end: u32) -> bool {
        if self.old_count == 0 {
            start <= self.old_start && self.old_start < end
        } else {
            self.old_start <= end && self.old_end() >= start
        }
    }

    pub fn lines(self) -> std::ops::RangeInclusive<u32> {
        self.old_start..=self.old_end()
    }
}

pub type Hunks = BTreeMap<String, Vec<Hunk>>;

pub fn parse(diff: &str) -> Hunks {
    let mut files = Hunks::new();
    let mut current: Option<String> = None;
    let mut in_header = true;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            in_header = true;
        } else if let (true, Some(path)) = (in_header, line.strip_prefix("--- ")) {
            current = path.strip_prefix("a/").map(str::to_owned);
        } else if let (true, Some(path)) = (in_header, line.strip_prefix("+++ ")) {
            if let Some(path) = path.strip_prefix("b/") {
                current = Some(path.to_owned());
            }
        } else if let (Some(file), Some(header)) = (&current, line.strip_prefix("@@ -"))
            && let Some(hunk) = header_hunk(header)
        {
            in_header = false;
            files.entry(file.clone()).or_default().push(hunk);
        }
    }
    files
}

fn header_hunk(header: &str) -> Option<Hunk> {
    let (ranges, _) = header.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |text: &str| -> Option<(u32, u32)> {
        match text.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((text.parse().ok()?, 1)),
        }
    };
    let ((old_start, old_count), (new_start, new_count)) = (range(old)?, range(new)?);
    Some(Hunk { old_start, old_count, new_start, new_count })
}

pub fn carry(start: u32, end: u32, hunks: &[Hunk]) -> Option<(u32, u32)> {
    let mut delta = 0i64;
    for hunk in hunks {
        if hunk.touches(start, end) {
            return None;
        }
        let before = if hunk.old_count == 0 { hunk.old_start < start } else { hunk.old_end() < start };
        if before {
            delta += i64::from(hunk.new_count) - i64::from(hunk.old_count);
        }
    }
    let shift = |line: u32| u32::try_from(i64::from(line) + delta).ok();
    Some((shift(start)?, shift(end)?))
}

pub fn stretch(start: u32, end: u32, hunks: &[Hunk]) -> (u32, u32) {
    let delta = |reaches: &dyn Fn(&Hunk) -> bool| hunks.iter().filter(|hunk| reaches(hunk)).map(|hunk| i64::from(hunk.new_count) - i64::from(hunk.old_count)).sum::<i64>();
    let before_start = delta(&|hunk: &Hunk| if hunk.old_count == 0 { hunk.old_start < start } else { hunk.old_end() < start });
    let through_end = delta(&|hunk: &Hunk| if hunk.old_count == 0 { hunk.old_start < end } else { hunk.old_start <= end });
    let shift = |line: u32, by: i64| u32::try_from((i64::from(line) + by).max(1)).unwrap_or(line);
    (shift(start, before_start), shift(end, through_end).max(shift(start, before_start)))
}

#[cfg(test)]
#[path = "hunks_tests.rs"]
mod tests;
