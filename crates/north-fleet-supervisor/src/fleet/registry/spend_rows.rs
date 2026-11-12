use crate::error::{NorthError, NorthResult};
use crate::fleet::books::SessionSpend;
use crate::fleet::host::Host;
use crate::fleet::model::{Delivery, Status};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind};
use crate::fleet::turns::Tokens;
use crate::credit::Open;
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

    /// The api turns running on this host now, each with the tokens it has used so far.
    pub fn open_api_turns(&self) -> NorthResult<Vec<Open>> {
        let mut open = Vec::new();
        for record in self.sessions()?.into_iter().filter(|record| record.account.starts_with("api:") && self.status(record) == Status::Working) {
            let Ok(follower) = self.follow(&record.name) else { continue };
            for (model, tokens) in follower.transcript().tracker.live_usage() {
                open.push(Open { session: record.name.clone(), account: Host::served_by(&record.host_dir).unwrap_or_else(|| record.account.clone()), model: model.or(record.model.as_deref()).map(str::to_owned), tokens: TokenUse::from(tokens) });
            }
        }
        Ok(open)
    }

    pub fn spend_or_ask(&self, name: &str, requester: Option<&str>) -> NorthResult<Spent> {
        if self.session_dir(name)?.exists() {
            return self.spend_line(name).map(Spent::Here);
        }
        let held = self.held_elsewhere(name)?.ok_or_else(|| NorthError::Usage(format!("no session named {name}")))?;
        let requester = requester.ok_or_else(|| NorthError::Usage(format!("{name} runs on {}, which answers a spend request with a message to the asking session; ask from a session, or run this on {}", held.host, held.host)))?;
        let row = self.new_row(RowKind::Spend, name, "", &Sender::Session(requester.to_owned()))?;
        self.spool().put(&row)?;
        Ok(Spent::Asked { id: row.id, host: held.host })
    }

    pub(super) fn answer_spend_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let requester = row.sender.session().ok_or_else(|| NorthError::Usage(format!("SPEND row {} names no session to answer", row.id)))?;
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

