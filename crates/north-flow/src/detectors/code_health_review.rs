use super::{Detector, Finding};

/// One repository whose `north code-health signal` fired, with the review marker it fired against.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub repo: String,
    pub since: String,
    pub reasons: Vec<String>,
    pub cost: Option<(f64, f64)>,
}

#[derive(Default)]
pub struct Facts {
    pub readings: Vec<Reading>,
}

pub fn reasons(stdout: &str) -> Vec<String> {
    stdout.lines().filter_map(|line| line.strip_prefix("FIRE ")).map(str::to_owned).collect()
}

/// One box per repository and review marker: the box stays filed until a review records a new marker.
pub fn due(facts: &Facts) -> Vec<Finding> {
    facts
        .readings
        .iter()
        .filter(|reading| !reading.reasons.is_empty())
        .map(|reading| {
            let repo = &reading.repo;
            let checkout = format!("~/code/{repo}/main");
            let measured = reading.cost.map_or_else(String::new, |(median, share)| format!(" Measured since the last review: median {median} hand files per feature commit, {:.1}% of feature commits reverted.", share * 100.0));
            Finding {
                detector: Detector::CodeHealthReview,
                key: format!("code-health-review:{repo}:{}", reading.since),
                owner: None,
                title: format!("Review the code health of {repo}"),
                detail: format!(
                    "`north code-health signal` fires for {repo} since its review marker {}: {}. Follow harness/code-health/review-brief.md with repo {repo} and checkout {checkout}, starting from `north code-health report --repo {checkout} --rev origin/main`, and land the bounded behavior-preserving slices it identifies.{measured}",
                    reading.since,
                    reading.reasons.join("; ")
                ),
                done_when: format!("`north code-health signal --repo {checkout} --rev origin/main | rg -qx quiet` exits 0, once the review slices have landed on {repo} main and `north code-health review-done --repo {checkout} --rev origin/main` has recorded the marker."),
                instances: reading.reasons.clone(),
            }
        })
        .collect()
}
