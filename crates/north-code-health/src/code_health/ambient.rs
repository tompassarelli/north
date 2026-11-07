use super::habits::Finding;
use super::rust_text::RustFile;
use std::collections::BTreeMap;

pub(super) const RULE: &str = "ambient-test-input";

pub(super) struct Ambient {
    pub(super) fixtures: Vec<String>,
    pub(super) allowlist: String,
    pub(super) fixture: String,
    pub(super) allowed: BTreeMap<String, usize>,
}

impl Ambient {
    pub(super) fn read_allowed(&mut self, text: &str) -> Result<(), String> {
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let entry = line.split_once('\t').and_then(|(path, count)| Some((path.to_owned(), count.trim().parse().ok()?)));
            let (path, count) = entry.ok_or_else(|| format!("{}: expected <path><TAB><count>, got {line:?}", self.allowlist))?;
            self.allowed.insert(path, count);
        }
        Ok(())
    }

    fn reads(&self, file: &RustFile) -> Vec<(usize, String)> {
        if self.fixtures.contains(&file.path) { Vec::new() } else { file.host_reads() }
    }

    pub(super) fn findings(&self, files: &[RustFile], tree: bool) -> Vec<Finding> {
        let mut out = Vec::new();
        let mut counted: BTreeMap<&str, usize> = BTreeMap::new();
        for file in files {
            let reads = self.reads(file);
            counted.insert(&file.path, reads.len());
            let allowed = self.allowed.get(&file.path).copied().unwrap_or(0);
            if reads.len() <= allowed {
                continue;
            }
            for (line, item) in reads {
                let text = format!("{item} in test code reads the real host — {}; {} lists {allowed} such reads for this file and only shrinks", self.fixture, self.allowlist);
                out.push(Finding { rule: RULE, path: file.path.clone(), line, item, text });
            }
        }
        if tree {
            for (path, allowed) in &self.allowed {
                let found = counted.get(path.as_str()).copied().unwrap_or(0);
                if found < *allowed {
                    let text = format!("{} lists {allowed} host reads in tests but the file has {found} — lower the count to {found} (delete the line at 0)", self.allowlist);
                    out.push(Finding { rule: RULE, path: path.clone(), line: 0, item: String::new(), text });
                }
            }
        }
        out
    }
}
