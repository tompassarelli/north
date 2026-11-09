//! What the lander tells the fleet session that queued a lane, which dispatch reads back from that session's transcript:
//! `landed BRANCH as SHA`, or `north land: BRANCH (HEAD) did not land: VERDICT`, followed for a lane its own checks
//! refused by `errors: N`, the errors its full check log reports (docs/rationale/nearest-routing.md section 5).

/// The fleet sender the lander signs its notices with; a session on another host sees it as `SENDER@HOST`.
pub const SENDER: &str = "service:north-land";

/// The fleet session that owns the lander and hears its alarms and what a lane with no owner would; a send to it while it is not live reaches the proxy, prefixed with this name.
pub const OWNER: &str = "land-lead";

const REFUSED: &str = "north land: ";
const DID_NOT_LAND: &str = ") did not land: ";
const ERRORS: &str = "errors: ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Landed { branch: String, sha: String },
    Refused { branch: String, head: String, verdict: String, errors: Option<usize> },
}

impl Notice {
    /// The notice's opening lines; the lander follows a refusal with the log's error lines and its path.
    pub fn text(&self) -> String {
        match self {
            Self::Landed { branch, sha } => format!("landed {branch} as {sha}"),
            Self::Refused { branch, head, verdict, errors } => {
                let counted = errors.map(|errors| format!("\n{ERRORS}{errors}")).unwrap_or_default();
                format!("{REFUSED}{branch} ({head}{DID_NOT_LAND}{verdict}{counted}")
            }
        }
    }

    /// The notice a message delivered to a session carries, when the fleet's `[from NAME] ` frame names the lander.
    pub fn delivered(message: &str) -> Option<Self> {
        let (sender, text) = message.trim_start().strip_prefix("[from ")?.split_once("] ")?;
        let lander = sender.strip_prefix(SENDER).is_some_and(|host| host.is_empty() || host.starts_with('@'));
        lander.then(|| Self::read(text)).flatten()
    }

    fn read(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let first = lines.next()?.trim_end();
        if let Some((branch, sha)) = first.strip_prefix("landed ").and_then(|rest| rest.split_once(" as ")) {
            return Some(Self::Landed { branch: branch.to_owned(), sha: sha.to_owned() });
        }
        let (branch, rest) = first.strip_prefix(REFUSED)?.split_once(" (")?;
        let (head, verdict) = rest.split_once(DID_NOT_LAND)?;
        let errors = lines.next().and_then(|line| line.trim().strip_prefix(ERRORS)).and_then(|count| count.parse().ok());
        Some(Self::Refused { branch: branch.to_owned(), head: head.to_owned(), verdict: verdict.to_owned(), errors })
    }
}

/// The sessions told a lane's verdict, in order until one is reached: its owner, then for a conflict, which only a person resolves, the lander's owner.
pub fn told<'a>(owner: Option<&'a str>, verdict: &str) -> Vec<&'a str> {
    owner.filter(|session| !session.is_empty()).into_iter().chain(verdict.starts_with("conflict ").then_some(OWNER)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_reaches_its_owner_then_the_landers_owner_and_a_sessionless_landing_reaches_no_one() {
        assert_eq!(told(Some("n1-worker"), "conflict src/a.rs,src/b.rs"), ["n1-worker", OWNER]);
        assert_eq!(told(None, "conflict src/a.rs"), ["land-lead"]);
        assert_eq!(told(Some(""), "conflict src/a.rs"), [OWNER]);
        assert!(told(None, "landed abc").is_empty());
        assert_eq!(told(Some("n1-worker"), "failed clippy"), ["n1-worker"]);
    }
}
