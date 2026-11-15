use std::path::{Path, PathBuf};

use north_landing::land::review_fetch as fetch_of;
use north_trusted_mirror::{lane_name_ok, plain_name};

use north_runtime_records::failure::{Failure, failed};
use north_runtime_records::hold_ledger::git;

pub(crate) const VERB: &str = "review-fetch";
const PROJECT: &str = "--project";
const USAGE: &str = "usage: north runtime review-fetch [--project NAME] LANE  fetch the HEAD of the lane LANE, named by its worktree, from the host that holds that worktree (this machine or a host the lander reads) into refs/review/LANE of the trusted mirror, which `north ln LANE` reads; a lane name is lowercase letters, digits and '-'; the fetch runs on the host that settles the fleet, and another host asks that host for it over its link to the ledger host (north#1610); --project names the project whose main checkout under ~/code the fetch is for, else it is the working directory's";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Fetch {
    pub lane: String,
    pub project: Option<String>,
}

impl Fetch {
    pub(crate) fn words(&self) -> Vec<String> {
        vec![VERB.to_owned(), self.lane.clone()]
    }

    pub(crate) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        Some(match rest {
            [lane] => lane_name_ok(lane).map(|()| Self { lane: (*lane).to_owned(), project: None }),
            _ => Err(format!("a review fetch request is `{VERB} LANE`")),
        })
    }

    /// The main checkout this fetch is for: the named project's under `~/code`, else the working directory's.
    pub(crate) fn main(&self) -> Result<PathBuf, String> {
        match &self.project {
            Some(project) => std::env::var_os("HOME").map(|home| PathBuf::from(home).join("code").join(project).join("main")).ok_or_else(|| "no home directory holds the project's checkout".to_owned()),
            None => main_here(),
        }
    }

    /// The words that ask the host settling the fleet for this fetch from another host, naming the project of `main`.
    pub(crate) fn forwarded(&self, main: &Path) -> Result<Vec<String>, String> {
        let project = main.parent().and_then(Path::file_name).and_then(|name| name.to_str()).filter(|name| plain_name(name)).ok_or_else(|| format!("{} is no main checkout of a project under ~/code", main.display()))?;
        Ok(["north", "runtime", VERB, PROJECT, project, &self.lane].map(str::to_owned).to_vec())
    }
}

pub(crate) fn run(flags: &[String], serve: impl FnOnce(&Fetch) -> Result<String, String>) -> u8 {
    command(flags, serve).map_or_else(
        |failure| {
            eprintln!("north runtime review-fetch: {}", failure.message);
            failure.code
        },
        |line| {
            if !line.is_empty() {
                println!("{line}");
            }
            0
        },
    )
}

fn command(flags: &[String], serve: impl FnOnce(&Fetch) -> Result<String, String>) -> Result<String, Failure> {
    let (project, lane) = match flags {
        [flag, project, lane] if flag == PROJECT && plain_name(project) => (Some(project.clone()), lane),
        [lane] => (None, lane),
        _ => return Err(failed(64, USAGE)),
    };
    if lane.starts_with('-') {
        return Err(failed(64, USAGE));
    }
    lane_name_ok(lane).map_err(|why| failed(64, why))?;
    serve(&Fetch { lane: lane.clone(), project }).map_err(|why| failed(1, why))
}

pub(crate) fn serve(main: &Path, fetch: &Fetch) -> Result<String, String> {
    let fetched = fetch_of::fetch(main, &fetch.lane)?;
    let mut lines = vec![format!("north runtime review-fetch: refs/review/{} is {} on {}", fetch.lane, fetched.sha, fetched.place)];
    lines.extend(fetched.swept);
    Ok(lines.join("\n"))
}

pub(crate) fn main_here() -> Result<PathBuf, String> {
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]).map_err(|failure| failure.message)?;
    Path::new(&common).parent().map(Path::to_path_buf).ok_or_else(|| format!("{common} has no checkout to fetch a lane for"))
}
