use regex::Regex;

use super::route::Route;

const SECRETS: &[&str] = &[
    r"\bsk-[A-Za-z0-9_-]{20,}",
    r"\bgh[pousr]_[A-Za-z0-9]{30,}",
    r"\bgithub_pat_[A-Za-z0-9_]{30,}",
    r"\bAKIA[0-9A-Z]{16}",
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    r"AGE-SECRET-KEY-[A-Z0-9]+",
    r"\bage1[0-9a-z]{58}\b",
    r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
    r"\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
];

/// Why a brief may not go to a route that can expose requests, or `Ok` when it names no private repository and carries no secret pattern.
pub fn refuse_for_free(route: Route, text: &str, private_repos: &[String]) -> Result<(), String> {
    for name in private_repos {
        let name = regex::escape(name);
        let reference = Regex::new(&format!(r"(?i)(?:[\w.-]+/{name}\b|~/code/{name}\b|/code/{name}/|\b{name}#\d+)")).map_err(|error| error.to_string())?;
        if let Some(found) = reference.find(text) {
            return Err(format!("the brief names a private repository ({:?}), and {}", found.as_str(), route.exposure()));
        }
    }
    for pattern in SECRETS {
        if Regex::new(pattern).map_err(|error| error.to_string())?.is_match(text) {
            return Err(format!("the brief carries a secret pattern, and {}", route.exposure()));
        }
    }
    Ok(())
}
