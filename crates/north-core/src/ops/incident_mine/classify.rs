use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

const RELAY_PATTERN: &str = r"(?i)^\s*(?:Read /home/|Delegation:|Item:|<task-notification|<command-|<local-command|<bash-|<pasted_content|<system-reminder|Caveat:|This session is being continued|From \S|Re \S|PEER\b|Claude\b|Codex\b|Plato\b|\S+-lead\b|\S+#\d+|#\d+|[\w-]+ \d{1,2}:\d{2}Z|Answer on|Use the Agent tool|Two new|New #|Capacity:|QUIET|Wind-down|Needs you|Stop hook|\[Request interrupted|Supervisor|CASE=|Overnight|Research only|You own|Add (?:two|one|a) |Tom\b|❯|The \S+ (?:fix|work)\b)";
const PUSHBACK_PATTERN: &str = r"(?i)(?:^\W*(?:no+|nope|stop|wait)\b|\b(?:wrong|why (?:are|is|did|do|does|would|not|isn'?t|aren'?t)|again\b|should(?:n'?t)? have|i said|i told|told you|i asked|asked you|you (?:didn'?t|did not|missed|forgot|keep)|broken|still (?:not|broken|failing|seeing|hanging|happening)|doesn'?t work|not working|isn'?t working|hate|annoy\w*|frustrat\w*|confus\w*|fuck\w*|shit|wtf|ugh|supposed to|nonsense|waste\w*|(?:that|this) is not|not what i|instead of|too (?:slow|long)|stuck|why the hell|stop \w+ing))";
const TOM_IN_RELAY_PATTERN: &str = r"(?i)\bTom(?:'s)?\b[^.\n]{0,60}\b(?:complain\w*|frustrat\w*|pushback|pushed back|correct\w*|annoyed|unhappy|angry|wrong|broken|hates?|not happy|reject\w*|objects?|is upset|said no)\b";
const BRIEF_PATTERN: &str = r"(?i)^\s*(?:Item:|CASE=|Read /|Delegation:|Research only|You own|Add )";
const LEAD_CORRECTION_PATTERN: &str = r"(?i)\b(?:wrong|did not stick|you did not|you didn'?t|not what|violat\w*|stop it|that is not|undo|revert (?:it|this)|you (?:skipped|ignored|missed))\b";

static RELAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(RELAY_PATTERN).unwrap());
static PUSHBACK: LazyLock<Regex> = LazyLock::new(|| Regex::new(PUSHBACK_PATTERN).unwrap());
static TOM_IN_RELAY: LazyLock<Regex> = LazyLock::new(|| Regex::new(TOM_IN_RELAY_PATTERN).unwrap());
static BRIEF: LazyLock<Regex> = LazyLock::new(|| Regex::new(BRIEF_PATTERN).unwrap());
static LEAD_CORRECTION: LazyLock<Regex> = LazyLock::new(|| Regex::new(LEAD_CORRECTION_PATTERN).unwrap());
static ASSISTANT: LazyLock<[(&str, Regex); 5]> = LazyLock::new(|| {
    [
        ("not-done", Regex::new(r"(?m)^\W*Not done\b").unwrap()),
        ("blocked", Regex::new(r"(?m)^\W*Blocked:").unwrap()),
        ("fix-chain", Regex::new(r"(?i)\b(?:two|2) failed fix|second failed fix|Stop: reasoning").unwrap()),
        ("revert", Regex::new(r"(?i)\b(?:I |was |been |auto-)?revert(?:ed|ing)\b").unwrap()),
        ("admission", Regex::new(r"(?i)\b(?:you'?re right|my mistake|I was wrong|I misread|I missed|my fault|apologi[sz]e)\b").unwrap()),
    ]
});

#[derive(Deserialize)]
pub(super) struct Message {
    pub(super) id: serde_json::Value,
    pub(super) provider: String,
    pub(super) session: String,
    #[serde(default)]
    pub(super) project: Option<String>,
    #[serde(default)]
    pub(super) agent: Option<String>,
    pub(super) ts: String,
    pub(super) role: String,
    #[serde(default)]
    pub(super) text: Option<String>,
}

impl Message {
    pub(super) fn text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    fn relayed_by_agent(&self) -> bool {
        self.agent.as_deref().is_some_and(|agent| !agent.is_empty())
    }
}

pub(super) type Hit = (&'static str, &'static str);

pub(super) fn user_candidate(message: &Message) -> Option<Hit> {
    let text = message.text();
    let length = text.chars().count();
    let head: String = text.trim_start().chars().take(400).collect();
    let relayed = message.relayed_by_agent() || RELAY.is_match(&head) || (message.provider == "codex" && length > 600);
    if !relayed {
        return (length <= 4000 && PUSHBACK.is_match(text)).then_some(("tom-typed", "pushback"));
    }
    if BRIEF.is_match(&head) || length > 2500 {
        return None;
    }
    if TOM_IN_RELAY.is_match(text) {
        return Some(("relayed-tom", "pushback"));
    }
    if LEAD_CORRECTION.is_match(text) && !message.relayed_by_agent() {
        return Some(("relayed-lead", "correction"));
    }
    None
}

pub(super) fn assistant_candidate(message: &Message) -> Option<Hit> {
    let text = message.text();
    ASSISTANT.iter().find(|(_, pattern)| pattern.is_match(text)).map(|(flag, _)| ("assistant", *flag))
}

pub(super) fn excerpt(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(limit - 1).collect();
    cut.push('…');
    cut
}
