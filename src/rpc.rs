use std::collections::BTreeMap;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::error::{NorthError, NorthResult};

#[derive(Clone)]
enum Failure {
    Rejected(String),
    Disconnected(String),
}

type Reply = Result<Value, Failure>;
pub type Events = broadcast::Receiver<Result<Value, String>>;

enum Outgoing {
    Request {
        method: String,
        params: Value,
        reply: oneshot::Sender<Reply>,
    },
    Message(Value, oneshot::Sender<Reply>),
    Close,
}

/// The JSONL reader owns response correlation; notifications and server requests
/// are separate from client replies, including when their numeric IDs coincide.
#[derive(Clone)]
pub struct Rpc {
    outgoing: mpsc::Sender<Outgoing>,
    events: broadcast::Sender<Result<Value, String>>,
}

impl Rpc {
    pub fn start<R, W>(reader: R, writer: W) -> (Self, Events, JoinHandle<()>)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (outgoing, commands) = mpsc::channel(64);
        let (events, receiver) = broadcast::channel(1024);
        let task_events = events.clone();
        let task = tokio::spawn(async move {
            drive(reader, writer, commands, task_events).await;
        });
        (Self { outgoing, events }, receiver, task)
    }

    pub fn subscribe(&self) -> Events {
        self.events.subscribe()
    }

    pub async fn request(&self, method: &str, params: Value) -> NorthResult<Value> {
        let (reply, received) = oneshot::channel();
        self.outgoing
            .send(Outgoing::Request { method: method.into(), params, reply })
            .await
            .map_err(|_| disconnected())?;
        receive_reply(received).await
    }

    pub async fn send(&self, message: Value) -> NorthResult<()> {
        let (reply, received) = oneshot::channel();
        self.outgoing.send(Outgoing::Message(message, reply)).await
            .map_err(|_| disconnected())?;
        receive_reply(received).await.map(|_| ())
    }

    pub async fn close(&self) {
        let _ = self.outgoing.send(Outgoing::Close).await;
    }
}

fn disconnected() -> NorthError {
    NorthError::Protocol("Codex connection closed".into())
}

async fn receive_reply(reply: oneshot::Receiver<Reply>) -> NorthResult<Value> {
    reply.await.map_err(|_| disconnected())?.map_err(|error| match error {
        Failure::Rejected(message) => NorthError::Rejected(message),
        Failure::Disconnected(message) => NorthError::Protocol(message),
    })
}

pub async fn next_event(events: &mut Events) -> NorthResult<Value> {
    events.recv().await.map_err(|error| {
        NorthError::Protocol(format!("Codex event stream needs reconciliation: {error}"))
    })?.map_err(NorthError::Protocol)
}

async fn write_message(writer: &mut (impl AsyncWrite + Unpin), message: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(message).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    writer.write_all(&bytes).await.map_err(|error| error.to_string())?;
    writer.flush().await.map_err(|error| error.to_string())
}

async fn drive(
    reader: impl AsyncRead + Unpin,
    mut writer: impl AsyncWrite + Unpin,
    mut commands: mpsc::Receiver<Outgoing>,
    events: broadcast::Sender<Result<Value, String>>,
) {
    let mut lines = BufReader::new(reader).lines();
    let mut next_id = 1_u64;
    let mut pending: BTreeMap<u64, (String, oneshot::Sender<Reply>)> = BTreeMap::new();
    let failure = loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Outgoing::Request { method, params, reply }) => {
                    let id = next_id;
                    next_id += 1;
                    let message = json!({"id": id, "method": method, "params": params});
                    pending.insert(id, (method, reply));
                    if let Err(error) = write_message(&mut writer, &message).await {
                        break error;
                    }
                }
                Some(Outgoing::Message(message, reply)) => {
                    let result = write_message(&mut writer, &message).await;
                    let _ = reply.send(result.clone().map(|()| Value::Null).map_err(Failure::Disconnected));
                    if let Err(error) = result { break error; }
                }
                Some(Outgoing::Close) | None => break "Codex connection closed".into(),
            },
            line = lines.next_line() => {
                let line = match line {
                    Ok(Some(line)) if line.trim().is_empty() => continue,
                    Ok(Some(line)) => line,
                    Ok(None) => break "Codex connection closed".into(),
                    Err(error) => break error.to_string(),
                };
                let message: Value = match serde_json::from_str(&line) {
                    Ok(message) => message,
                    Err(error) => break format!("Invalid Codex message: {error}"),
                };
                if message.get("method").is_some() {
                    let _ = events.send(Ok(message));
                } else if let Some((method, reply)) = message.get("id").and_then(Value::as_u64)
                    .and_then(|id| pending.remove(&id))
                {
                    let result = if let Some(error) = message.get("error") {
                        Err(Failure::Rejected(format!("{method}: {error}")))
                    } else {
                        message.get("result").cloned().ok_or_else(|| Failure::Disconnected(format!("{method} omitted result")))
                    };
                    let _ = reply.send(result);
                }
            }
        }
    };
    for (_, (_, reply)) in pending {
        let _ = reply.send(Err(Failure::Disconnected(failure.clone())));
    }
    let _ = events.send(Err(failure));
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, split};

    #[tokio::test]
    async fn responses_do_not_consume_progress_or_server_requests() {
        let (client, server) = duplex(8192);
        let (reader, writer) = split(client);
        let (rpc, mut events, driver) = Rpc::start(reader, writer);
        let server = tokio::spawn(async move {
            let (reader, mut writer) = split(server);
            let mut lines = BufReader::new(reader).lines();
            let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            write_message(&mut writer, &json!({"method":"item/agentMessage/delta","params":{"delta":"working"}})).await.unwrap();
            // The server's request ID is independent of our request namespace.
            write_message(&mut writer, &json!({"id":request["id"],"method":"item/tool/requestUserInput","params":{}})).await.unwrap();
            write_message(&mut writer, &json!({"id":request["id"],"result":{"accepted":true}})).await.unwrap();
            let response: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(response, json!({"id":request["id"],"result":{"answers":{}}}));
        });
        assert_eq!(rpc.request("turn/start", json!({})).await.unwrap(), json!({"accepted":true}));
        assert_eq!(next_event(&mut events).await.unwrap()["params"]["delta"], "working");
        let question = next_event(&mut events).await.unwrap();
        rpc.send(json!({"id":question["id"],"result":{"answers":{}}})).await.unwrap();
        server.await.unwrap();
        driver.await.unwrap();
        assert!(next_event(&mut events).await.is_err());
    }

    #[tokio::test]
    async fn disconnect_releases_pending_requests() {
        let (client, server) = duplex(1024);
        let (reader, writer) = split(client);
        let (rpc, _events, driver) = Rpc::start(reader, writer);
        let server = tokio::spawn(async move {
            let mut reader = BufReader::new(server);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
        });
        assert!(rpc.request("turn/start", json!({})).await.is_err());
        server.await.unwrap();
        driver.await.unwrap();
    }
}
