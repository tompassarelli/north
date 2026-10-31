use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::error::NorthResult;
use crate::foundation::clock::{self, Timestamp};

use super::paths::owner_path;
use super::{REVIEW_STALE_DAYS, configuration, owner, string_field, units};

pub(super) fn skill_metadata(path: &Path) -> NorthResult<BTreeMap<String, String>> {
    let source = fs::read_to_string(path)?;
    let mut result = BTreeMap::new();
    let mut lines = source.lines();
    if lines.next() != Some("---") {
        return configuration(format!("skill has invalid frontmatter: {}", path.display()));
    }
    let frontmatter = lines.take_while(|line| *line != "---").collect::<Vec<_>>();
    let mut index = 0;
    while index < frontmatter.len() {
        if let Some((key, raw)) = frontmatter[index].split_once(':') {
            let raw = raw.trim();
            if [">", ">-", "|", "|-"].contains(&raw) {
                index += 1;
                let mut parts = Vec::new();
                while index < frontmatter.len()
                    && (frontmatter[index].starts_with(' ') || frontmatter[index].trim().is_empty())
                {
                    let text = frontmatter[index].trim();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                    index += 1;
                }
                result.insert(
                    key.into(),
                    parts.join(if raw.starts_with('>') { " " } else { "\n" }),
                );
                continue;
            }
            result.insert(key.into(), raw.trim_matches(['\'', '"']).into());
        }
        index += 1;
    }
    Ok(result)
}


const REVIEW_FIELDS: [&str; 2] = ["grounded", "written"];
const LOCAL_DATE_SLACK_DAYS: i64 = 1;

fn review_age(
    metadata: &BTreeMap<String, String>,
    field: &str,
    path: &Path,
    today: i64,
) -> NorthResult<i64> {
    let Some(reviewed) = metadata.get(field) else {
        return configuration(format!(
            "skill has no `{field}: YYYY-MM-DD` field: {}",
            path.display()
        ));
    };
    let Some(day) = clock::parse_day_number(reviewed) else {
        return configuration(format!(
            "skill {field} date is not YYYY-MM-DD: {}",
            path.display()
        ));
    };
    if day > today + LOCAL_DATE_SLACK_DAYS {
        return configuration(format!(
            "skill {field} date {reviewed} is in the future: {}",
            path.display()
        ));
    }
    Ok(today - day)
}

pub(super) fn check_reviews(metadata: &BTreeMap<String, String>, path: &Path) -> NorthResult<()> {
    let today = Timestamp::now().unix_days();
    for field in REVIEW_FIELDS {
        review_age(metadata, field, path, today)?;
    }
    Ok(())
}

pub(super) fn stale_skills(activation: &Value) -> NorthResult<Vec<Value>> {
    let today = Timestamp::now().unix_days();
    let mut stale = Vec::new();
    for unit in units(activation)? {
        if unit.get("kind").and_then(Value::as_str) != Some("skill") {
            continue;
        }
        let path = owner_path(owner(unit)?)?;
        let metadata = skill_metadata(&path)?;
        let mut due = Map::new();
        for field in REVIEW_FIELDS {
            let age = review_age(&metadata, field, &path, today)?;
            if age > REVIEW_STALE_DAYS {
                due.insert(
                    field.into(),
                    json!({"date": metadata[field], "ageDays": age}),
                );
            }
        }
        if !due.is_empty() {
            stale.push(json!({
                "id": string_field(unit, "id")?,
                "due": due,
                "path": path,
            }));
        }
    }
    stale.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    Ok(stale)
}

pub(super) fn print_stale(stale: &[Value]) {
    if stale.is_empty() {
        println!("no skill review is older than {REVIEW_STALE_DAYS} days");
        return;
    }
    println!("skill reviews older than {REVIEW_STALE_DAYS} days:");
    for entry in stale {
        let due = entry["due"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(field, review)| {
                format!(
                    "{field} {} ({} days)",
                    review["date"].as_str().unwrap_or_default(),
                    review["ageDays"]
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        println!("  {}: {due}", entry["id"].as_str().unwrap_or_default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aged(date: &str, today: &str) -> NorthResult<i64> {
        let metadata = BTreeMap::from([("grounded".to_owned(), date.to_owned())]);
        review_age(&metadata, "grounded", Path::new("SKILL.md"), clock::parse_day_number(today).unwrap())
    }

    #[test]
    fn a_skill_dated_with_the_local_day_ahead_of_utc_is_accepted_and_one_dated_two_days_ahead_is_refused() {
        assert_eq!(aged("2026-10-31", "2026-10-30").unwrap(), -1);
        assert!(aged("2026-11-01", "2026-10-30").is_err());
    }
}
