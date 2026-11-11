//! The well-specified band (north#1210): the boxes dispatch draws on the outside provider when its scope admits that
//! provider and not Claude. Every other box keeps its Claude routing and waits, and no box on a private repository is in it.
use super::facets;
use super::issues::Issue;
use crate::pacing::placement::gate;
use toml_edit::DocumentMut;

pub const CATEGORIES: [&str; 5] = ["mechanical", "docs-policy", "tooling", "bug-known-cause", "feature"];
pub const SPECS: [&str; 2] = ["exact", "named"];
pub const SCOPES: [&str; 2] = ["one-file", "module"];

/// Why `issue`'s box, routed in `category`, stays out of the band, or `None` when the outside provider may take it.
pub fn outside(issue: &Issue, category: &str, cfg: &DocumentMut) -> Option<String> {
    if !gate::public(&issue.repo, cfg) {
        return Some(format!("{} is not a public repository", issue.repo));
    }
    if facets::category(issue).is_none() {
        return Some("Category: not declared by the issue".to_owned());
    }
    if !CATEGORIES.contains(&category) {
        return Some(format!("Category: {category} is outside the band ({})", CATEGORIES.join(", ")));
    }
    let Some(spec) = facets::declared(issue, "Spec").map(str::to_lowercase) else {
        return Some("Spec: not declared by the issue".to_owned());
    };
    if !SPECS.contains(&spec.as_str()) {
        return Some(format!("Spec: {spec} is outside the band ({})", SPECS.join(", ")));
    }
    let Some(scope) = facets::declared(issue, "Scope").map(str::to_lowercase) else {
        return Some("Scope: not declared by the issue".to_owned());
    };
    if !SCOPES.contains(&scope.as_str()) {
        return Some(format!("Scope: {scope} is wider than one module"));
    }
    None
}
