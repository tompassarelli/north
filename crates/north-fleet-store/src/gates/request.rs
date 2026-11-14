use crate::ask_once;
use crate::error::{NorthError, NorthResult};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GateRequest {
    pub verification: bool,
    #[serde(default)]
    pub issue: Option<String>,
    /// The one box of `issue` this gate decides, matched against a launch's `Item:` and `Box:` lines; none holds the whole issue.
    #[serde(default)]
    pub box_text: Option<String>,
    pub session: String,
    pub question: String,
    pub options: Vec<String>,
    pub recommend: usize,
    pub default: usize,
    pub default_after_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask_once: Option<String>,
}

impl GateRequest {
    pub fn validate(&self) -> NorthResult<()> {
        let in_range = |choice: usize| (1..=self.options.len()).contains(&choice);
        if self.box_text.as_deref().is_some_and(|text| text.trim().is_empty() || self.issue.is_none()) {
            return Err(NorthError::Usage("--box needs non-empty text and an --issue to scope".into()));
        }
        if self.question.trim().is_empty() {
            return Err(NorthError::Usage("a gate needs a question".into()));
        }
        if self.options.len() < 2 || self.options.iter().any(|option| option.trim().is_empty()) {
            return Err(NorthError::Usage("a gate needs at least two non-empty options".into()));
        }
        if !in_range(self.recommend) {
            return Err(NorthError::Usage(format!("--recommend must name an option from 1 to {}", self.options.len())));
        }
        let joined = std::iter::once(self.question.as_str()).chain(self.options.iter().map(String::as_str)).collect::<Vec<_>>().join("\n");
        if let Some(phrase) = north_base::platform::signin::names(&joined) && !ask_once::names("accounts", &self.question, &self.options) {
            return Err(NorthError::Usage(format!("this gate asks Tom to sign in (\"{phrase}\"), and no agent work waits on a human sign-in: build the machine path (a GitHub App token, a sops-held key or a setup token) as a box instead; only a new account, which nobody can create for him, may ask him to sign in")));
        }
        self.decision_rule()
    }

    pub fn has_default(&self) -> bool {
        self.default != 0 || self.default_after_ms != 0
    }

    /// Whether only Tom answers this gate: an ask-once gate carries no default, so it waits for him while every other gate applies its default at its deadline.
    pub fn for_tom(&self) -> bool {
        !self.has_default()
    }
}

pub const ASKED_REPLY: &str = "North put this question to your parent session as a gate (to Tom when you have no parent). \
The answer arrives later as a message that starts with [gate N]. Keep working on everything the question does not block, \
and end your turn when nothing else is left.";
const ASK_DEFAULT_AFTER_MS: u64 = 30 * 60 * 1000;
const RECOMMENDED: &str = "(Recommended)";
const FILLERS: [&str; 2] = ["Use your best judgment", "Wait for a written answer"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ask {
    pub text: String,
    pub options: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Question {
    pub conversation: String,
    pub asks: Vec<Ask>,
    pub reply: String,
}

impl Ask {
    pub fn gate(&self, session: &str) -> GateRequest {
        let mut options: Vec<String> = self.options.iter().map(|option| option.trim()).filter(|option| !option.is_empty()).map(str::to_owned).collect();
        let recommend = options.iter().position(|option| option.contains(RECOMMENDED)).map_or(1, |index| index + 1);
        for filler in FILLERS {
            if options.len() < 2 {
                options.push(filler.to_owned());
            }
        }
        let category = ask_once::class(&self.text, &options).map(str::to_owned);
        let (default, default_after_ms) = if category.is_some() { (0, 0) } else { (recommend, ASK_DEFAULT_AFTER_MS) };
        GateRequest { verification: false, issue: None, box_text: None, session: session.to_owned(), question: self.text.clone(), options, recommend, default, default_after_ms, ask_once: category }
    }
}

pub(super) fn opened_notice(label: &str, request: &GateRequest) -> String {
    let default = if request.has_default() { format!("default: {}", request.options.get(request.default.saturating_sub(1)).map_or("", String::as_str)) } else { "for Tom: only the work this gate holds waits for his answer; keep working on everything else".to_owned() };
    let mark = if request.verification { "test data: " } else { "" };
    format!("{mark}[gate {label} from {}] {} ({default})", request.session, request.question.replace('\n', " "))
}
