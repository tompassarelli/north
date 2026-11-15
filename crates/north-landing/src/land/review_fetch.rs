use std::path::{Component, Path, PathBuf};
use std::process::Command;

use north_trusted_mirror::{Mirror, ORIGIN_MAIN, command, forget_review, lane_name_ok, plain_name, review_head, review_lanes, review_ref};

use super::after::route::{self, SSH_OPTIONS, quote, ssh};
use super::setting;

pub struct Fetched {
    pub sha: String,
    pub place: String,
    pub swept: Vec<String>,
}

#[derive(Clone)]
struct Found {
    place: String,
    login: Option<String>,
    root: PathBuf,
}

type Searched = Vec<(String, Result<Vec<Found>, String>)>;

const SEARCH: &str = r#"for d in "$HOME"/code/"$1"/worktrees/*; do [ -e "$d/.git" ] && printf '%s\n' "$d"; done; true"#;

pub fn fetch(main: &Path, lane: &str) -> Result<Fetched, String> {
    lane_name_ok(lane)?;
    let project = project_of(main)?;
    let searched = search(main, &project);
    let found = choose(lane, &searched)?;
    if !worktree_ok(&project, lane, &found.root) {
        return Err(format!("{} on {} is not the worktree of lane {lane} in project {project}", found.root.display(), found.place));
    }
    let url = match &found.login {
        Some(login) => format!("ssh://{login}{}", found.root.display()),
        None => found.root.display().to_string(),
    };
    let fetched = fetch_from(main, lane, &url, &found.place)?;
    let swept = sweep(main, lane, &searched);
    Ok(Fetched { swept, ..fetched })
}

pub fn fetch_from(main: &Path, lane: &str, url: &str, place: &str) -> Result<Fetched, String> {
    lane_name_ok(lane)?;
    let store = Mirror::open(main)?.git_dir().to_owned();
    north_git_locks::retry_after_clearing(&store, || fetch_head(&store, url, lane))?;
    match review_head(main, lane) {
        Ok(Some(sha)) => Ok(Fetched { sha, place: place.to_owned(), swept: Vec::new() }),
        Ok(None) => Err(format!("{} holds no review ref after the fetch", review_ref(lane))),
        Err(why) => {
            let _ = forget_review(main, lane);
            Err(why)
        }
    }
}

fn project_of(main: &Path) -> Result<String, String> {
    main.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| plain_name(name))
        .map(str::to_owned)
        .ok_or_else(|| format!("{} is not a project's main checkout, so its lanes cannot be searched", main.display()))
}

fn search(main: &Path, project: &str) -> Searched {
    let mut searched = vec![("this machine".to_owned(), local(project).map(|roots| roots.into_iter().map(|root| Found { place: "this machine".to_owned(), login: None, root }).collect()))];
    for host in setting::outbox_hosts(main, ORIGIN_MAIN) {
        match route::outboxes(&host) {
            Ok(outboxes) => searched.extend(outboxes.into_iter().map(|outbox| {
                let place = outbox.name().to_owned();
                let outcome = ssh(&outbox.login, &format!("sh -c {} sh {}", quote(SEARCH), quote(project))).map(|text| roots(&text).into_iter().map(|root| Found { place: place.clone(), login: Some(outbox.login.clone()), root }).collect());
                (place, outcome)
            })),
            Err(why) => searched.push((host, Err(why))),
        }
    }
    searched
}

fn local(project: &str) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("sh").args(["-c", SEARCH, "sh", project]).output().map_err(|error| format!("sh: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(roots(&String::from_utf8_lossy(&output.stdout)))
}

fn roots(text: &str) -> Vec<PathBuf> {
    text.lines().map(str::trim).filter(|line| !line.is_empty()).map(PathBuf::from).collect()
}

fn choose(lane: &str, searched: &Searched) -> Result<Found, String> {
    let unsearched: Vec<String> = searched.iter().filter_map(|(place, outcome)| outcome.as_ref().err().map(|why| format!("{place} ({why})"))).collect();
    if !unsearched.is_empty() {
        return Err(format!("cannot search every host for the worktree {lane}, so none is fetched: {}", unsearched.join("; ")));
    }
    let mut found: Vec<Found> = searched.iter().filter_map(|(_, outcome)| outcome.as_ref().ok()).flatten().filter(|found| found.root.file_name().is_some_and(|name| name == lane)).cloned().collect();
    match found.len() {
        0 => Err(format!("no worktree named {lane} on this machine or on any host the lander reads")),
        1 => Ok(found.remove(0)),
        _ => Err(format!("worktrees named {lane} on {}; name the one to review", found.iter().map(|found| found.place.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

fn worktree_ok(project: &str, lane: &str, root: &Path) -> bool {
    root.is_absolute()
        && !root.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        && root.file_name().is_some_and(|name| name == lane)
        && root.parent().and_then(Path::file_name).is_some_and(|name| name == "worktrees")
        && root.parent().and_then(Path::parent).and_then(Path::file_name).is_some_and(|name| name == project)
        && root.to_str().is_some_and(|text| text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-')))
}

fn sweep(main: &Path, lane: &str, searched: &Searched) -> Vec<String> {
    let mut present = Vec::new();
    for (_, outcome) in searched {
        match outcome {
            Ok(found) => present.extend(found.iter().filter_map(|found| found.root.file_name().and_then(|name| name.to_str()).map(str::to_owned))),
            Err(_) => return Vec::new(),
        }
    }
    let Ok(reviewed) = review_lanes(main) else { return Vec::new() };
    reviewed
        .into_iter()
        .filter(|name| name != lane && !present.contains(name))
        .map(|name| match forget_review(main, &name) {
            Ok(()) => format!("swept the review ref of {name}, whose worktree is gone"),
            Err(why) => format!("could not sweep the review ref of {name}: {why}"),
        })
        .collect()
}

fn fetch_head(store: &Path, url: &str, lane: &str) -> Result<(), String> {
    let ssh_command = std::iter::once("ssh").chain(SSH_OPTIONS.iter().copied()).collect::<Vec<_>>().join(" ");
    let output = command(store).args(["fetch", "-q", "--no-tags", "--no-write-fetch-head", url, &format!("+HEAD:{}", review_ref(lane))]).env("GIT_SSH_COMMAND", ssh_command).output().map_err(|error| format!("git fetch {url}: {error}"))?;
    if !output.status.success() {
        return Err(format!("git fetch {url} failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}
