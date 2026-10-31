use crate::foundation::clock::Timestamp;
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projects {
    repos: Vec<String>,
    pauses: BTreeMap<String, Pause>,
}

#[derive(Clone, Debug, PartialEq)]
struct Pause {
    project: String,
    until: f64,
    text: String,
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect()
}

impl Projects {
    pub fn read(cfg: &DocumentMut) -> Result<Self, String> {
        let private = strings(cfg.get("placement").and_then(|rules| rules.get("private_repos")));
        let mut named: Vec<(i64, String, &Item)> = cfg
            .get("projects")
            .and_then(Item::as_table_like)
            .into_iter()
            .flat_map(|projects| projects.iter())
            .map(|(name, project)| (project.get("priority").and_then(Item::as_integer).unwrap_or(i64::MAX), name.to_owned(), project))
            .collect();
        named.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
        let mut found = Self::default();
        for (_, name, project) in named {
            let staffed = project.get("dispatch_repos").or_else(|| project.get("repos"));
            for repo in strings(staffed).into_iter().filter(|repo| !private.contains(repo)) {
                if !found.repos.contains(&repo) {
                    found.repos.push(repo.clone());
                }
                if let Some(text) = project.get("dispatch_paused_until").and_then(Item::as_str) {
                    let until = Timestamp::parse_rfc3339(text).ok_or_else(|| format!("[projects.{name}] dispatch_paused_until {text:?} is not an RFC 3339 time"))?.unix_seconds_f64();
                    found.pauses.insert(repo, Pause { project: name.clone(), until, text: text.to_owned() });
                }
            }
        }
        if found.repos.is_empty() {
            return Err("orchestration names no [projects.NAME] repos to dispatch".into());
        }
        Ok(found)
    }

    pub fn repos(&self) -> &[String] {
        &self.repos
    }

    pub fn position(&self, repo: &str) -> Option<usize> {
        self.repos.iter().position(|named| named == repo)
    }

    pub fn paused(&self, repo: &str, at: f64) -> Option<String> {
        let pause = self.pauses.get(repo).filter(|pause| at < pause.until)?;
        Some(format!("project {} paused until {} (dispatch_paused_until)", pause.project, pause.text))
    }
}
