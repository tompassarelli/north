use regex::Regex;
use std::sync::LazyLock;

static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*pub(?:\([a-z]+\))?\s+(?:const\s+|async\s+|unsafe\s+)*(fn|struct|enum|trait|const|static|type|mod)\s+(\w+)").expect("item pattern"));

pub fn items(source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let found = ITEM.captures(line)?;
            Some(format!("{} {} L{}", &found[1], &found[2], index + 1))
        })
        .collect()
}
