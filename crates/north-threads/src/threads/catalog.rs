//! The repositories and native namespaces NorthThreads tracks: every GitHub repository under the
//! owner whose issues it follows, the checkout names that stand in for one, and the namespaces
//! that live only in the ledger and never reach GitHub.
use std::collections::BTreeMap;
use std::sync::LazyLock;

pub struct Catalog {
    pub owner: String,
    pub repos: Vec<String>,
    pub native: Vec<String>,
    aliases: BTreeMap<String, String>,
}

const OWNER: &str = "tompassarelli";
const REPOS: [&str; 11] = ["makgora", "makgora-client", "padbridge", "wisp", "firn", "north", "south", "gjoa", "fram", "wake", "battlecraft"];
const ALIASES: [(&str, &str); 3] = [("nixos-config", "firn"), ("smashcraft", "makgora"), ("smashcraft-client", "makgora-client")];
const NATIVE: [&str; 4] = ["life", "accounts", "travel", "ops"];

pub static CATALOG: LazyLock<Catalog> = LazyLock::new(|| Catalog {
    owner: OWNER.to_owned(),
    repos: REPOS.map(str::to_owned).to_vec(),
    native: NATIVE.map(str::to_owned).to_vec(),
    aliases: ALIASES.iter().map(|(alias, repo)| ((*alias).to_owned(), (*repo).to_owned())).collect(),
});

pub fn checkout(repo: &str) -> &str {
    crate::platform::repo_names::checkout_name(repo)
}

pub fn canonical_topics(topics: Vec<crate::platform::frozen_topics::Topic>) -> Vec<crate::platform::frozen_topics::Topic> {
    topics.into_iter().map(|mut topic| { topic.repo = CATALOG.canonical(&topic.repo).to_owned(); topic }).collect()
}

impl Catalog {
    pub fn is_native(&self, repo: &str) -> bool {
        self.native.iter().any(|name| name == repo)
    }

    pub fn canonical<'a>(&'a self, name: &'a str) -> &'a str {
        let name = name.strip_prefix(&format!("{}/", self.owner)).unwrap_or(name);
        let known = self.repos.iter().chain(&self.native).chain(self.aliases.keys()).find(|known| known.eq_ignore_ascii_case(name));
        let name = known.map_or(name, String::as_str);
        self.aliases.get(name).map_or(name, String::as_str)
    }

    /// `makgora#222`, `tompassarelli/makgora#222` or `nixos-config#3` as a repository or
    /// namespace and number.
    pub fn parse_ref(&self, text: &str) -> Result<(String, u64), String> {
        let malformed = || format!("{text} is not an issue ref like makgora#222");
        let (name, number) = text.trim().rsplit_once('#').ok_or_else(malformed)?;
        let number = number.parse::<u64>().ok().filter(|number| *number > 0).ok_or_else(malformed)?;
        let repo = self.canonical(name);
        if !self.repos.iter().chain(&self.native).any(|known| known == repo) {
            let known: Vec<&str> = self.repos.iter().chain(&self.native).map(String::as_str).collect();
            let name = name.strip_prefix(&format!("{}/", self.owner)).unwrap_or(name);
            return Err(format!("unknown repo {name} (known: {})", known.join(", ")));
        }
        Ok((repo.to_owned(), number))
    }
}

#[cfg(test)]
mod tests {
    use super::CATALOG;

    #[test]
    fn every_repo_the_fleet_staffs_is_accepted() {
        for (text, repo) in [("south#93", "south"), ("tompassarelli/south#85", "south"), ("smashcraft#277", "makgora"), ("smashcraft-client#4", "makgora-client")] {
            assert_eq!(CATALOG.parse_ref(text).map(|(repo, _)| repo), Ok(repo.to_owned()), "{text}");
        }
    }
}
