use super::case::Case;
use super::state::{self, sha256_hex, with_shared, with_state};
use super::{Ctx, PEER_LINE, asked_keyword, log_override, pattern};
use crate::hook::{HookInput, Verdict};
use crate::platform::pyjson::{self, Value as Py};
use serde_json::{Value, json};
use std::path::Path;

const ONE_RUN: &str = "Tom's rule: every check gets one run. It passes, ship; a failure gets fixed and run once more. The sim is deterministic, so a second run on the same code shows nothing new.";
const RERUN_ASKED: [&str; 5] = ["rerun", "re run", "again", "retry", "run it once more"];
const SCAFFOLD_WORDS: &str = "manifest|provenance|attest|sbom|checksum|schema_version|migration|compat|changelog|release|inventory|sha256";

pattern!(FACT_BANNED, r"(?i)confiden|\bsure\b|\bagain\b|double[- ]?check|sanity|verif|flaky|just in case|safety");
pattern!(JUSTIFY_NOOP, r#"^\s*:\s+justify\s+([\w-]+)\s+([A-Za-z])\s+("([^"]*)"|'([^']*)')\s*$"#);
pattern!(TEE, r"\|\s*tee\b");
pattern!(TIMING_CHECK, r"(?i)\b(perf|bench\w*|frame[- ]?cost|latency|timing)\b");
pattern!(MEASURE_ONLY, r"(?i)\b(measure|measurement|profil\w*|diagnos\w*|investigat\w*|research)[- ]only\b|\bno (fix|optimi[sz]ation|code change)s? (in|for) (this|that) task\b|\b(do not|don'?t) (fix|change|optimi[sz]e)\b|\bwithout (landing |making )?(a |any )?fix\b");
pattern!(MEASURE_OUTCOME, r"(?i)\bPEER\b|\bland(s|ing)? (a |the |its |one )?fix\b|\bfix it lands\b");
pattern!(HANDOFF, r"agents/handoffs\b");
pattern!(SHELL_TARGET, r#"(?:>>?|\btee(?:\s+-a)?|\btouch)\s+['"]?([^\s'";|&<>]+)"#);
pattern!(PATCH_ADD, r"(?m)^\*\*\* Add File:\s*(\S+)");
static JOINED: std::sync::LazyLock<fancy_regex::Regex> =
    std::sync::LazyLock::new(|| fancy_regex::Regex::new(r"(?<![\w/.\-])[a-z]{25,}(?![\w/.\-])").expect("joined pattern compiles"));
static SCAFFOLD_FILE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    super::re(&format!(r"(?i)(^|[/_.-])({SCAFFOLD_WORDS})\w*([._-][^/]*)?$|\.(sha256|sha256sum|sha512)$"))
});

const RERUN_CASES: &str = "Which case is it?\nA. It reads something outside the worktree that changed since {at} (a rebuilt map, a new client build): proceed, and name it in FACT.\nB. Tom asked for a rerun: proceed; the hook checks his last 3 prompts.\nC. A rerun for confidence before landing: stop and land with safe-push. On 8 Oct #244's worker spent 95 min and 31.9M tokens on five more CI runs after its boxes passed, and four tuning workers ran the same balance baseline on febfa549 within six minutes.\nD. The last pass looked flaky: stop and report it in one line. Never rerun until it passes.\nE. You lost the output: rerun once, with `| tee FILE`.\nF. A frame-cost or latency check confirming once on a quiet machine, under an exclusive capacity lease: proceed, and name the load difference in FACT.\nTo proceed, prefix the command with CASE=<letter> FACT=\"<the one new fact this run shows>\". One justification per check and code state. ";

fn fact_problem(fact: &str) -> Option<String> {
    if fact.chars().count() < 15 {
        return Some("FACT must name the new fact in 15 or more characters.".into());
    }
    FACT_BANNED.find(fact).map(|banned| {
        format!("FACT can't rest on \"{}\": name what this run shows that the last one didn't.", banned.as_str())
    })
}

fn rerun_problem(case: &str, case_input: &Case, name: &str, command: &str, recent: &[String]) -> Option<String> {
    let problem = match case {
        "C" => Some("Case C stops here: land it with `safe-push --to main` and tick the box.".to_owned()),
        "D" => Some("Case D stops here: report the flaky result in one line; don't rerun it until it passes.".to_owned()),
        "B" if !asked_keyword(&RERUN_ASKED, recent) => Some("Case B requires Tom's ask: none of his last 3 prompts asks for a rerun.".to_owned()),
        "B" => None,
        "E" if !TEE.is_match(command) => Some("Case E reruns only with `| tee FILE`, so the output survives this time.".to_owned()),
        "F" if !(TIMING_CHECK.is_match(name) && command.contains("exclusive")) => {
            Some("Case F is only for a frame-cost or latency check run under an exclusive capacity lease.".to_owned())
        }
        "A" | "E" | "F" => None,
        other => Some(format!("There is no case {other}.")),
    };
    if problem.is_none() && case != "B" { fact_problem(&case_input.fact) } else { problem }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn rerun_gate(ctx: &Ctx, input: &HookInput, event: &Value, name: &str, at: &str, fingerprint: &str, case_input: &Case, command: &str) -> Verdict {
    let lead = format!("`{}` already passed on this code at {at}. {ONE_RUN}\n", super::chars(name, 80));
    let Some(case) = case_input.letter.as_deref() else {
        return Verdict::Deny(format!("{lead}{}{PEER_LINE}", RERUN_CASES.replace("{at}", at)));
    };
    let recent = with_state(ctx, &state::agent_key(event), |state| state::prompts(state, 3));
    let mut problem = rerun_problem(case, case_input, name, command, &recent);
    if problem.is_none() {
        let key = sha256_hex(fingerprint.as_bytes())[..24].to_owned();
        problem = with_shared(ctx, |shared| {
            let used = shared.entry("justified").or_insert_with(|| json!({}));
            if !used.is_object() {
                return None;
            }
            if state::truthy(used.get(&key)) {
                return Some("This check already used its one justification on this code.".to_owned());
            }
            used[&key] = json!(case);
            None
        });
    }
    if let Some(problem) = problem {
        return Verdict::Deny(format!("{lead}{problem} {PEER_LINE}"));
    }
    log_override(ctx, input, "rerun", case, &case_input.fact);
    Verdict::Allow
}

const MEASURE_CASES: &str = "Which case is it?\nA. A box asks for this number: proceed; FACT starts `box:` and quotes the box.\nB. A baseline for a fix you're about to make: proceed; FACT starts `fix:` and names the fix. The next measurement needs a patch first.\nC. To understand the problem better: stop and change the likeliest cause. On 8 Oct perf_phases_168 spent 6.0M tokens on a phase table, wrote \"No optimization was made\", and #168 stayed at 18.26 ms against 10 ms.\nD. To confirm the earlier number: stop and report it.\nTo proceed, prefix the command with CASE=<letter> FACT=\"box: ...\" or FACT=\"fix: ...\". ";

fn measure_problem(case: &str, fact: &str) -> Option<String> {
    let lower = fact.to_lowercase();
    let problem = match case {
        "C" => Some("Case C stops here: change the likeliest cause now.".to_owned()),
        "D" => Some("Case D stops here: report the number you have.".to_owned()),
        "A" if !lower.starts_with("box:") => Some("Case A's FACT starts `box:` and quotes the box that asks for this number.".to_owned()),
        "B" if !lower.starts_with("fix:") => Some("Case B's FACT starts `fix:` and names the fix you're about to make.".to_owned()),
        "A" | "B" => None,
        other => Some(format!("There is no case {other}.")),
    };
    problem.or_else(|| fact_problem(fact))
}

pub(super) fn measure_gate(ctx: &Ctx, input: &HookInput, event: &Value, case_input: &Case) -> Option<Verdict> {
    let allowed = with_state(ctx, &state::agent_key(event), |state| {
        let count = state::count(state.get("measures"));
        let baseline = state::truthy(state.get("fix_baseline"));
        if count < 2 && !baseline {
            return None;
        }
        let lead = format!("You've run {count} measurements since your last change. Tom's rule: a measurement ends in a fix you land, or a PEER line.\n");
        if baseline {
            return Some(Verdict::Deny(format!(
                "{lead}You took a fix: baseline already. Make the fix first; the next measurement needs a patch in between. {PEER_LINE}"
            )));
        }
        let Some(case) = case_input.letter.as_deref() else {
            return Some(Verdict::Deny(format!("{lead}{MEASURE_CASES}{PEER_LINE}")));
        };
        if let Some(problem) = measure_problem(case, &case_input.fact) {
            return Some(Verdict::Deny(format!("{lead}{problem} {PEER_LINE}")));
        }
        state.insert("measures".into(), json!(0));
        if case == "B" {
            state.insert("fix_baseline".into(), json!(true));
        }
        Some(Verdict::Allow)
    })?;
    if allowed == Verdict::Allow {
        log_override(ctx, input, "measure", case_input.letter.as_deref().unwrap_or(""), &case_input.fact);
        return None;
    }
    Some(allowed)
}

pub(super) fn pre_justify(ctx: &Ctx, input: &HookInput, event: &Value, command: &str) -> Option<Verdict> {
    let m = JUSTIFY_NOOP.captures(command)?;
    let (gate, case) = (&m[1], m[2].to_uppercase());
    let fact = m.get(4).or(m.get(5)).map_or("", |f| f.as_str()).to_owned();
    let problem = match case.as_str() {
        _ if gate != "scaffold" => Some("Only the scaffold gate takes `: justify`; other gates take a CASE= FACT= prefix.".to_owned()),
        "C" => Some("Case C stops here: drop the traceability or later-readiness part.".to_owned()),
        "A" => Some("Case A is checked by the hook from Tom's prompts; just retry the patch if he asked.".to_owned()),
        "B" | "D" => None,
        other => Some(format!("There is no case {other}.")),
    }
    .or_else(|| fact_problem(&fact));
    if let Some(problem) = problem {
        return Some(Verdict::Deny(format!("{problem} {PEER_LINE}")));
    }
    with_state(ctx, &state::agent_key(event), |state| {
        state.insert("scaffold_ok".into(), json!({"case": case, "fact": fact, "at": ctx.now}));
    });
    log_override(ctx, input, "scaffold", &case, &fact);
    Some(Verdict::Allow)
}

fn tool_input_text(input: &HookInput) -> String {
    match input.data.get("tool_input") {
        Some(Py::String(text)) => text.lossy().into_owned(),
        Some(value) if py_truthy(value) => pyjson::dumps(value),
        _ => "{}".into(),
    }
}

fn py_truthy(value: &Py) -> bool {
    match value {
        Py::Null => false,
        Py::Bool(flag) => *flag,
        Py::String(text) => !text.lossy().is_empty(),
        Py::Array(items) => !items.is_empty(),
        Py::Object(entries) => !entries.is_empty(),
        other => other.as_f64().is_none_or(|n| n != 0.0),
    }
}

pub(super) fn joined_words(input: &HookInput, brief: bool) -> Option<String> {
    let text = tool_input_text(input);
    if !(brief || HANDOFF.is_match(&text)) {
        return None;
    }
    let text = text.replace("\\n", "\n");
    let runs: Vec<&str> = JOINED.find_iter(&text).filter_map(Result::ok).map(|m| m.as_str()).collect();
    let first = runs.first()?;
    Some(format!(
        "No. This text runs words together (\"{}\"): {} run(s) of 25+ letters with no spaces. Another agent acts on it and misreads it. Rewrite each in full sentences with normal spaces between words and numbers, then retry.",
        super::chars(first, 40),
        runs.len()
    ))
}

fn codex_box_denial(input: &HookInput) -> Option<String> {
    let message = match input.data.get("tool_input") {
        Some(Py::Object(_)) => match input.data["tool_input"].get("message") {
            None => "",
            Some(value) => value.as_str()?,
        },
        _ => "",
    };
    let answer = crate::router::ops::answer(&json!({"op": "codex_box", "brief": message}))?.ok()?;
    let failed = answer.as_str().filter(|text| !text.is_empty())?;
    Some(format!("route to Claude: {failed}"))
}

pub(super) fn pre_spawn(input: &HookInput) -> Verdict {
    if let Some(reason) = joined_words(input, true) {
        return Verdict::Deny(reason);
    }
    let brief = tool_input_text(input);
    if MEASURE_ONLY.is_match(&brief) && !MEASURE_OUTCOME.is_match(&brief) {
        return Verdict::Deny("This brief only measures. Tom's rule: a measurement worker ends with a fix it lands, or a PEER line naming what the numbers point to. On 8 Oct perf_phases_168 spent 6.0M tokens on a measure-only brief and #168 moved no number. To retry, add one line to the brief: \"End with a fix you land, or a PEER line.\"".into());
    }
    codex_box_denial(input).map_or(Verdict::Allow, Verdict::Deny)
}

fn new_files(event: &Value) -> Vec<String> {
    let tool_input = &event["tool_input"];
    let text = match tool_input {
        Value::Object(_) => super::command(event),
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    };
    let pattern = if event["tool_name"] == "Bash" { &*SHELL_TARGET } else { &*PATCH_ADD };
    pattern.captures_iter(&text).map(|m| m[1].to_owned()).collect()
}

pub(super) fn pre_scaffold(ctx: &Ctx, event: &Value) -> Option<String> {
    let cwd = event["cwd"].as_str().filter(|c| !c.is_empty());
    let root = state::resolve(&ctx.code_root);
    let inside: Vec<String> = new_files(event)
        .into_iter()
        .filter(|name| SCAFFOLD_FILE.is_match(name))
        .filter(|name| {
            let path = Path::new(name);
            let path = if path.is_absolute() { path.to_path_buf() } else { Path::new(cwd.unwrap_or(".")).join(path) };
            let resolved = state::resolve(&path);
            let parent = resolved.parent().filter(|p| p.exists()).map(|p| p.to_string_lossy().into_owned());
            resolved.starts_with(&root) && !resolved.exists() && super::expected_profile(ctx, parent.as_deref().or(cwd)) == "prototype"
        })
        .collect();
    let first = inside.first()?;
    let word = SCAFFOLD_FILE.captures(first).and_then(|m| m.get(2)).map_or("sha256", |m| m.as_str()).to_owned();
    let (recent, armed) = with_state(ctx, &state::agent_key(event), |state| (state::prompts(state, 3), state.remove("scaffold_ok")));
    if asked_keyword(&[&word.to_lowercase()], &recent) || state::truthy(armed.as_ref()) {
        return None;
    }
    Some(format!(
        "`{first}` adds a {word} file to a prototype repo, and none of Tom's last 3 prompts asks for one. Tom is the only user. Which case is it?\nA. Tom asked for it: the hook checks his prompts, so this isn't A.\nB. The build or run fails without it: name the command and its error.\nC. Traceability, safety or readiness for later: stop and drop that part. On 8 Oct the #168 worker committed tapes.sha256, a generator SHA256 and 384 KB of samples, and 20 of 180 Mak'gora commits only added such records (evidence/ is 391 MB).\nD. Another tool requires this format: treat it as B.\nTo proceed with B or D, run `: justify scaffold B \"<command and its error>\"`, then retry the patch. Otherwise put the result in the issue comment. {PEER_LINE}"
    ))
}
