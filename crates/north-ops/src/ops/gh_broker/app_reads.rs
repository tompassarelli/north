use std::time::{SystemTime, UNIX_EPOCH};

use super::broker::{Broker, Source};
use super::calls::token_kind;
use super::http::{Request, Response};
use crate::platform::github_app::{self, AGENT_APP, Host};

type Mint = Box<dyn Fn() -> Option<String> + Send + Sync>;

const APP_CANNOT_READ: [u16; 3] = [401, 403, 404];

pub struct AppReads {
    mint: Mint,
}

impl AppReads {
    pub fn from_host() -> Self {
        Self::with(Box::new(|| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX));
            let request = github_app::Request::installation(AGENT_APP)?;
            github_app::mint(&request, &Host::from_env(), now).ok()
        }))
    }

    pub fn with(mint: Mint) -> Self {
        Self { mint }
    }

    pub fn serve(&self, broker: &Broker, caller: &str, request: Request) -> (Request, Response, Source) {
        if let Some(borrowed) = self.borrow(caller, &request) {
            let (response, source) = broker.handle(&borrowed);
            if !APP_CANNOT_READ.contains(&response.status) {
                return (borrowed, response, source);
            }
        }
        let (response, source) = broker.handle(&request);
        (request, response, source)
    }

    fn borrow(&self, caller: &str, request: &Request) -> Option<Request> {
        let rest_read = matches!(request.method.as_str(), "GET" | "HEAD") && !request.target.starts_with("/graphql");
        if !caller.ends_with(".service") || !rest_read || token_kind(request) != "personal" {
            return None;
        }
        let token = (self.mint)()?;
        let mut borrowed = request.clone();
        borrowed.headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
        borrowed.headers.push(("Authorization".to_owned(), format!("token {token}")));
        Some(borrowed)
    }
}
