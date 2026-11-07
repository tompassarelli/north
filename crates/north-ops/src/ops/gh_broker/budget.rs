use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::quota;
use crate::platform::gh_broker_route::Place;
use crate::platform::gh_session;
use crate::platform::github_app::{self, AGENT_APP, Host, Request};

const USAGE: &str = "usage: north github budget

Print what is left of each GitHub token's hourly limit (the user's own login and the Socrates Agent
App installation fleet sessions use), every fleet session's spend against its hourly share
(NORTH_GH_HOURLY_BUDGET units, see north gh-session) with the five calls that cost it most by caller
(the north command that ran gh, such as hook-public-text or fleet-tick, else shell) and endpoint, and the callers that cost the most GitHub
calls in the last hour by token (personal, app, none), personal first, from the broker's calls.log (calls GitHub answered with anything but 304,
apart from /rate_limit). A session reaching 50% and 80% of its share tells the proxy once.";

const TOP_CALLERS: usize = 10;
const SESSION_ROWS: usize = 5;
const HOUR: u64 = 3600;

pub fn github(arguments: &[String]) -> u8 {
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["budget"] => budget(),
        ["-h" | "--help"] => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn budget() -> u8 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let installation = Request::installation(AGENT_APP).ok_or_else(|| format!("unknown app {AGENT_APP}")).and_then(|request| github_app::mint(&request, &Host::from_env(), i64::try_from(now).unwrap_or(i64::MAX)));
    for (token, limits) in [("personal", quota::read(None)), ("socrates-agent app", installation.and_then(|token| quota::read(Some(&token))))] {
        match limits {
            Ok(limits) => limits.iter().for_each(|limit| println!("{token}\t{}", quota::line(limit))),
            Err(message) => println!("{token}\tunavailable: {message}"),
        }
    }
    let share = gh_session::hourly_budget();
    for spend in gh_session::session_spends(i64::try_from(now).unwrap_or(i64::MAX)) {
        println!("session {}\t{} of {share} units ({}%)", spend.session, spend.used, u64::from(spend.used) * 100 / u64::from(share.max(1)));
        for (caller, endpoint, units) in spend.by_call.iter().take(SESSION_ROWS) {
            println!("  {caller}\t{endpoint}\t{units} units ({}%)", u64::from(*units) * 100 / u64::from(share.max(1)));
        }
    }
    let log = std::fs::read_to_string(Place::from_env().calls()).unwrap_or_default();
    for (caller, calls) in top_callers(&log, now).into_iter().take(TOP_CALLERS) {
        println!("caller {caller}\t{} calls in the last hour ({} personal, {} app, {} none)", calls.total(), calls.personal, calls.app, calls.none);
    }
    0
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Calls {
    personal: u64,
    app: u64,
    none: u64,
}

impl Calls {
    fn total(&self) -> u64 {
        self.personal + self.app + self.none
    }
}

fn top_callers(log: &str, now: u64) -> Vec<(String, Calls)> {
    let mut spent: HashMap<&str, Calls> = HashMap::new();
    for line in log.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (at, caller, source, status, target, token) = match fields.as_slice() {
            [at, caller, source, status, _method, target, token] => (at, caller, source, status, target, *token),
            [at, caller, source, status, _method, target] => (at, caller, source, status, target, "personal"),
            _ => continue,
        };
        let recent = at.parse::<u64>().is_ok_and(|at| now.saturating_sub(at) < HOUR);
        if recent && *source == "upstream" && *status != "304" && !target.starts_with("/rate_limit") {
            let calls = spent.entry(caller).or_default();
            match token {
                "app" => calls.app += 1,
                "none" => calls.none += 1,
                _ => calls.personal += 1,
            }
        }
    }
    let mut callers: Vec<(String, Calls)> = spent.into_iter().map(|(caller, calls)| (caller.to_owned(), calls)).collect();
    callers.sort_by(|left, right| right.1.personal.cmp(&left.1.personal).then_with(|| right.1.total().cmp(&left.1.total())).then_with(|| left.0.cmp(&right.0)));
    callers
}

#[cfg(test)]
mod tests {
    use super::{Calls, top_callers};

    #[test]
    fn top_callers_counts_only_the_last_hours_upstream_calls_that_cost_quota_by_token_and_ranks_personal_first() {
        let log = "9000\ta.service\tupstream\t200\tGET\t/repos/o/r/issues\tapp\n\
                   9100\ta.service\tupstream\t200\tGET\t/repos/o/r/pulls\tapp\n\
                   9100\tb.service\tupstream\t200\tGET\t/repos/o/r/pulls\tpersonal\n\
                   9100\tb.service\tupstream\t304\tGET\t/repos/o/r/pulls\tpersonal\n\
                   9100\tb.service\tfresh\t200\tGET\t/repos/o/r/pulls\tpersonal\n\
                   9100\tb.service\tupstream\t200\tGET\t/rate_limit\tpersonal\n\
                   100\tb.service\tupstream\t200\tGET\t/repos/o/r/issues\tpersonal\n";
        assert_eq!(top_callers(log, 9200), [("b.service".to_owned(), Calls { personal: 1, app: 0, none: 0 }), ("a.service".to_owned(), Calls { personal: 0, app: 2, none: 0 })]);
    }
}
