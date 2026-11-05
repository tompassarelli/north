use super::state::{self, tree_state, truthy, unlanded, with_state};
use super::{Ctx, pattern};
use crate::hook::Verdict;
use serde_json::Value;

pattern!(ENDED_HONESTLY, r"(?i)^\W*(not done|blocked):");
pattern!(STOP_ASKING, r"(?i)^(should i|shall i|may i|do you want me to|want me to|would you like me to|let me know if you(?:'d| would) like|if you(?:'d| would) like,? i can)\b");
pattern!(STOP_NARRATING, r"(?i)^(next,? i(?:'ll| will)|i(?:'ll| will) now|now i(?:'ll| will)|i(?:'m| am) (now )?going to|continuing with|i(?:'ll| will) (continue|proceed|start|keep)|i(?:'m| am) (doing|fixing|running|handling|on) (that|this|it) now|(assigning|redirecting|dispatching|handing)\b[^.]*\b(owner|worker|team|agent))\b");
pattern!(STOP_DISCLAIMERS, r"(?i)\b(does not|doesn't|do not|don't) (prove|establish|demonstrate)\b|\bremains? (unproven|unobserved|unestablished)\b|\bnot yet (established|proven)\b");
pattern!(STOP_NEARLY, r"(?i)\b(nearly|almost) (done|there|finished|complete)\b");
pattern!(COUNTED, r"\d+ (of|/) ?\d+|\d+ (left|remaining)");
pattern!(STOP_MISSING_TOOL, r"(?i)\b(tools?|send_message|followup_task|spawn_agent|wait_agent)\b[^.\n]{0,60}\b(missing|unavailable|not available|restored?|gone)\b|\b(lacks?|missing|restore)\b[^.\n]{0,40}\btools?\b");
pattern!(CALL_ERROR, r"(?i)\berror\b|\bfailed with\b");
pattern!(ESCALATION, r"(?i)(^|\n)\W*blocked\b");
pattern!(ATTEMPT_EVIDENCE, r"(?i)\b(error|errors|failed|failure|fails|exit code|exited|returned|denied|refused|not found|timed out|timeout|traceback|panic|panicked|http \d{3}|\d{3} (status|response))\b");
pattern!(REAL_ASK, r"(?i)\b(money|pay|paid|billing|purchase|price|subscription|account|sign[- ]?in|log[- ]?in|password|credential|2fa|mfa|email|send (it|this|that|an? \w+) to|publish|post publicly|delete|choose|pick (one|between)|which (option|direction|one)|product (choice|decision|direction))\b|\$\d");

fn last_sentence(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < chars.len() {
        let after_stop = i > 0 && matches!(chars[i - 1], '.' | '!' | '?') && chars[i].is_whitespace();
        let end = if after_stop {
            chars[i..].iter().take_while(|c| c.is_whitespace()).count()
        } else if chars[i] == '\n' {
            chars[i..].iter().take_while(|c| **c == '\n').count()
        } else {
            0
        };
        if end == 0 {
            i += 1;
            continue;
        }
        pieces.push(chars[start..i].iter().collect::<String>());
        i += end;
        start = i;
    }
    pieces.push(chars[start..].iter().collect());
    pieces.into_iter().map(|p| p.trim().to_owned()).rfind(|p| !p.is_empty()).unwrap_or_default()
}

fn tail_chars(text: &str, count: usize) -> &str {
    let total = text.chars().count();
    text.char_indices().nth(total.saturating_sub(count)).map_or(text, |(start, _)| &text[start..])
}

fn unlanded_pass(ctx: &Ctx, event: &Value) -> bool {
    let last = with_state(ctx, &state::agent_key(event), |state| state.get("last_pass").cloned());
    let Some(last) = last.filter(|l| truthy(Some(l))) else {
        return false;
    };
    let cwd = last["cwd"].as_str().unwrap_or("None");
    tree_state(cwd).map_or(Value::Null, Value::from) == last["tree"] && unlanded(cwd)
}

pub(super) fn stop(ctx: &Ctx, event: &Value) -> Verdict {
    if truthy(event.get("stop_hook_active")) {
        return Verdict::Allow;
    }
    let message = event["last_assistant_message"].as_str().unwrap_or("").trim();
    if message.is_empty() {
        return Verdict::Allow;
    }
    if !ENDED_HONESTLY.is_match(message) && unlanded_pass(ctx, event) {
        return Verdict::Deny("Your last check passed on code that isn't landed. Land it now with `safe-push --to main` and close the box, then send a 3-line report starting \"Done:\". If something blocks landing, start with \"Blocked:\" and the exact error. Don't add checks.".into());
    }
    if STOP_MISSING_TOOL.is_match(message) && !CALL_ERROR.is_match(message) {
        return Verdict::Deny("No. You say a tool is missing, but you never got an error from calling it. Call it directly first. spawn_agent, send_message, followup_task, list_agents and wait_agent are direct collaboration tools; exec's ALL_TOOLS lists only scripting tools. Report a missing tool only with the error its call returned.".into());
    }
    if ESCALATION.is_match(message) && !ATTEMPT_EVIDENCE.is_match(message) && !REAL_ASK.is_match(message) {
        return Verdict::Deny("No. You're escalating without having tried. Try it, and come back only with the error it returned. Tom decides only money, accounts, sending as him, deleting data you didn't create, and product choices.".into());
    }
    let last = if message.to_lowercase().contains("decision for tom:") { String::new() } else { last_sentence(tail_chars(message, 600)) };
    if STOP_ASKING.is_match(&last) {
        return Verdict::Deny("No. You ended by asking permission. Tom gave you authority over everything reversible: do it now and report the result. If it truly needs him (money, accounts, sending as Tom, deleting data you didn't create, a product fork), end with `Blocked: decision for Tom: <question>` and your recommendation; the proxy puts it to Tom through the AskUserQuestion picker. Never address Tom yourself.".into());
    }
    if STOP_NARRATING.is_match(&last) {
        return Verdict::Deny("No. You ended on a plan or a hand-off. That's not an ending. Do the next step yourself, now, and stop only when it's done or blocked.".into());
    }
    if STOP_DISCLAIMERS.is_match(message) {
        return Verdict::Deny("No. Delete the list of what the result doesn't prove. Give the result, the numbers and one line of remaining risk.".into());
    }
    if STOP_NEARLY.is_match(message) && !COUNTED.is_match(message) {
        return Verdict::Deny("No. 'Nearly done' means nothing. Give a count of what's left.".into());
    }
    Verdict::Allow
}

