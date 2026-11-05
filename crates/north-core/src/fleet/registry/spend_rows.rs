use crate::error::{NorthError, NorthResult};
use crate::fleet::books::SessionSpend;
use crate::fleet::model::Delivery;
use crate::fleet::store::{FleetRow, RowKind, Spool};
use crate::fleet::turns::Tokens;
use crate::router::pricing::{PriceTable, TokenUse};

use super::{Fleet, HandedIn};

#[derive(Debug, Eq, PartialEq)]
pub enum Spent {
    Here(String),
    Asked { id: String, host: String },
}

impl Fleet {
    pub fn spend_line(&self, name: &str) -> NorthResult<String> {
        let record = self.record(name)?;
        let follower = self.follow(name)?;
        let prices = PriceTable::load().map_err(|error| NorthError::State(format!("price table: {error}")))?;
        Ok(line(&so_far(name, self.spend_of(name)?, &follower.transcript().tracker.live_usage(), record.model.as_deref(), &prices)))
    }

    pub fn spend_or_ask(&self, name: &str, requester: Option<&str>) -> NorthResult<Spent> {
        if self.session_dir(name)?.exists() {
            return self.spend_line(name).map(Spent::Here);
        }
        let held = self.held_elsewhere(name)?.ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?;
        let requester = requester.ok_or_else(|| NorthError::Usage(format!("{name} runs on {}, which answers a spend request with a message to the asking session; ask from a session, or run this on {}", held.host, held.host)))?;
        let row = self.new_row(RowKind::Spend, name, "", Some(requester))?;
        Spool::at(&self.root).put(&row)?;
        Ok(Spent::Asked { id: row.id, host: held.host })
    }

    pub(super) fn answer_spend_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let requester = row.sender.as_deref().ok_or_else(|| NorthError::Usage(format!("SPEND row {} names no session to answer", row.id)))?;
        let mut answer = None;
        let handed = self.journaled(row, || {
            answer = Some(self.spend_line(&row.to)?);
            Ok(())
        })?;
        if let Some(line) = answer {
            self.tell(requester, &format!("[north: spend you asked for] {line}"), Delivery::Queued);
        }
        Ok(handed)
    }
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
    use super::super::fixtures::{fleet, live};
    use super::*;
    use crate::fleet::store::{SessionRow, cache_sessions};

    fn tokens(input: u64, output: u64) -> Tokens {
        Tokens { input, output, cache_read: 0, cache_write: 0 }
    }

    fn cached(fleet: &Fleet, names: &[&str]) {
        let rows: Vec<SessionRow> = names.iter().map(|name| SessionRow { name: (*name).into(), host: "laptop".into(), status: "idle".into(), created: "t".into(), updated: "t".into(), ..SessionRow::default() }).collect();
        cache_sessions(&fleet.root, &rows).unwrap();
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

    #[test]
    fn a_spend_request_for_a_session_another_host_holds_is_spooled_and_that_host_answers_the_asker_once() {
        let (far_home, home) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (far, laptop) = (fleet(far_home.path()), fleet(home.path()));
        let _worker = live(&laptop, "worker");
        let asker = live(&laptop, "asker");
        cached(&far, &["worker"]);

        assert!(matches!(far.spend_or_ask("worker", None), Err(NorthError::Usage(why)) if why.contains("laptop")));
        assert!(matches!(far.spend_or_ask("nobody", Some("asker")), Err(NorthError::Usage(why)) if why == "no session named nobody"));
        let Spent::Asked { host, .. } = far.spend_or_ask("worker", Some("asker")).unwrap() else { panic!("a session of another host is asked") };
        let mut pending = Spool::at(&far.root).pending().unwrap();
        assert_eq!((host.as_str(), pending.len()), ("laptop", 1));
        let row = pending.remove(0);
        assert_eq!((row.kind, row.to.as_str(), row.sender.as_deref()), (RowKind::Spend, "worker", Some("asker")));

        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Delivered);
        assert_eq!(laptop.deliver_row(&row).unwrap(), HandedIn::Already);

        let told = asker.frames.lock().unwrap().join("\n");
        assert_eq!(told.matches("worker: API-equivalent spend so far").count(), 1, "{told}");
    }
}
