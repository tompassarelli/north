use std::path::PathBuf;

use super::{State, strip};
use crate::platform::gh_shim;

const LOCAL: &[&str] = &["auth", "config", "help", "version", "completion", "alias", "extension", "--version", "--help", "-h"];
const WHY: &str = "every agent GitHub API call goes through north's GitHub broker, so identical reads share one call and GitHub's rate-limit holds are honored (north#638)";

fn github_api(argument: &str) -> bool {
    let rest = argument.strip_prefix("https://").or_else(|| argument.strip_prefix("http://")).unwrap_or(argument);
    rest.split(['/', '?']).next().is_some_and(|host| host == "api.github.com" || host == "api.github.com:443")
}

impl State<'_> {
    pub(super) fn gh_outside_broker(&self, program: &str, args: &[String]) -> Result<(), String> {
        let Some(sub) = args.first().map(|argument| strip(argument)) else {
            return Ok(());
        };
        if LOCAL.contains(&sub) {
            return Ok(());
        }
        let Some(binary) = self.gh_binary(program) else {
            return Ok(());
        };
        if gh_shim::is_shim(&binary) {
            return Ok(());
        }
        Err(format!(
            "raw 'gh {sub}' runs {} outside the GitHub broker: {WHY}; run plain `gh`, north's bin/gh shim",
            binary.display()
        ))
    }

    fn gh_binary(&self, program: &str) -> Option<PathBuf> {
        let program = strip(program.trim_start_matches(['(', '{']));
        if program.contains('/') {
            self.resolve(program, "").map(PathBuf::from)
        } else {
            gh_shim::first_on_path(&self.path)
        }
    }

    pub(super) fn github_http_outside_broker(&self, verb: &str, args: &[String]) -> Result<(), String> {
        if args.iter().any(|argument| github_api(strip(argument))) {
            return Err(format!("{verb} to api.github.com outside the GitHub broker: {WHY}; use `gh api` instead"));
        }
        Ok(())
    }
}
