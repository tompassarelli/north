//! The child issue the outside broker files for a confined session, which holds no GitHub token: `issue child --title T --body B [--box TEXT] [--type K] [--label L]...` and nothing else of the caller's. The parent is the task issue the caller's own session record names, read from the fleet directory by the session whose scope the caller's cgroup names (lane_grant.rs), so a request that names a repository or a parent is refused. The broker runs its own north's `issue child` for that parent outside every domain, with its environment, its `PATH` behind the gh shim under the asking session's name and no standard input; the broker's token ceiling decides the write.

use std::path::Path;

use serde_json::Value;

use super::offload_request;
use super::check_request;
use super::lane_grant::Host;

const FLAGS: [&str; 5] = ["--title", "--body", "--box", "--type", "--label"];
const SHAPE: &str = "a confined session files a child of its own task issue with north issue child --title TITLE --body BODY [--box TEXT] [--type Task|Bug|Feature] [--label LABEL]..., naming no repository or parent";

/// The process that files a child: the asking session, the `north` arguments and the environment it runs with.
#[derive(Debug)]
pub(super) struct Granted {
    pub session: String,
    pub parent: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

/// The flags of the child `arguments` ask for: `None` for a request that is no `issue child`, and why one in other words is refused.
pub(super) fn asked(arguments: &[String]) -> Option<Result<Vec<String>, String>> {
    let [noun, verb, flags @ ..] = arguments else { return None };
    (noun == "issue" && verb == "child").then(|| {
        let (pairs, rest) = flags.as_chunks::<2>();
        let whole = rest.is_empty() && pairs.iter().all(|[flag, value]| FLAGS.contains(&flag.as_str()) && !value.trim().is_empty());
        let named = |name: &str| pairs.iter().any(|[flag, _]| flag == name);
        if whole && named("--title") && named("--body") { Ok(flags.to_vec()) } else { Err(SHAPE.to_owned()) }
    })
}

/// Whether `arguments` are an `issue child` in the shape the broker files.
pub(super) fn permitted(arguments: &[String]) -> bool {
    matches!(asked(arguments), Some(Ok(_)))
}

/// The process that files `flags` for the session whose record is `record`, under that session's own task issue, or why it is refused: the session was launched with no task, or the host holds no gh shim.
pub(super) fn granted(flags: Vec<String>, record: &Value, host: &Host, runtime: &Path, homes: &[(String, String)]) -> Result<Granted, String> {
    let field = |key: &str| record[key].as_str().filter(|value| !value.is_empty()).map(str::to_owned);
    let session = field("name").ok_or("the session record names no session")?;
    let parent = field("task_ref").ok_or_else(|| format!("{session} was launched with no --task, so it has no issue to file a child of"))?;
    let environment = check_request::environment(&session, offload_request::environment(&host.environment, runtime, homes, None)?, &host.home)?;
    let arguments = ["issue".to_owned(), "child".to_owned(), parent.clone()].into_iter().chain(flags).collect();
    Ok(Granted { session, parent, arguments, environment })
}
