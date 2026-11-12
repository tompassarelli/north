use crate::foundation::clock::Timestamp;
use std::collections::BTreeMap;
use toml_edit::{DocumentMut, Item};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projects {
    repos: Vec<String>,
    pauses: BTreeMap<String, Pause>,
    set_aside: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Pause {
    project: String,
    until: f64,
    reason: String,
}

fn strings(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect()
}

impl Projects {
    pub fn read(cfg: &DocumentMut) -> Result<Self, String> {
        let canonical = |names: Vec<String>| names.into_iter().map(|name| crate::threads::catalog::CATALOG.canonical(&name).to_owned()).collect::<Vec<_>>();
        let private = canonical(strings(cfg.get("placement").and_then(|rules| rules.get("private_repos"))));
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
            let set_aside = project.get("paused").and_then(Item::as_bool).unwrap_or(false);
            if set_aside {
                found.set_aside.push(name.clone());
            }
            let staffed = project.get("dispatch_repos").or_else(|| project.get("repos"));
            for repo in canonical(strings(staffed)).into_iter().filter(|repo| !private.contains(repo)) {
                if !found.repos.contains(&repo) {
                    found.repos.push(repo.clone());
                }
                if set_aside {
                    found.pauses.insert(repo, Pause { project: name.clone(), until: f64::INFINITY, reason: "paused = true".into() });
                } else if let Some(text) = project.get("dispatch_paused_until").and_then(Item::as_str) {
                    let until = Timestamp::parse_rfc3339(text).ok_or_else(|| format!("[projects.{name}] dispatch_paused_until {text:?} is not an RFC 3339 time"))?.unix_seconds_f64();
                    found.pauses.insert(repo, Pause { project: name.clone(), until, reason: format!("until {text} (dispatch_paused_until)") });
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
        Some(format!("project {} paused {}", pause.project, pause.reason))
    }

    /// The projects with `paused = true`, private ones included, in priority order.
    pub fn set_aside(&self) -> &[String] {
        &self.set_aside
    }
}
