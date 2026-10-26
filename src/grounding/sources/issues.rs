use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};
pub fn ledger_path() -> PathBuf {
    if let Some(p) = env::var_os("THREADS_DB") {
        return p.into();
    }
    let state = env_path("XDG_STATE_HOME", home().join(".local/state")).join("threads");
    state.join(if home().join(".config/threads/remote").exists() {
        "snapshot.db"
    } else {
        "threads.db"
    })
}
fn event_line(ts: &str, kind: &str, actor: &str, data: &Value) -> String {
    let at = cut(ts, 16);
    match kind {
        "handoff" => format!("{at} handoff {} -> {}", s(data, "from"), s(data, "to")),
        "claim" | "release" | "capture" | "close" | "reopen" => {
            format!("{at} {kind} {actor}").trim_end().into()
        }
        "need" | "unneed" => format!("{at} {kind} {}", s(data, "needs")),
        _ => {
            let said = ["text", "block", "body"]
                .into_iter()
                .map(|k| s(data, k))
                .find(|t| !t.is_empty());
            said.map(|t| format!("{at} {kind} {actor}: {t}"))
                .unwrap_or_default()
        }
    }
}
pub fn threads(since: Option<f64>, path: &Path, visible_native: bool) -> Result<Vec<Doc>> {
    let native = CATALOG.native.clone();
    let mut db = Sqlite::open(path, true)?;
    db.exec("PRAGMA busy_timeout=2000;")?;
    if path.ends_with("snapshot.db") {
        let check = db.query("PRAGMA quick_check;")?;
        if check.len() != 1 || check[0]["quick_check"] != "ok" {
            return Err(format!(
                "threads snapshot {} failed its integrity check",
                path.display()
            )
            .into());
        }
    }
    let mut titles = HashMap::new();
    for row in db.query("select repo,issues from gh_cache;")? {
        let issues: Value = serde_json::from_str(&s(&row, "issues"))?;
        if let Some(issues) = issues.as_object() {
            for (number, issue) in issues {
                titles.insert(format!("{}#{number}", s(&row, "repo")), s(issue, "title"));
            }
        }
    }
    let mut trail = HashMap::<String, Vec<(f64, String)>>::new();
    let mut latest = BTreeMap::<String, f64>::new();
    let mut note = |item: String, at: f64, line: String| {
        if !line.is_empty() {
            trail.entry(item.clone()).or_default().push((at, line));
        }
        let last = latest.entry(item).or_default();
        *last = last.max(at);
    };
    for row in
        db.query("select ts,item,kind,actor,data from events where item like '%#%' order by seq;")?
    {
        let ts = s(&row, "ts");
        let data: Value = serde_json::from_str(&s(&row, "data"))?;
        note(
            s(&row, "item"),
            epoch(&ts),
            event_line(&ts, &s(&row, "kind"), &s(&row, "actor"), &data),
        );
    }
    for row in
        db.query("select item,ended,outcome,tier,report,note from runs where item like '%#%';")?
    {
        let ended = s(&row, "ended");
        let run_note = s(&row, "note");
        note(
            s(&row, "item"),
            epoch(&ended),
            format!(
                "{} run {} {}: {}{}",
                cut(&ended, 16),
                s(&row, "tier"),
                s(&row, "outcome"),
                s(&row, "report"),
                if run_note.is_empty() {
                    String::new()
                } else {
                    format!(" (note {run_note})")
                }
            ),
        );
    }
    let blocks: HashMap<_, _> = db
        .query("select item,reason from blocks;")?
        .into_iter()
        .map(|r| (s(&r, "item"), s(&r, "reason")))
        .collect();
    let claims: HashMap<_, _> = db
        .query("select item,owner from claims;")?
        .into_iter()
        .map(|r| (s(&r, "item"), s(&r, "owner")))
        .collect();
    let items = db.query("select ns,number,title,body,state,created,closed,due from items;")?;
    let status = |item: &str| {
        let mut lines = Vec::new();
        if let Some(owner) = claims.get(item) {
            lines.push(format!("owner: {owner}"));
        }
        if let Some(reason) = blocks.get(item) {
            lines.push(format!("blocked: {reason}"));
        }
        lines
    };
    let recent = |item: &str| {
        let mut t = trail.get(item).cloned().unwrap_or_default();
        t.sort_by(|a, b| b.0.total_cmp(&a.0));
        t.into_iter().map(|(_, l)| l).collect::<Vec<_>>()
    };
    let mut seen = HashSet::new();
    let mut docs = Vec::new();
    for row in items {
        let ns = s(&row, "ns");
        let item = format!("{ns}#{}", s(&row, "number"));
        seen.insert(item.clone());
        if !visible_native || !native.contains(&ns) {
            continue;
        }
        let at = epoch(&s(&row, "created"))
            .max(epoch(&s(&row, "closed")))
            .max(*latest.get(&item).unwrap_or(&0.0));
        if since.is_some_and(|s| at <= s) {
            continue;
        }
        let title = s(&row, "title");
        let due = s(&row, "due");
        let mut head = vec![
            title.clone(),
            format!(
                "state: {}{}",
                s(&row, "state"),
                if due.is_empty() {
                    String::new()
                } else {
                    format!(", due {due}")
                }
            ),
            s(&row, "body"),
        ];
        head.extend(status(&item));
        head.extend(recent(&item));
        docs.push(doc(
            "threads",
            item.clone(),
            format!("threads {item}"),
            title,
            clipped(
                &head
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                2000,
            ),
            at,
            row["state"] == "closed",
        ));
    }
    for (item, at) in latest {
        if seen.contains(&item)
            || native.iter().any(|n| item.starts_with(&format!("{n}#")))
            || since.is_some_and(|s| at <= s)
        {
            continue;
        }
        let title = titles.get(&item).cloned().unwrap_or_else(|| item.clone());
        let mut head = vec![
            title.clone(),
            format!(
                "state: {}",
                if titles.contains_key(&item) {
                    "open"
                } else {
                    "not open"
                }
            ),
        ];
        head.extend(status(&item));
        head.extend(recent(&item));
        docs.push(doc(
            "threads",
            item.clone(),
            format!("threads {item}"),
            title,
            clipped(
                &head
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                2000,
            ),
            at,
            !titles.contains_key(&item),
        ));
    }
    Ok(docs)
}
fn graphql_query(repos: &[String]) -> String {
    let params = repos
        .iter()
        .enumerate()
        .map(|(i, _)| format!(", $c{i}: String"))
        .collect::<String>();
    let parts=repos.iter().enumerate().map(|(i,repo)|format!(" r{i}: repository(owner: $owner, name: \"{repo}\") {{ issues(first: 100, after: $c{i}, orderBy: {{field: UPDATED_AT, direction: ASC}}, filterBy: {{since: $since}}) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ number title body state updatedAt labels(first: 20) {{ nodes {{ name }} }} }} }} }}")).collect::<String>();
    format!("query($owner: String!, $since: DateTime{params}) {{{parts} }}")
}
pub fn github_with(
    since: Option<f64>,
    mut run: impl FnMut(&[String]) -> Result<String>,
) -> Result<Vec<Doc>> {
    let (owner, repos) = (CATALOG.owner.clone(), CATALOG.repos.clone());
    let mut cursors: Vec<(String, Option<String>)> = repos.into_iter().map(|r| (r, None)).collect();
    let mut docs = Vec::new();
    while !cursors.is_empty() {
        let repos: Vec<_> = cursors.iter().map(|(r, _)| r.clone()).collect();
        let mut args = vec![
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={}", graphql_query(&repos)),
            "-F".into(),
            format!("owner={owner}"),
        ];
        if let Some(since) = since {
            let at = re(r"\.\d+Z$").replace(&Timestamp::from_unix_seconds_f64(since).iso_millis(), "Z").into_owned();
            args.extend(["-F".into(), format!("since={at}")]);
        }
        for (i, (_, cursor)) in cursors.iter().enumerate() {
            if let Some(cursor) = cursor {
                args.extend(["-F".into(), format!("c{i}={cursor}")]);
            }
        }
        let data: Value = serde_json::from_str(&run(&args)?)?;
        let mut next = Vec::new();
        for (i, (repo, _)) in cursors.into_iter().enumerate() {
            let page = &data["data"][format!("r{i}")]["issues"];
            let nodes = page["nodes"]
                .as_array()
                .ok_or("github: missing issue page")?;
            for node in nodes {
                let at = epoch(&s(node, "updatedAt"));
                if since.is_some_and(|s| at <= s) {
                    continue;
                }
                let labels = node["labels"]["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|l| s(l, "name"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let mut parts = vec![
                    s(node, "title"),
                    format!("state: {}", s(node, "state").to_lowercase()),
                ];
                if !labels.is_empty() {
                    parts.push(format!("labels: {labels}"));
                }
                let body = s(node, "body");
                if !body.is_empty() {
                    parts.push(body);
                }
                let item = format!("{owner}/{repo}#{}", s(node, "number"));
                docs.push(doc(
                    "github",
                    item.clone(),
                    format!("github {item}"),
                    s(node, "title"),
                    clipped(&parts.join("\n"), 2000),
                    at,
                    node["state"] != "OPEN",
                ));
            }
            if page["pageInfo"]["hasNextPage"] == true {
                next.push((
                    repo,
                    page["pageInfo"]["endCursor"].as_str().map(str::to_owned),
                ));
            }
        }
        cursors = next;
    }
    Ok(docs)
}
pub fn github(since: Option<f64>) -> Result<Vec<Doc>> {
    github_with(since, |args| {
        let mut delay = 30.0f64;
        let mut waited = 0.0;
        loop {
            let out = crate::platform::github::run_owned(args)?;
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
            }
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.to_lowercase().contains("rate limit") || waited > 65.0 * 60.0 {
                return Err(format!("gh api graphql: {}", err.trim()).into());
            }
            let jitter = Timestamp::now().unix_seconds_f64().fract() * 0.4 + 0.8;
            let pause = delay * jitter;
            eprintln!("grounding github: GitHub rate limit, waiting {pause:.0} s");
            std::thread::sleep(std::time::Duration::from_secs_f64(pause));
            waited += pause;
            delay = (delay * 2.0).min(300.0);
        }
    })
}
