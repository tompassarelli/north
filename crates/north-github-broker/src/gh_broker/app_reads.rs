use std::time::{SystemTime, UNIX_EPOCH};

use super::broker::{Broker, Source};
use super::calls::{Caller, token_kind};
use super::http::{Request, Response};
use north_base::platform::gh_broker_route::CREDENTIAL_ADDED_BY_BROKER;
use north_base::platform::github_app;

type Mint = Box<dyn Fn() -> Option<String> + Send + Sync>;

const APP_CANNOT_READ: [u16; 3] = [401, 403, 404];
/// The units whose REST reads go out on the agent installation token: north's own long-lived services that read with the user's login, as the call logs of gex45, whiterabbit, cpx62b, cpx62 and nexus showed them on 2026-11-15, and each repository's lander, `north-land@REPOSITORY.service`.
const BORROWERS: [&str; 13] = [
    "agent-runtime-update.service",
    "agents-flow-stall.service",
    "autoland-sweep.service",
    "denylist-attest.service",
    "fleet-tick.service",
    "harness-health.service",
    "lane-gc.service",
    "north-land-shadow.service",
    "north-lander-replay.service",
    "north-runtime-follow.service",
    "north-tune.service",
    "north-watch.service",
    "outside-watch.service",
];
const LANDER_UNIT: (&str, &str) = ("north-land@", ".service");

fn borrows(unit: &str) -> bool {
    BORROWERS.contains(&unit) || unit.strip_prefix(LANDER_UNIT.0).and_then(|instance| instance.strip_suffix(LANDER_UNIT.1)).is_some_and(|instance| !instance.is_empty())
}

pub struct AppReads {
    mint: Mint,
}

impl AppReads {
    pub fn from_host() -> Self {
        Self::with(Box::new(|| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX));
            github_app::agent_installation(now).ok()
        }))
    }

    pub fn with(mint: Mint) -> Self {
        Self { mint }
    }

    pub fn serve(&self, broker: &Broker, caller: &Caller, request: Request) -> (Request, Response, Source) {
        if let Some(added) = self.added(caller, &request) {
            let (response, source) = broker.handle(&added);
            return (added, response, source);
        }
        if let Some(borrowed) = self.borrow(caller, &request) {
            let (response, source) = broker.handle(&borrowed);
            if !APP_CANNOT_READ.contains(&response.status) {
                return (borrowed, response, source);
            }
        }
        let (response, source) = broker.handle(&request);
        (request, response, source)
    }

    fn added(&self, caller: &Caller, request: &Request) -> Option<Request> {
        let credential = request.header("authorization").and_then(|value| value.split_whitespace().nth(1));
        if !caller.in_worker_scope() || credential.is_some_and(|credential| credential != CREDENTIAL_ADDED_BY_BROKER) {
            return None;
        }
        self.with_token(request)
    }

    fn borrow(&self, caller: &Caller, request: &Request) -> Option<Request> {
        let rest_read = matches!(request.method.as_str(), "GET" | "HEAD") && !request.target.starts_with("/graphql");
        if !caller.unit().is_some_and(borrows) || !rest_read || token_kind(request) != "personal" {
            return None;
        }
        self.with_token(request)
    }

    fn with_token(&self, request: &Request) -> Option<Request> {
        let token = (self.mint)()?;
        let mut borrowed = request.clone();
        borrowed.headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
        borrowed.headers.push(("Authorization".to_owned(), format!("token {token}")));
        Some(borrowed)
    }
}
