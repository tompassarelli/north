use std::path::{Path, PathBuf};

use north_landing::land::review_fetch as fetch_of;
use north_trusted_mirror::lane_name_ok;

use super::failure::{Failure, failed};
use super::review_hold::git;

pub(crate) const VERB: &str = "review-fetch";
const USAGE: &str = "usage: north runtime review-fetch LANE  fetch the HEAD of the lane LANE, named by its worktree, from the host that holds that worktree (this machine or a host the lander reads) into refs/review/LANE of the trusted mirror, which `north ln LANE` reads; a lane name is lowercase letters, digits and '-', and the fetch runs only on the host that settles the fleet";

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Fetch {
    pub lane: String,
}

impl Fetch {
    pub(crate) fn words(&self) -> Vec<String> {
        vec![VERB.to_owned(), self.lane.clone()]
    }

    pub(crate) fn asked(arguments: &[String]) -> Option<Result<Self, String>> {
        let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
        let [VERB, rest @ ..] = words.as_slice() else { return None };
        Some(match rest {
            [lane] => lane_name_ok(lane).map(|()| Self { lane: (*lane).to_owned() }),
            _ => Err(format!("a review fetch request is `{VERB} LANE`")),
        })
    }
}

pub(crate) fn run(flags: &[String], settles: bool, serve: impl FnOnce(&Fetch) -> Result<String, String>) -> u8 {
    command(flags, settles, serve).map_or_else(
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

fn command(flags: &[String], settles: bool, serve: impl FnOnce(&Fetch) -> Result<String, String>) -> Result<String, Failure> {
    let [lane] = flags else { return Err(failed(64, USAGE)) };
    if lane.starts_with('-') {
        return Err(failed(64, USAGE));
    }
    if !settles {
        return Err(failed(64, format!("the host that settles the fleet holds the trusted mirror that a review fetch writes, and this is not it\n{USAGE}")));
    }
    lane_name_ok(lane).map_err(|why| failed(64, why))?;
    serve(&Fetch { lane: lane.clone() }).map_err(|why| failed(1, why))
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
