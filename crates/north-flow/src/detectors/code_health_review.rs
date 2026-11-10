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
    pub owner: Option<String>,
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
                owner: facts.owner.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(repo: &str, reasons: &[&str]) -> Reading {
        Reading { repo: repo.to_owned(), since: "6f03d49689".to_owned(), reasons: reasons.iter().map(|&reason| reason.to_owned()).collect(), cost: Some((4.0, 0.125)) }
    }

    #[test]
    fn a_fired_repository_is_one_box_on_the_owner_and_a_quiet_one_is_none() {
        let facts = Facts { owner: Some("north#722".to_owned()), readings: vec![reading("wisp", &["11 days since review with 40 commits (limit 3)"]), reading("makgora", &[])] };
        let found = due(&facts);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key, "code-health-review:wisp:6f03d49689");
        assert_eq!(found[0].owner.as_deref(), Some("north#722"));
        assert!(found[0].box_line().contains("11 days since review with 40 commits"));
        assert!(found[0].box_line().contains("median 4 hand files per feature commit, 12.5% of feature commits reverted"));
    }
}
