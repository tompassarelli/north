use std::collections::BTreeSet;
use std::env;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::Value;

use crate::error::{NorthError, NorthResult};

use super::configuration;
use super::live_refs::{DEFAULT_MAX_AGE_MS, Report, judge, now_ms, parse_duration_ms};
use super::live_refs_transport as transport;
use super::registration_checks::{canonical, fetch_main, git_object, git_text, landing_hook_names};
use super::retired_hooks;

const CATALOG: &str = "agents/catalog-config.json";
const BASE: &str = "origin/main";

pub fn dropped_hooks(base: &Value, candidate: &Value, retired: &str) -> BTreeSet<String> {
    let kept = landing_hook_names(candidate);
    let retired: BTreeSet<&str> = retired_hooks::names(retired).collect();
    landing_hook_names(base).into_iter().filter(|name| !kept.contains(name) && !retired.contains(name.as_str())).collect()
}

pub fn refusal(reports: &[(String, Report)]) -> Option<String> {
    let lines: Vec<String> = reports
        .iter()
        .filter(|(_, report)| !report.is_clean())
        .map(|(name, report)| {
            format!("{name}: still named: [{}]; unknown, so refused: [{}]; fact ages: {}", report.naming.join("; "), report.unknown.join("; "), report.ages.join(", "))
        })
        .collect();
    (!lines.is_empty()).then(|| format!("this catalog drops hooks that recorded hosts may still register: {}", lines.join(" | ")))
}

fn catalog_at(root: &Path, reference: &str) -> NorthResult<Value> {
    Ok(serde_json::from_slice(&git_object(root, reference, CATALOG)?)?)
}

#[derive(Parser)]
#[command(no_binary_name = true, disable_help_flag = true, args_override_self = true)]
struct Options {
    #[arg(long = "dropped-by-catalog")]
    _dropped_by_catalog: bool,
    #[arg(long, value_name = "DIR", allow_hyphen_values = true)]
    south_root: Option<PathBuf>,
    #[arg(long, value_name = "DIR", allow_hyphen_values = true)]
    north_root: Option<PathBuf>,
    #[arg(long, value_name = "DIR", allow_hyphen_values = true)]
    repo: Option<PathBuf>,
    #[arg(long, allow_hyphen_values = true, default_value = "origin")]
    remote: String,
    #[arg(long, allow_hyphen_values = true)]
    max_age: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    hosts: Option<String>,
}

pub(super) fn run(arguments: &[String]) -> NorthResult<()> {
    let options: Options = crate::arguments::parse(arguments).map_err(NorthError::Usage)?;
    let remote = options.remote;
    let max_age = options.max_age.as_deref().map_or(Ok(DEFAULT_MAX_AGE_MS), parse_duration_ms)?;
    let hosts: Vec<String> = options.hosts.as_deref().unwrap_or_default().split(',').filter(|host| !host.is_empty()).map(str::to_owned).collect();
    let (Some(south), Some(north), Some(repo)) = (options.south_root, options.north_root, options.repo) else {
        return Err(NorthError::Usage("live-refs --dropped-by-catalog needs --south-root DIR --north-root DIR --repo DIR".into()));
    };
    let current_root = git_text(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    let head = env::var("SAFE_PUSH_LAND_HEAD").or_else(|_| git_text(Path::new("."), &["rev-parse", "HEAD"])).map_err(|error| NorthError::Configuration(error.to_string()))?;
    let candidate_ref = if canonical(&south)? == current_root { head } else { fetch_main(&south)? };
    let retired = String::from_utf8(git_object(&north, BASE, retired_hooks::LIST)?).map_err(|error| NorthError::Configuration(error.to_string()))?;
    let dropped = dropped_hooks(&catalog_at(&south, BASE)?, &catalog_at(&south, &candidate_ref)?, &retired);
    if dropped.is_empty() {
        println!("catalog drops no hook distribution");
        return Ok(());
    }
    let all = transport::fetch_all(&repo, &remote)?;
    let reports: Vec<(String, Report)> = dropped.into_iter().map(|name| { let report = judge(&all, &name, &hosts, now_ms(), max_age); (name, report) }).collect();
    match refusal(&reports) {
        Some(message) => configuration(message),
        None => {
            println!("catalog drops {}; no recorded host registers them", reports.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", "));
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "live_refs_drops_tests.rs"]
mod tests;
