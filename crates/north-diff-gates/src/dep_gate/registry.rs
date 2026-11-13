use super::{Identity, LockCandidate, Parsed, Urgent, get, parse_json, required, string};
use crate::foundation::clock::Timestamp;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::time::Duration;

fn quote(text: &str) -> String {
    percent_encoding::utf8_percent_encode(text, percent_encoding::NON_ALPHANUMERIC).to_string()
}

pub trait Registry: Sync {
    fn read(&self, host: &str, path: &str) -> Result<String, &'static str>;
}

pub struct PublicRegistries;

impl Registry for PublicRegistries {
    fn read(&self, host: &str, path: &str) -> Result<String, &'static str> {
        read_registry(host, path)
    }
}

fn read_registry(host: &str, path: &str) -> Parsed<String> {
    let output = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "10",
            "--user-agent",
            "dep-gate (https://github.com/tompassarelli/north)",
            "--write-out",
            "\n%{http_code}",
            &format!("https://{host}{path}"),
        ])
        .output()
        .map_err(|_| "FileNotFoundError")?;
    if !output.status.success() {
        return Err("OSError");
    }
    let output = String::from_utf8(output.stdout).map_err(|_| "UnicodeDecodeError")?;
    let (body, status) = output.rsplit_once('\n').ok_or("HTTPException")?;
    if status != "200" {
        return Err("HTTPException");
    }
    Ok(body.to_owned())
}

pub(super) fn publish_times(
    candidates: &[LockCandidate],
    urgent: &Urgent,
    registry: &dyn Registry,
) -> Result<BTreeMap<Identity, f64>, String> {
    let mut groups: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for c in candidates {
        if c.ecosystem != "flake" && !urgent.contains(&(c.name.clone(), c.version.clone())) {
            groups
                .entry((c.ecosystem.clone(), c.name.clone()))
                .or_default()
                .insert(c.version.clone());
        }
    }
    let jobs: Vec<_> = groups.into_iter().collect();
    let api_read = std::sync::Mutex::new(None::<std::time::Instant>);
    std::thread::scope(|scope| {
        let mut output = BTreeMap::new();
        for batch in jobs.chunks(8) {
            let handles: Vec<_> = batch.iter().map(|((ecosystem,name),versions)| {
                let api_read = &api_read;
                scope.spawn(move || {
                    let fetch = || -> Parsed<BTreeMap<Identity,f64>> {
                        let data = if ecosystem == "npm" {
                            let data = parse_json(&registry.read("registry.npmjs.org",&format!("/{}",quote(name)))?)?;
                            required(&data,"time")?.clone()
                        } else {
                            let lower = name.to_lowercase();
                            let prefix = match lower.len() { 0..=2 => lower.len().to_string(), 3 => format!("3/{}",&lower[..1]), _ => format!("{}/{}",&lower[..2],&lower[2..4]) };
                            let body = registry.read("index.crates.io",&format!("/{prefix}/{}",quote(&lower)))?;
                            let mut rows = Map::new();
                            for line in body.lines() { let row = parse_json(line)?; rows.insert(string(required(&row,"vers")?)?.to_owned(),row); }
                            Value::Object(rows)
                        };
                        let mut result = BTreeMap::new();
                        for version in versions {
                            let row = required(&data,version)?;
                            let timestamp = if ecosystem == "npm" { row.clone() } else if !get(row,"pubtime").is_null() { row["pubtime"].clone() } else {
                                let mut last = api_read.lock().expect("registry rate lock");
                                if let Some(last) = *last { std::thread::sleep(Duration::from_secs(1).saturating_sub(last.elapsed())); }
                                *last = Some(std::time::Instant::now());
                                let response = parse_json(&registry.read("crates.io",&format!("/api/v1/crates/{}/{}",quote(name),quote(version)))?)?;
                                required(required(&response,"version")?,"created_at")?.clone()
                            };
                            let timestamp = string(&timestamp)?.replace('Z',"+00:00");
                            result.insert((ecosystem.clone(),name.clone(),version.clone()),Timestamp::parse_publish_time(&timestamp).ok_or("ValueError")?.unix_seconds_f64());
                        }
                        Ok(result)
                    };
                    fetch().map_err(|kind| format!("{ecosystem}: {name}: registry unavailable or invalid publish time; retry ({kind})"))
                })
            }).collect();
            for handle in handles {
                output.extend(handle.join().expect("registry reader thread")?);
            }
        }
        Ok(output)
    })
}
