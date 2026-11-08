use std::collections::BTreeSet;

use super::it_modules::TREE_WIDE;

pub const TABLE: &str = "tests/it/tree_wide_units.txt";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Units(BTreeSet<(String, String)>);

impl Units {
    pub fn parse(table: &str, packages: &BTreeSet<&str>) -> Result<Self, String> {
        let mut units = BTreeSet::new();
        for (index, line) in table.lines().enumerate() {
            let (line, comment) = line.split_once(" #").unwrap_or((line, ""));
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let at = index + 1;
            let words: Vec<&str> = line.split_whitespace().collect();
            let [package, test] = words[..] else {
                return Err(format!("{TABLE}:{at}: a line is PACKAGE TEST, with the test as its path inside the package's lib"));
            };
            if !packages.contains(package) {
                return Err(format!("{TABLE}:{at}: {test} names {package}, which is not a [[crate_dag.crate]] in structure.toml"));
            }
            if comment.trim().strip_prefix(TREE_WIDE).is_none_or(|reason| reason.trim().is_empty()) {
                return Err(format!("{TABLE}:{at}: {test} needs a trailing `# {TREE_WIDE} REASON`"));
            }
            if !units.insert((package.to_owned(), test.to_owned())) {
                return Err(format!("{TABLE}:{at}: {test} is listed twice"));
            }
        }
        Ok(Self(units))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn packages(&self) -> BTreeSet<String> {
        self.0.iter().map(|(package, _)| package.clone()).collect()
    }

    pub fn names(&self) -> String {
        self.0.iter().map(|(package, test)| format!("{package} {test}")).collect::<Vec<_>>().join(", ")
    }

    pub fn filter(&self) -> String {
        self.0.iter().map(|(package, test)| format!("(binary_id(={package})&test(={test}))")).collect::<Vec<_>>().join("|")
    }
}
