use std::path::{Path, PathBuf};

use crate::land::credential;
use crate::land::lander::{self, Lander};

pub(super) struct Options {
    pub(super) once: bool,
    pub(super) all: bool,
    pub(super) repos: Vec<PathBuf>,
    pub(super) checker: Vec<String>,
    pub(super) depth: usize,
    pub(super) app_token: Vec<String>,
}

pub(super) fn parse(arguments: &[String]) -> Result<Options, ()> {
    let mut options = Options { once: false, all: false, repos: Vec::new(), checker: vec!["safe-push".to_owned()], depth: lander::DEPTH, app_token: credential::mint_program() };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.clone().next()) {
            ("--once", _) => options.once = true,
            ("--all", _) => options.all = true,
            ("--repo", Some(value)) => {
                options.repos.push(PathBuf::from(value));
                rest.next();
            }
            ("--checker", Some(value)) => {
                options.checker = vec![value.clone()];
                rest.next();
            }
            ("--app-token", Some(value)) => {
                options.app_token = vec![value.clone()];
                rest.next();
            }
            ("--depth", Some(value)) => {
                let Some(parsed) = value.parse::<usize>().ok().filter(|parsed| *parsed >= 1) else { return Err(()) };
                options.depth = parsed;
                rest.next();
            }
            _ => return Err(()),
        }
    }
    if options.repos.is_empty() && !options.all {
        options.repos.push(PathBuf::from("."));
    }
    Ok(options)
}

pub(super) fn lander_for(repo: &Path, options: &Options) -> Result<Lander, String> {
    let lander = Lander::new(repo, options.checker.clone(), options.depth)?;
    Ok(Lander { app_token: options.app_token.clone(), ..lander })
}
