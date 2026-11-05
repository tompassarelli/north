use super::config::{Config, Kind, at_rev};
use super::report::{build, hand_of, inventory, now, texts};
use super::snapshot::{Snapshot, git, line_count};
use super::state::{Marker, State};
use north_base::foundation::clock::Timestamp;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|/)(tests?|__tests__)/|(^|/)tests\.rs$|\.(test|spec)\.[a-z]+$|_tests?\.(rs|ts|sh|py)$").unwrap());

fn hand_lines(repo: &Path, rev: &str, config: Option<&Config>) -> Result<usize, String> {
    let mut snapshot = Snapshot::new(repo, rev)?;
    let owned;
    let config = match config {
        Some(config) => config,
        None => {
            owned = at_rev(&snapshot, None)?;
            &owned
        }
    };
    let hand = hand_of(&inventory(&snapshot, config));
    Ok(texts(&mut snapshot, &hand)?.iter().map(|file| line_count(&file.text)).sum())
}

fn churn(repo: &Path, rev: &str, base: &str, config: &Config) -> Result<(usize, usize), String> {
    let kinds: HashMap<String, Kind> = inventory(&Snapshot::new(repo, rev)?, config).into_iter().collect();
    let listing = git(repo, &["diff", "--numstat", "-z", "--find-renames", base, rev])?;
    let fields: Vec<&str> = listing.split('\0').collect();
    let (mut added, mut tests) = (0, 0);
    let mut i = 0;
    while i < fields.len() {
        let mut parts = fields[i].split('\t');
        let (a, d, named) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let path = if named.is_empty() {
            i += 2;
            fields.get(i).copied().unwrap_or("")
        } else {
            named
        };
        i += 1;
        if path.is_empty() || a == "-" {
            continue;
        }
        let (a, d): (usize, usize) = (a.parse().unwrap_or(0), d.parse().unwrap_or(0));
        if kinds.get(path) == Some(&Kind::Hand) {
            added += a;
        }
        if TEST_PATH.is_match(path) {
            tests += a + d;
        }
    }
    Ok((added, tests))
}

pub(super) fn signal(state: &State, repo: &Path, config: &Config, now: Timestamp, rev: &str) -> Result<Vec<String>, String> {
    if !config.present {
        return Ok(Vec::new());
    }
    let Some(marker) = state.marker(repo)? else {
        return Ok(vec!["no review recorded (run code-health review-done after the first review)".to_owned()]);
    };
    let r = config.review;
    let mut reasons = Vec::new();
    let short = &marker.sha[..marker.sha.len().min(10)];
    let commits: usize = git(repo, &["rev-list", "--count", &format!("{}..{rev}", marker.sha)])?.trim().parse().unwrap_or(0);
    if r.commits > 0.0 && commits as f64 >= r.commits {
        reasons.push(format!("{commits} commits since review {short} (limit {})", r.commits));
    }
    let (added, tests) = churn(repo, rev, &marker.sha, config)?;
    if r.hand_lines_added > 0.0 && added as f64 >= r.hand_lines_added {
        reasons.push(format!("{added} hand lines added since review (limit {})", r.hand_lines_added));
    }
    if r.test_lines_changed > 0.0 && tests as f64 >= r.test_lines_changed {
        reasons.push(format!("{tests} test lines changed since review (limit {})", r.test_lines_changed));
    }
    let days = Timestamp::parse_rfc3339(&marker.at).map_or(f64::NAN, |at| (now.unix_millis() - at.unix_millis()) as f64 / 86_400_000.0);
    if r.days > 0.0 && days >= r.days && commits > 0 {
        reasons.push(format!("{} days since review with {commits} commits (limit {})", days.floor(), r.days));
    }
    if r.hotspot_score > 0.0
        && let Some(top) = build(repo, rev, config)?.hotspots.first().filter(|top| top.score as f64 > r.hotspot_score)
    {
        reasons.push(format!("hotspot {} score {} > {}", top.path, top.score, r.hotspot_score));
    }
    Ok(reasons)
}

pub(super) fn review_done(state: &State, repo: &Path, sha: &str, config: Option<&Config>) -> Result<(), String> {
    let full = git(repo, &["rev-parse", sha])?.trim().to_owned();
    let hand_lines = hand_lines(repo, &full, config)?;
    state.write_marker(repo, &Marker { sha: full, at: now(), hand_lines })
}
