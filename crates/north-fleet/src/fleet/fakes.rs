#![cfg(any(test, feature = "test-support"))]

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

use crate::claude_stream::{encode_interrupt, encode_user_message};
use crate::error::NorthResult;
use crate::router::pricing::{PriceTable, TokenUse};

use super::books::{Books, Landing, SessionSpend};
use super::gates::{Closure, Gate, GateBook, GateRequest};
use super::model::{Entry, EntryKind, LaunchRequest, ProviderName, Quota, Status, Transcript};
use super::receipts::ReceiptBook;
use super::roster::Roster;
use super::runtime::{HostSpec, Instructions, Opening, Runtime, new_uuid};
use super::turns::{RunRecord, RunSink, Tokens, TurnExit, TurnSignal, TurnTotals};

#[derive(Default)]
pub struct Paper {
    runs: Mutex<Vec<RunRecord>>,
    receipts: Mutex<BTreeSet<(String, String)>>,
    gates: Mutex<Vec<Gate>>,
    landings: Mutex<BTreeMap<String, Landing>>,
}

impl RunSink for Paper {
    fn record_run(&self, run: &RunRecord) -> NorthResult<u64> {
        let mut runs = self.runs.lock().unwrap();
        runs.push(run.clone());
        Ok(runs.len() as u64)
    }

    fn join_task(&self, task_ref: &str, session: &str) -> NorthResult<()> {
        for run in self.runs.lock().unwrap().iter_mut().filter(|run| run.session == session && run.task_ref.is_none()) {
            run.task_ref = Some(task_ref.to_owned());
        }
        Ok(())
    }
}

impl ReceiptBook for Paper {
    fn claim_receipt(&self, session: &str, key: &str, _claimed_ms: u64) -> NorthResult<bool> {
        Ok(self.receipts.lock().unwrap().insert((session.to_owned(), key.to_owned())))
    }
}

impl GateBook for Paper {
    fn open_gate(&self, request: &GateRequest, opened_ms: u64) -> NorthResult<i64> {
        let mut gates = self.gates.lock().unwrap();
        let id = i64::try_from(gates.len()).unwrap() + 1;
        gates.push(Gate { id, request: request.clone(), opened_ms, closure: None });
        Ok(id)
    }

    fn gate(&self, id: i64) -> NorthResult<Option<Gate>> {
        Ok(self.gates.lock().unwrap().iter().find(|gate| gate.id == id).cloned())
    }

    fn gates(&self, closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        let mut gates: Vec<Gate> = self.gates.lock().unwrap().iter().filter(|gate| gate.closure.as_ref().is_none_or(|closure| closure.closed_ms >= closed_since_ms)).cloned().collect();
        gates.sort_by_key(|gate| (gate.closure.is_some(), gate.opened_ms + gate.request.default_after_ms, gate.id));
        Ok(gates)
    }

    fn close_gate(&self, id: i64, closure: &Closure) -> NorthResult<bool> {
        let mut gates = self.gates.lock().unwrap();
        let gate = gates.iter_mut().find(|gate| gate.id == id && gate.closure.is_none());
        Ok(gate.map(|gate| gate.closure = Some(closure.clone())).is_some())
    }
}

impl Books for Paper {
    fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
        let prices = PriceTable::load()?;
        let mut sessions: BTreeMap<String, SessionSpend> = BTreeMap::new();
        for run in self.runs.lock().unwrap().iter() {
            let tokens = run.turn.tokens;
            let price = run.turn.model.as_deref().and_then(|model| prices.price(model));
            let entry = sessions.entry(run.session.clone()).or_insert_with(|| SessionSpend { session: run.session.clone(), tokens: 0, api_usd: Some(0.0) });
            entry.tokens += tokens.input + tokens.output + tokens.cache_read + tokens.cache_write;
            entry.api_usd = entry.api_usd.zip(price.map(|price| price.of(TokenUse::from(tokens)))).map(|(sum, cost)| sum + cost);
        }
        Ok(sessions.into_values().collect())
    }

    fn landed_since(&self, since_ms: u64) -> NorthResult<Vec<Landing>> {
        let mut landed: Vec<Landing> = self.landings.lock().unwrap().values().filter(|landing| landing.landed_ms >= since_ms).cloned().collect();
        landed.sort_by_key(|landing| std::cmp::Reverse(landing.landed_ms));
        Ok(landed)
    }

    fn record_landing(&self, task_ref: &str, sha: &str, landed_ms: u64, wall_ms: Option<u64>) -> NorthResult<bool> {
        let started = self.runs.lock().unwrap().iter().filter(|run| run.task_ref.as_deref() == Some(task_ref)).map(|run| run.turn.started_ms).min();
        let mut landings = self.landings.lock().unwrap();
        if landings.get(task_ref).is_some_and(|landing| landing.sha == sha) {
            return Ok(false);
        }
        let wall_ms = started.map(|started| landed_ms.saturating_sub(started)).or(wall_ms);
        landings.insert(task_ref.to_owned(), Landing { task_ref: task_ref.to_owned(), sha: sha.to_owned(), wall_ms, landed_ms });
        Ok(true)
    }
}

pub struct Stream;

impl Runtime for Stream {
    fn provider(&self) -> ProviderName {
        ProviderName::new("claude")
    }

    fn models(&self) -> &'static [&'static str] {
        &[]
    }

    fn roster(&self, _state_root: &Path, _home: &Path, _policy: &Path) -> Roster {
        Roster::default()
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        new_uuid().map(Some)
    }

    fn host(&self, _account: &super::model::Account, request: &LaunchRequest, _opening: Opening, _instructions: &str) -> NorthResult<HostSpec> {
        Ok(HostSpec { key: request.name.clone(), shared: false, argv: vec!["true".into()], env: Vec::new(), env_remove: Vec::new(), cwd: request.cwd.clone() })
    }

    fn handshake(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(&self, _name: &str, _request: &LaunchRequest, _opening: Opening, _instructions: Instructions) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn message(&self, _transcript: &Transcript, id: &str, text: &str) -> NorthResult<Vec<String>> {
        Ok(vec![encode_user_message(id, text)])
    }

    fn interrupt(&self, _transcript: &Transcript) -> NorthResult<Vec<String>> {
        Ok(vec![encode_interrupt(&format!("interrupt-{}", new_uuid()?))])
    }

    fn close(&self, _transcript: &Transcript) -> Vec<String> {
        Vec::new()
    }

    fn observe(&self, frame: &Value, transcript: &mut Transcript) {
        match frame["type"].as_str() {
            Some("user") => {
                transcript.status = Status::Working;
                transcript.active_turn = frame["uuid"].as_str().map(str::to_owned);
                push(transcript, EntryKind::User, text(frame));
            }
            Some("assistant") => {
                transcript.status = Status::Working;
                push(transcript, EntryKind::Assistant, text(frame));
            }
            Some("result") => {
                transcript.status = Status::Idle;
                transcript.active_turn = None;
                push(transcript, EntryKind::TurnEnd, frame["subtype"].as_str().unwrap_or("result").to_owned());
            }
            _ if transcript.status == Status::Starting => transcript.status = Status::Idle,
            _ => {}
        }
    }

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        let Some(conversation) = frame["session_id"].as_str().map(str::to_owned) else { return Vec::new() };
        match frame["type"].as_str() {
            Some("user") => vec![TurnSignal::Started { conversation: conversation.clone() }, TurnSignal::Prompt { conversation, text: text(frame) }],
            Some("assistant") => vec![TurnSignal::Started { conversation }],
            Some("result") => vec![TurnSignal::Ended { conversation, exit: if frame["is_error"] == true { TurnExit::Failed } else { TurnExit::Completed }, totals: totals(frame) }],
            _ => Vec::new(),
        }
    }

    fn frame_conversation<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        frame["session_id"].as_str()
    }

    fn quota(&self, _frame: &Value) -> Option<Quota> {
        None
    }

    fn sent_prompt(&self, frame: &Value) -> Option<String> {
        (frame["type"] == "user").then(|| frame["uuid"].as_str().map(str::to_owned)).flatten()
    }

    fn accepted_prompt(&self, frame: &Value) -> Option<String> {
        (frame["type"] == "user" && frame["isReplay"] == true).then(|| frame["uuid"].as_str().map(str::to_owned)).flatten()
    }
}

fn text(frame: &Value) -> String {
    let content = &frame["message"]["content"];
    content.as_str().map_or_else(|| content.as_array().into_iter().flatten().filter_map(|block| block["text"].as_str()).collect::<Vec<_>>().join("\n"), str::to_owned)
}

fn totals(frame: &Value) -> TurnTotals {
    let usage = &frame["usage"];
    let count = |key: &str| usage[key].as_u64().unwrap_or(0);
    let tokens = usage.is_object().then(|| Tokens { input: count("input_tokens"), output: count("output_tokens"), cache_read: count("cache_read_input_tokens"), cache_write: count("cache_creation_input_tokens") });
    TurnTotals { tokens, turns: frame["num_turns"].as_u64(), wall_ms: frame["duration_ms"].as_u64() }
}

fn push(transcript: &mut Transcript, kind: EntryKind, text: String) {
    if !text.is_empty() {
        transcript.entries.push(Entry { kind, text });
    }
}
