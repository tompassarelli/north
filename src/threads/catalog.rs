//! The repositories and native namespaces NorthThreads tracks, read from bin/threads, which
//! owns them until the rest of NorthThreads moves to Rust.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

pub(crate) struct Catalog {
    pub owner: String,
    pub repos: Vec<String>,
    pub native: Vec<String>,
    aliases: BTreeMap<String, String>,
}

pub(crate) static CATALOG: LazyLock<Catalog> = LazyLock::new(|| Catalog::parse(include_str!("../../bin/threads")));

impl Catalog {
    fn parse(source: &str) -> Self {
        let quoted = Regex::new(r#""([^"]+)""#).expect("quoted pattern");
        let assignment = |name: &str, open: char, close: char| {
            let pattern = format!(r"(?m)^{name} = \{open}([^\{close}]*)\{close}");
            Regex::new(&pattern).expect("assignment pattern").captures(source).map(|found| found[1].to_owned()).unwrap_or_default()
        };
        let list = |name: &str| quoted.captures_iter(&assignment(name, '[', ']')).map(|found| found[1].to_owned()).collect();
        let pairs: Vec<String> = quoted.captures_iter(&assignment("ALIASES", '{', '}')).map(|found| found[1].to_owned()).collect();
        let owner = Regex::new(r#"(?m)^OWNER = "([^"]+)""#).expect("owner pattern").captures(source).map(|found| found[1].to_owned()).unwrap_or_default();
        Self { owner, repos: list("REPOS"), native: list("NATIVE"), aliases: pairs.chunks_exact(2).map(|pair| (pair[0].clone(), pair[1].clone())).collect() }
    }

    pub(crate) fn is_native(&self, repo: &str) -> bool {
        self.native.iter().any(|name| name == repo)
    }

    /// `smashcraft#222`, `tompassarelli/smashcraft#222` or `nixos-config#3` as a repository or
    /// namespace and number.
    pub(crate) fn parse_ref(&self, text: &str) -> Result<(String, u64), String> {
        let malformed = || format!("{text} is not an issue ref like smashcraft#222");
        let (name, number) = text.trim().rsplit_once('#').ok_or_else(malformed)?;
        let number = number.parse::<u64>().ok().filter(|number| *number > 0).ok_or_else(malformed)?;
        let name = name.strip_prefix(&format!("{}/", self.owner)).unwrap_or(name);
        let repo = self.aliases.get(name).map_or(name, String::as_str);
        if !self.repos.iter().chain(&self.native).any(|known| known == repo) {
            let known: Vec<&str> = self.repos.iter().chain(&self.native).map(String::as_str).collect();
            return Err(format!("unknown repo {name} (known: {})", known.join(", ")));
        }
        Ok((repo.to_owned(), number))
    }
}

#[cfg(test)]
mod tests {
    use super::CATALOG;

    #[test]
    fn a_checkout_alias_and_the_owner_prefix_name_the_same_issue() {
        assert_eq!(CATALOG.parse_ref("nixos-config#9"), Ok(("firn".to_owned(), 9)));
        assert_eq!(CATALOG.parse_ref("tompassarelli/firn#9"), Ok(("firn".to_owned(), 9)));
        assert!(CATALOG.parse_ref("nowhere#9").unwrap_err().starts_with("unknown repo nowhere"));
        assert!(CATALOG.parse_ref("smashcraft#0").is_err() && CATALOG.parse_ref("smashcraft").is_err());
        assert_eq!(CATALOG.parse_ref("smashcraft-client#26"), Ok(("smashcraft-client".to_owned(), 26)));
        assert_eq!(CATALOG.parse_ref("padbridge#1"), Ok(("padbridge".to_owned(), 1)));
        assert!(CATALOG.is_native("life") && !CATALOG.is_native("wisp"));
    }
}
