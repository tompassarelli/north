use super::Refusal;

const THREADS_VERBS: [&str; 26] = [
    "show", "ready", "list", "unowned", "run-ids", "summary", "recommend", "replay-staffing", "planner-band", "box-eta",
    "job-runs", "recommend-runs", "calibration-runs", "calibrate", "job-run", "queue", "claim", "release", "need", "unneed",
    "new", "note", "close", "reopen", "block", "review-finding",
];
const THREADS_UNBLOCK: &str = "unblock";
const NORTH_ONLY_THREADS: [&str; 2] = ["ingest", "snapshot-pages"];
const FLEET_ROW: [&str; 5] = ["put", "wait", "exchange", "settle", "register"];

pub(super) fn authorize(argv: &[String]) -> Result<String, Refusal> {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let refused = || Refusal::Verb(words.iter().take(3).copied().collect::<Vec<_>>().join(" "));
    let verb = match words.as_slice() {
        ["threads", rest @ ..] => threads(rest, false).map(|name| format!("threads {name}")),
        ["north", "threads", rest @ ..] => threads(rest, true).map(|name| format!("threads {name}")),
        ["north", "fleet", "row", "leased", _session] => Some("fleet row leased".to_string()),
        ["north", "fleet", "row", verb, ..] if FLEET_ROW.contains(verb) => Some(format!("fleet row {verb}")),
        ["north", "laptop", "status"] | ["north", "laptop", "status", "--json"] => Some("laptop status".to_string()),
        ["agents", "usage", "--refresh", "--local", "--json"] => Some("agents usage".to_string()),
        ["readlink", ".local/state/north/current"] => Some("readlink current".to_string()),
        ["north", "runtime", "health"] => Some("runtime health".to_string()),
        ["south", "land"] => Some("south land".to_string()),
        _ => None,
    };
    verb.ok_or_else(refused)
}

fn threads(rest: &[&str], north: bool) -> Option<&'static str> {
    let (verb, arguments) = rest.split_first()?;
    if *verb == "calibrate" && arguments.contains(&"--write") {
        return None;
    }
    let named = |names: &'static [&'static str]| names.iter().find(|name| *name == verb).copied();
    named(&THREADS_VERBS)
        .or_else(|| (*verb == THREADS_UNBLOCK).then_some(THREADS_UNBLOCK))
        .or_else(|| north.then(|| named(&NORTH_ONLY_THREADS)).flatten())
}
