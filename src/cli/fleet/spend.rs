//! `north fleet spend [NAME | --self]`: the API-equivalent spend a session has run up so far,
//! the ledger's finished turns plus the turn still open.
use crate::error::{NorthError, NorthResult};
use crate::fleet::books::SessionSpend;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::SESSION_ENV;
use crate::fleet::turns::Tokens;
use crate::ledger::pricing::{PriceTable, TokenUse};

const USAGE: &str = "Usage: north fleet spend [NAME | --self] (--self and no NAME read the session name from $NORTH_FLEET_SESSION)";

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let own = std::env::var(SESSION_ENV).ok().filter(|name| !name.is_empty());
    let name = match (words, own) {
        ([] | ["--self"], Some(own)) => own,
        ([] | ["--self"], None) => return Err(NorthError::Usage(format!("{USAGE}; ${SESSION_ENV} is not set"))),
        ([name], _) if !name.starts_with("--") => (*name).to_owned(),
        _ => return Err(NorthError::Usage(USAGE.into())),
    };
    let record = fleet.session(&name)?;
    let follower = fleet.follow(&name)?;
    let spend = so_far(&name, fleet.spend_of(&name)?, &follower.transcript().tracker.live_usage(), record.model.as_deref(), &PriceTable::load().map_err(|error| NorthError::State(format!("price table: {error}")))?);
    println!("{}", line(&spend));
    Ok(())
}

fn so_far(name: &str, settled: Option<SessionSpend>, live: &[(Option<&str>, Tokens)], session_model: Option<&str>, prices: &PriceTable) -> SessionSpend {
    let mut spend = settled.unwrap_or_else(|| SessionSpend { session: name.to_owned(), tokens: 0, api_usd: Some(0.0) });
    for (model, tokens) in live {
        spend.tokens += tokens.total();
        let cost = model.or(session_model).and_then(|model| prices.price(model)).map(|price| price.of(TokenUse::from(*tokens)));
        spend.api_usd = spend.api_usd.zip(cost).map(|(sum, cost)| sum + cost);
    }
    spend
}

fn line(spend: &SessionSpend) -> String {
    let dollars = spend.api_usd.map_or_else(|| "unmeasured (a model has no price)".to_owned(), |usd| format!("${usd:.2}"));
    format!("{}: API-equivalent spend so far {dollars} ({} tokens)", spend.session, spend.tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: u64, output: u64) -> Tokens {
        Tokens { input, output, cache_read: 0, cache_write: 0 }
    }

    #[test]
    fn a_session_with_one_long_first_turn_still_reports_its_open_turns_spend() {
        let prices = PriceTable::load().unwrap();
        let spend = so_far("worker", None, &[(Some("claude-opus-5-5"), tokens(1_000_000, 0))], None, &prices);
        assert_eq!(line(&spend), "worker: API-equivalent spend so far $4.00 (1000000 tokens)");
    }

    #[test]
    fn an_open_turn_whose_stream_names_no_model_is_priced_at_the_sessions_model() {
        let prices = PriceTable::load().unwrap();
        let spend = so_far("worker", None, &[(None, tokens(1_000_000, 0))], Some("claude-opus-5-5"), &prices);
        assert_eq!(line(&spend), "worker: API-equivalent spend so far $4.00 (1000000 tokens)");
    }

    #[test]
    fn finished_turns_and_the_open_turn_add_up_and_an_unpriced_model_reads_unmeasured() {
        let prices = PriceTable::load().unwrap();
        let settled = SessionSpend { session: "worker".into(), tokens: 2_000_000, api_usd: Some(0.6) };
        let spend = so_far("worker", Some(settled.clone()), &[(Some("claude-opus-5-5"), tokens(1_000_000, 0))], None, &prices);
        assert_eq!(line(&spend), "worker: API-equivalent spend so far $4.60 (3000000 tokens)");
        let spend = so_far("worker", Some(settled), &[(Some("gpt-6-luna-mini"), tokens(10, 0))], None, &prices);
        assert_eq!(line(&spend), "worker: API-equivalent spend so far unmeasured (a model has no price) (2000010 tokens)");
    }
}
