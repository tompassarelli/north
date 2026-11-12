use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::pin;
use super::world::{GIVEN, NOT_RUN, beyond_files, judged};

pub const RULE: &str = "A case is neutral when none of its checks names a tool only T3 Code has. A check names one when (a) one of its tool-name fields (calls, distinctCalls, order, minCalls and endsTurnAfter, which require a call; callsMustNot, notCalls and maxCalls, which forbid or bound one) holds a tool T3 Code's MCP server defines, which in the case files is every lower-case name, each found under apps/server/src/mcp/toolkits, while Claude Code's own tools are capitalised; or (b) it is a check whose grader is built on T3 Code's tools or thread state: newThreads (t3_thread_launch, create_threads), linked and linkedNumbers (pull requests linked to a T3 Code thread), device (device_*), secret (request_secret). A case is runnable in both arms when a real session can be given its world: files alone (no mocked tool or shell result, no earlier turn, thread context or side chat, no file outside the repository), the top role, not Codex-only and a prompt of its own. A case asks for something without T3 Code's tools when it requires no T3 Code tool, has no check on T3 Code's thread state, and at least one check asks the agent to produce something (a call of a tool both harnesses have, a pattern its text or shell command must match, or working file links); a case whose only checks forbid things is passed by doing nothing.";

pub const JUDGED: &str = "Two fields are judged case by case and are not the rule's: runnable_given names what a real session in either harness is given besides the case's files so that the case can run alike in both, and not_run names why a case's world cannot be given alike in both harnesses. A case with neither and a world beyond files waits on a mechanism that is not built.";

const TOOL_CHECKS: [&str; 8] = ["calls", "distinctCalls", "order", "minCalls", "endsTurnAfter", "callsMustNot", "notCalls", "maxCalls"];
const REQUIRING: usize = 5;
const STATE_CHECKS: [&str; 5] = ["newThreads", "linked", "linkedNumbers", "device", "secret"];
const MUST: [&str; 3] = ["shellMust", "textMust", "finalTextMust"];
const MUST_NOT: [&str; 2] = ["shellMustNot", "textMustNot"];

fn t3_only(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_lowercase())
}

fn names(check: &Value) -> BTreeSet<String> {
    let listed: Vec<&str> = match check {
        Value::Array(items) => items.iter().filter_map(|item| item.as_str().or_else(|| item["tool"].as_str())).collect(),
        Value::Object(map) => map.keys().map(String::as_str).collect(),
        Value::String(text) => vec![text.as_str()],
        _ => Vec::new(),
    };
    listed.into_iter().flat_map(|tool| tool.split('|')).map(str::to_owned).collect()
}

fn named(expect: &Value) -> (BTreeSet<String>, BTreeSet<String>) {
    let of = |keys: &[&str]| keys.iter().flat_map(|key| names(&expect[*key])).collect();
    (of(&TOOL_CHECKS[..REQUIRING]), of(&TOOL_CHECKS[REQUIRING..]))
}

fn only_t3(names: BTreeSet<String>) -> Vec<String> {
    names.into_iter().filter(|name| t3_only(name)).collect()
}

fn has_word(text: &str, word: &str) -> bool {
    let inside = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| !text[..at].ends_with(inside) && !text[at + word.len()..].starts_with(inside))
}

fn world(case: &Value) -> Vec<String> {
    let setup = &case["setup"];
    let mut needs: BTreeSet<String> = names(&setup["mocks"]).into_iter().filter(|name| t3_only(name)).map(|name| format!("mock {name}")).collect();
    let prior = setup["priorTurns"].as_array().into_iter().flatten().flat_map(|turn| turn["toolCalls"].as_array().into_iter().flatten());
    needs.extend(prior.filter_map(|call| call["tool"].as_str()).filter(|name| t3_only(name)).map(|name| format!("prior call {name}")));
    if !setup["sideChat"].is_null() {
        needs.insert("side chat".to_owned());
    }
    if setup["role"] != "top" {
        needs.insert(format!("role {}", setup["role"].as_str().unwrap_or_default()));
    }
    if setup["harness"] == "codex" {
        needs.insert("Codex only".to_owned());
    }
    if case["prompt"].as_str().is_none_or(str::is_empty) {
        needs.insert("no prompt of its own".to_owned());
    }
    needs.into_iter().collect()
}

fn entry(case: &Value, file: &str) -> Value {
    let expect = &case["expect"];
    let (requiring, forbidding) = named(expect);
    let present = |keys: &[&str]| -> Vec<String> { keys.iter().copied().filter(|key| !expect[*key].is_null()).map(str::to_owned).collect() };
    let links = (expect["fileLinks"]["minWorking"].as_u64().unwrap_or(0) > 0).then(|| "fileLinks".to_owned());
    let unlinked = (!expect["fileLinks"].is_null() && links.is_none()).then(|| "fileLinks".to_owned());
    let asked: Vec<String> = requiring.iter().filter(|name| !t3_only(name)).cloned().chain(present(&MUST)).chain(links).collect();
    let barred: Vec<String> = forbidding.iter().filter(|name| !t3_only(name)).cloned().chain(present(&MUST_NOT)).chain(unlinked).collect();
    let (required, forbidden) = (only_t3(requiring), only_t3(forbidding));
    let state: Vec<&str> = STATE_CHECKS.iter().copied().filter(|key| !expect[*key].is_null()).collect();
    let world = world(case);
    let beyond = beyond_files(case);
    let neutral = required.is_empty() && forbidden.is_empty() && state.is_empty();
    let id = case["id"].as_str().unwrap_or_default();
    json!({
        "file": file, "kind": case["kind"], "area": case["area"], "neutral": neutral, "runnable_in_both_arms": world.is_empty() && beyond.is_empty(), "replays_prior_turns": !case["setup"]["priorTurns"].is_null(),
        "requires_t3_tools": required, "forbids_t3_tools": forbidden, "t3_state_checks": state, "world_needs_t3": world,
        "asks_without_t3_tools": asked, "bars_without_t3_tools": barred, "world_beyond_files": beyond,
        "runnable_given": judged(&GIVEN, id), "not_run": judged(&NOT_RUN, id),
    })
}

fn undefined(names: &BTreeSet<String>, checkout: &Path) -> Result<Vec<String>, String> {
    let mut source = String::new();
    let mut pending = vec![checkout.join("apps/server/src/mcp/toolkits")];
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "ts") {
                source.push_str(&fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?);
            }
        }
    }
    Ok(names.iter().filter(|name| !has_word(&source, name)).cloned().collect())
}

fn count(cases: &Map<String, Value>, keep: impl Fn(&Value) -> bool) -> Value {
    let kept: Vec<&Value> = cases.values().filter(|case| keep(case)).collect();
    let of = |kind: &str| kept.iter().filter(|case| case["kind"] == kind).count();
    json!({"cases": kept.len(), "bad": of("bad"), "guard": of("guard"), "control": of("control")})
}

fn empty(value: &Value) -> bool {
    value.as_array().is_some_and(Vec::is_empty)
}

fn loose(case: &Value) -> bool {
    empty(&case["requires_t3_tools"]) && empty(&case["t3_state_checks"])
}

fn runnable(case: &Value) -> bool {
    case["runnable_in_both_arms"] == true
}

fn asks(case: &Value) -> bool {
    loose(case) && !empty(&case["asks_without_t3_tools"])
}

fn replayed_control(case: &Value) -> bool {
    case["kind"] == "control" && asks(case) && runnable(case) && case["replays_prior_turns"] == true
}

fn ids(cases: &Map<String, Value>, keep: impl Fn(&Value) -> bool) -> Vec<&String> {
    cases.iter().filter(|(_, case)| keep(case)).map(|(id, _)| id).collect()
}

pub fn classify(checkout: &Path, pin_commit: &str) -> Result<Value, String> {
    pin::at(checkout, pin_commit)?;
    let files = pin::case_files(checkout)?;
    let mut cases = Map::new();
    let mut named = BTreeSet::new();
    for file in &files {
        let text = fs::read_to_string(checkout.join(file)).map_err(|e| format!("{file}: {e}"))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let case: Value = serde_json::from_str(line).map_err(|e| format!("{file}: {e}"))?;
            let id = case["id"].as_str().ok_or_else(|| format!("{file}: a case has no id"))?.to_owned();
            let row = entry(&case, file);
            for key in ["requires_t3_tools", "forbids_t3_tools"] {
                named.extend(row[key].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned));
            }
            if cases.insert(id.clone(), row).is_some() {
                return Err(format!("{file}: case id {id} appears twice"));
            }
        }
    }
    let unknown = undefined(&named, checkout)?;
    if !unknown.is_empty() {
        return Err(format!("these lower-case tool names are in checks but not defined under apps/server/src/mcp/toolkits, so the rule cannot call them T3 Code's: {}", unknown.join(", ")));
    }
    let counts = json!({
        "all": count(&cases, |_| true),
        "neutral": count(&cases, |case| case["neutral"] == true),
        "neutral_and_runnable_in_both_arms": count(&cases, |case| case["neutral"] == true && runnable(case)),
        "no_t3_tool_required_and_no_t3_state_check": count(&cases, loose),
        "of_those_with_a_check_that_names_no_t3_tool": count(&cases, |case| loose(case) && !(empty(&case["asks_without_t3_tools"]) && empty(&case["bars_without_t3_tools"]))),
        "of_those_asking_for_something_without_t3_tools": count(&cases, asks),
        "replayed_control": count(&cases, replayed_control),
        "of_those_runnable_in_both_arms": count(&cases, |case| asks(case) && runnable(case)),
        "of_those_whose_world_needs_no_t3_tool": count(&cases, |case| loose(case) && !(empty(&case["asks_without_t3_tools"]) && empty(&case["bars_without_t3_tools"])) && empty(&case["world_needs_t3"])),
    });
    let lists = json!({
        "neutral": ids(&cases, |case| case["neutral"] == true),
        "neutral_and_runnable_in_both_arms": ids(&cases, |case| case["neutral"] == true && runnable(case)),
        "replayed_control": ids(&cases, replayed_control),
        "asking_for_something_without_t3_tools": ids(&cases, asks),
        "asking_and_runnable_in_both_arms": ids(&cases, |case| asks(case) && runnable(case)),
        "asking_and_runnable_given_more_than_files": ids(&cases, |case| asks(case) && !case["runnable_given"].is_null()),
        "asking_and_not_run": ids(&cases, |case| asks(case) && !case["not_run"].is_null()),
    });
    let stray: Vec<&str> = GIVEN.iter().chain(&NOT_RUN).map(|(id, _)| *id).filter(|id| !cases.get(*id).is_some_and(|case| asks(case) && !runnable(case))).collect();
    if pin_commit == pin::PIN && !stray.is_empty() {
        return Err(format!("these judged cases are not asking cases a files-only world leaves out at the pin: {}", stray.join(", ")));
    }
    let graders: Vec<String> = pin::GRADERS.iter().map(|path| (*path).to_owned()).collect();
    Ok(json!({
        "source": pin::SOURCE, "release": pin::RELEASE, "pin": pin_commit, "rule": RULE, "judged": JUDGED,
        "case_files": pin::hashes(checkout, &files)?, "graders": pin::hashes(checkout, &graders)?,
        "counts": counts, "lists": lists, "cases": cases,
    }))
}
