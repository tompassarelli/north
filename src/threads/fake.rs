use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use super::claims::Issue;
use super::claims::issues::Issues;
use super::verb::Outside;

/// A gh reply from the joined arguments, or None to fall through to `answers` and `reply`.
pub(crate) type Respond = fn(&str) -> Option<Result<String, String>>;

/// Records each gh call and answers it with the reply of the first `answers` needle its joined
/// arguments contain, else `reply`, refusing a call that contains `refuse`; refuses text holding
/// `secret`.
#[derive(Default)]
pub(crate) struct Recorded {
    pub calls: RefCell<Vec<Vec<String>>>,
    pub secret: Option<&'static str>,
    pub reply: &'static str,
    pub answers: Vec<(&'static str, String)>,
    pub alive: Vec<&'static str>,
    pub archived: &'static str,
    pub refuse: Option<&'static str>,
    /// Answers a call from its joined arguments before `answers` and `reply` are tried.
    pub respond: Option<Respond>,
    pub prices: Vec<(&'static str, f64)>,
}

impl Issues for Recorded {
    fn read(&self, items: &BTreeSet<(String, u64)>) -> BTreeMap<(String, u64), Issue> {
        items.iter().map(|item| (item.clone(), Issue::Unread)).collect()
    }
}

impl Outside for Recorded {
    fn scan(&self, text: &str) -> Result<(), String> {
        match self.secret {
            Some(secret) if text.contains(secret) => Err("gitleaks found a secret in the item text; credentials stay in Bitwarden, so nothing was stored".into()),
            _ => Ok(()),
        }
    }

    fn gh(&self, args: &[String]) -> Result<String, String> {
        self.calls.borrow_mut().push(args.to_vec());
        let joined = args.join(" ");
        if self.refuse.is_some_and(|needle| joined.contains(needle)) {
            return Err("gh api graphql: refused".into());
        }
        if let Some(answer) = self.respond.and_then(|respond| respond(&joined)) {
            return answer;
        }
        Ok(self.answers.iter().find(|(needle, _)| joined.contains(needle)).map_or(self.reply, |(_, reply)| reply.as_str()).to_owned())
    }

    fn alive_owners(&self) -> BTreeSet<String> {
        self.alive.iter().map(|owner| (*owner).to_owned()).collect()
    }

    fn archives(&self, _repo: &str, _number: u64) -> String {
        self.archived.to_owned()
    }

    fn prices(&self) -> Result<BTreeMap<String, f64>, String> {
        Ok(self.prices.iter().map(|(service, usd)| ((*service).to_owned(), *usd)).collect())
    }
}
