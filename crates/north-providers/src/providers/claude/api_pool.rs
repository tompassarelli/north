use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use toml_edit::{DocumentMut, Item};

use crate::fleet::model::ApiKey;

const NEVER: &str = "9999-12-31";

/// One organization's entry in the pool of API keys: identified by its organization id, so two keys
/// that report the same id are one entry with one balance and one exhausted mark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Org {
    pub name: String,
    pub id: String,
    pub emails: Vec<String>,
    pub keys: Vec<PathBuf>,
    pub expires: Vec<String>,
}

impl Org {
    /// The earliest credit expiry not yet past on `today`, or `NEVER` when none is dated; an entry whose dated credit has all expired has none left.
    fn sorts_by(&self, today: &str) -> Option<&str> {
        let live = self.expires.iter().map(String::as_str).filter(|date| *date >= today).min();
        match (live, self.expires.is_empty()) {
            (Some(date), _) => Some(date),
            (None, true) => Some(NEVER),
            (None, false) => None,
        }
    }

    fn line(&self) -> String {
        format!("org={} id={} emails={} expires={}", self.name, self.id, self.emails.join(","), if self.expires.is_empty() { "unknown".to_owned() } else { self.expires.join(",") })
    }
}

/// The pool entries `[accounts.claude.api_orgs.NAME]` declares for `here`, entries sharing an id merged.
pub fn declared(policy: &DocumentMut, home: &Path, here: &str) -> Vec<Org> {
    let table = policy.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("api_orgs")).and_then(Item::as_table_like);
    let strings = |item: Option<&Item>| -> Vec<String> { item.and_then(Item::as_array).into_iter().flatten().filter_map(|value| value.as_str().map(str::to_owned)).collect() };
    let mut merged: BTreeMap<String, Org> = BTreeMap::new();
    for (name, entry) in table.into_iter().flat_map(|table| table.iter()) {
        let Some(id) = entry.get("id").and_then(Item::as_str) else { continue };
        let org = merged.entry(id.to_owned()).or_insert_with(|| Org { name: name.to_owned(), id: id.to_owned(), ..Org::default() });
        org.emails.extend(strings(entry.get("emails")));
        org.expires.extend(entry.get("grants").and_then(Item::as_array).into_iter().flatten().filter_map(|grant| grant.as_inline_table()?.get("expires")?.as_str().map(str::to_owned)));
        org.keys.extend(entry.get("keys").and_then(|keys| keys.get(here)).and_then(Item::as_array).into_iter().flatten().filter_map(|file| file.as_str().map(|path| expand(home, path))));
    }
    merged.into_values().collect()
}

fn expand(home: &Path, path: &str) -> PathBuf {
    path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

/// The entry to launch on: the one whose credit expires soonest among those not marked exhausted and holding a key `usable` reads, with that key.
pub fn open<'a>(orgs: &'a [Org], today: &str, exhausted: impl Fn(&Org) -> bool, usable: impl Fn(&Path) -> bool) -> Option<(&'a Org, ApiKey)> {
    let mut ranked: Vec<(&str, &Org)> = orgs.iter().filter(|org| !exhausted(org)).filter_map(|org| Some((org.sorts_by(today)?, org))).collect();
    ranked.sort_by(|(left, a), (right, b)| left.cmp(right).then_with(|| a.name.cmp(&b.name)));
    ranked.into_iter().find_map(|(_, org)| Some((org, ApiKey { org: org.name.clone(), file: org.keys.iter().find(|file| usable(file))?.clone() })))
}

/// Whether `org` is marked exhausted: a file named by its organization id in `marks`.
pub fn exhausted(marks: &Path, org: &Org) -> bool {
    marks.join(&org.id).exists()
}

/// Today's UTC date as `YYYY-MM-DD`.
pub fn today() -> String {
    let days = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| (elapsed.as_secs() / 86_400) as i64);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// One line per pool entry on `here`, then the entry a launch on the pooled account takes: what `north fleet accounts --api` prints.
pub fn listing(policy: &DocumentMut, home: &Path, state_root: &Path, here: &str) -> Vec<String> {
    let orgs = declared(policy, home, here);
    let marks = state_root.join("api-exhausted");
    let mut lines: Vec<String> = orgs.iter().map(|org| format!("{} keys={} exhausted={}", org.line(), org.keys.len(), exhausted(&marks, org))).collect();
    let chosen = open(&orgs, &today(), |org| exhausted(&marks, org), |file| std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.len() > 0));
    lines.push(chosen.map_or_else(|| "api launches on: no open entry".to_owned(), |(org, _)| format!("api launches on: {}", org.name)));
    lines
}
