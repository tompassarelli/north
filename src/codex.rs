use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::error::{NorthError, NorthResult};
use crate::providers::codex::{accounts, launch};
use crate::rpc::{self, Events, Rpc};

const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
const DELEGATION_COORDINATOR_INSTRUCTIONS: &str = r#"You are North's explicit delegation coordinator for this turn.

If your task identity is /root, call collaboration.spawn_agent exactly once and give that child the entire operator task. Do not perform any part of the task yourself and do not finish without a successful spawn receipt. Wait until that exact child reaches a terminal completed state. If the child fails, do not claim success. After the child completes, follow the operator's requested final-reply instruction exactly. Keep the final reply free of progress narration.

If you are the spawned child rather than /root, execute the assigned task directly. Do not delegate it again."#;

#[derive(Debug, Eq, PartialEq)]
pub struct DelegationOutcome {
    pub child_id: String,
    pub answer: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct TurnOutcome {
    pub answer: String,
    pub commands: Vec<CommandOutcome>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutcome {
    pub command: String,
    pub succeeded: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationOption {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub current: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationSnapshot {
    pub id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub entries: Vec<ChatUpdate>,
    pub turns: Vec<TurnObservation>,
    pub accepted_inputs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnObservation {
    pub id: String,
    pub status: String,
}

pub struct ResumedConversation {
    pub snapshot: ConversationSnapshot,
    pub session: TurnSession,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatUpdate {
    pub conversation: String,
    pub turn: String,
    pub key: String,
    pub kind: String,
    pub text: String,
    pub status: String,
    pub append: bool,
}

/// Convert app-server item payloads at the foreign boundary. Ordering and
/// replacement of these observations belongs to the checked application.
pub fn chat_updates(message: &Value) -> Vec<ChatUpdate> {
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];
    let Some(conversation) = params["threadId"].as_str() else { return Vec::new(); };
    let turn = params["turnId"].as_str().or_else(|| params["turn"]["id"].as_str()).unwrap_or_default();
    let update = |key: &str, kind: &str, text: String, status: &str, append: bool| ChatUpdate {
        conversation: conversation.into(), turn: turn.into(), key: key.into(),
        kind: kind.into(), text, status: status.into(), append,
    };
    let item_update = |item: &Value| decode_chat_item(conversation, turn, item,
        if method == "item/started" { "inProgress" } else { "completed" });
    match method {
        "item/started" | "item/completed" => item_update(&params["item"]).into_iter().collect(),
        "item/agentMessage/delta" | "item/commandExecution/outputDelta" => {
            let (Some(key), Some(delta)) = (params["itemId"].as_str(), params["delta"].as_str()) else { return Vec::new(); };
            let kind = if method == "item/agentMessage/delta" { "agentMessage" } else { "commandExecution" };
            vec![update(key, kind, delta.into(), "inProgress", true)]
        }
        "turn/diff/updated" => params["diff"].as_str().map(|diff| update("turn-diff", "diff", diff.into(), "completed", false)).into_iter().collect(),
        "turn/completed" => params["turn"]["items"].as_array().into_iter().flatten().filter_map(item_update).collect(),
        _ => Vec::new(),
    }
}

fn decode_chat_item(conversation: &str, turn: &str, item: &Value, default_status: &str) -> Option<ChatUpdate> {
    let kind = item["type"].as_str()?;
    let key = if kind == "userMessage" { item["clientId"].as_str().or_else(|| item["id"].as_str())? }
        else { item["id"].as_str()? };
    let status = item["status"].as_str().unwrap_or(default_status);
    let text = match kind {
        "userMessage" => decode_user_message(item).ok()??,
        "agentMessage" => item["text"].as_str()?.to_owned(),
        "commandExecution" => {
            let command = item["command"].as_str()?;
            let output = item["aggregatedOutput"].as_str().unwrap_or_default();
            format!("{command}\n{output}")
        }
        "fileChange" => item["changes"].as_array()?.iter().map(|change| {
            let path = change["path"].as_str().unwrap_or_default();
            let diff = change["diff"].as_str().unwrap_or_default();
            format!("{path}\n{diff}")
        }).collect::<Vec<_>>().join("\n"),
        _ => return None,
    };
    Some(ChatUpdate { conversation: conversation.into(), turn: turn.into(), key: key.into(), kind: kind.into(), text, status: status.into(), append: false })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReasoningOption {
    pub effort: String,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelOption {
    pub model: String,
    pub description: String,
    pub reasoning: Vec<ReasoningOption>,
    pub default_effort: String,
    pub is_default: bool,
}

pub struct Codex {
    owned_server: Option<OwnedServer>,
    rpc: Rpc,
    rpc_task: JoinHandle<()>,
    thread_id: Option<String>,
    model: String,
    reasoning_effort: String,
    models: Vec<ModelOption>,
}

struct OwnedServer {
    child: Child,
    stderr: mpsc::Receiver<String>,
    stderr_task: JoinHandle<()>,
}

impl Codex {
    pub async fn start(cwd: &Path) -> NorthResult<Self> {
        let mut codex = Self::connect(cwd).await?;
        codex.start_new_conversation(cwd).await?;
        Ok(codex)
    }

    pub async fn connect(cwd: &Path) -> NorthResult<Self> {
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let server = launch::app_server(&accounts::pooled_home(&home))?;
        let mut command = Command::new(&server.argv[0]);
        command
            .args(&server.argv[1..])
            .envs(server.env)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| NorthError::Protocol("Codex app-server stdin is unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| NorthError::Protocol("Codex app-server stdout is unavailable".into()))?;
        let child_stderr = child
            .stderr
            .take()
            .ok_or_else(|| NorthError::Protocol("Codex app-server stderr is unavailable".into()))?;
        let (stderr_tx, stderr) = mpsc::channel(64);
        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(child_stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = stderr_tx.try_send(line);
            }
        });
        let (rpc, _events, rpc_task) = Rpc::start(stdout, stdin);
        Self::initialize_connection(rpc, rpc_task, OwnedServer { child, stderr, stderr_task }).await
    }

    async fn initialize_connection(rpc: Rpc, rpc_task: JoinHandle<()>, owned_server: OwnedServer) -> NorthResult<Self> {
        let mut codex = Self {
            owned_server: Some(owned_server),
            rpc,
            rpc_task,
            thread_id: None,
            model: String::new(),
            reasoning_effort: String::new(),
            models: Vec::new(),
        };
        codex.initialize().await?;
        let models = codex.model_catalog().await?;
        let selection = default_model_selection(&models)
            .map_err(|message| codex.protocol_error(&message, &serde_json::Value::Null))?;
        codex.model = selection.model;
        codex.reasoning_effort = selection.reasoning_effort;
        codex.models = models;
        Ok(codex)
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn reasoning_effort(&self) -> &str {
        &self.reasoning_effort
    }

    pub fn models(&self) -> &[ModelOption] {
        &self.models
    }

    pub fn connection(&self) -> Rpc {
        self.rpc.clone()
    }

    pub async fn start_new_conversation(&mut self, cwd: &Path) -> NorthResult<String> {
        let selection = ModelSelection {
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
        };
        let thread_id = self.start_thread(cwd, &selection).await?;
        self.thread_id = Some(thread_id.clone());
        Ok(thread_id)
    }

    pub async fn conversations(&mut self, cwd: &Path, archived: bool) -> NorthResult<Vec<ConversationOption>> {
        self.list_conversations(
                json!({
                    "limit": 100,
                    "sortKey": "recency_at",
                    "sortDirection": "desc",
                    "cwd": cwd,
                    "archived": archived,
                    "sourceKinds": ["appServer", "cli", "vscode", "unknown"],
                    "useStateDbOnly": true
                }),
            )
            .await
    }

    pub async fn list_conversations(&mut self, mut parameters: Value) -> NorthResult<Vec<ConversationOption>> {
        let mut conversations = Vec::new();
        let mut cursors = std::collections::BTreeSet::new();
        loop {
            let result = self.request("thread/list", parameters.clone()).await?;
            conversations.extend(decode_conversations(&result, self.thread_id.as_deref())
                .map_err(|message| self.protocol_error(&message, &result))?);
            let Some(cursor) = result.get("nextCursor").and_then(Value::as_str) else {
                return Ok(conversations);
            };
            if !cursors.insert(cursor.to_owned()) {
                return Err(self.protocol_error("thread/list repeated its cursor", &result));
            }
            parameters["cursor"] = Value::String(cursor.to_owned());
        }
    }

    pub async fn resume_conversation(
        &mut self,
        conversation_id: &str,
    ) -> NorthResult<ResumedConversation> {
        let resumed = TurnSession::resume(self.rpc.clone(), conversation_id).await?;
        let snapshot = &resumed.snapshot;
        self.thread_id = Some(snapshot.id.clone());
        self.model = snapshot.model.clone();
        self.reasoning_effort = snapshot.reasoning_effort.clone();
        Ok(resumed)
    }

    pub fn select_attached_conversation(&mut self, id: &str, model: &str, effort: &str) {
        self.thread_id = Some(id.into());
        self.model = model.into();
        self.reasoning_effort = effort.into();
    }

    pub async fn rename_conversation(&mut self, id: &str, name: &str) -> NorthResult<()> {
        self.request("thread/name/set", json!({"threadId": id, "name": name})).await.map(|_| ())
    }

    pub async fn archive_conversation(&mut self, id: &str) -> NorthResult<()> {
        self.request("thread/archive", json!({"threadId": id})).await.map(|_| ())
    }

    pub async fn restore_conversation(&mut self, id: &str) -> NorthResult<()> {
        self.request("thread/unarchive", json!({"threadId": id})).await.map(|_| ())
    }

    pub async fn fork_conversation(&mut self, id: &str) -> NorthResult<ResumedConversation> {
        let mut session = TurnSession::new(self.rpc.clone(), String::new(), self.model.clone());
        let result = self.request("thread/fork", json!({"threadId": id})).await?;
        let snapshot = decode_conversation_snapshot(&result)
            .map_err(|message| self.protocol_error(&message, &result))?;
        session.thread_id = Some(snapshot.id.clone());
        session.model = snapshot.model.clone();
        self.select_attached_conversation(&snapshot.id, &snapshot.model, &snapshot.reasoning_effort);
        Ok(ResumedConversation { snapshot, session })
    }

    pub async fn set_model_and_effort(&mut self, model: &str, effort: &str) -> NorthResult<()> {
        let supported = self.models.iter().any(|candidate| {
            candidate.model == model
                && candidate
                    .reasoning
                    .iter()
                    .any(|candidate| candidate.effort == effort)
        });
        if !supported {
            return Err(NorthError::Configuration(format!(
                "{model} does not support {effort} reasoning"
            )));
        }
        let thread_id = self.require_thread_id()?.to_owned();
        self.request(
            "thread/settings/update",
            thread_settings_update_params(&thread_id, model, effort),
        )
        .await?;
        self.model = model.to_owned();
        self.reasoning_effort = effort.to_owned();
        Ok(())
    }

    pub fn turn_session(&self) -> NorthResult<TurnSession> {
        Ok(TurnSession::new(self.rpc.clone(), self.require_thread_id()?.to_owned(), self.model.clone()))
    }

    #[cfg(test)]
    pub async fn run_turn(&mut self, prompt: &str) -> NorthResult<TurnOutcome> {
        self.turn_session()?.run_turn(prompt).await
    }

    pub async fn shutdown(mut self) -> NorthResult<()> {
        self.rpc.close().await;
        let _ = (&mut self.rpc_task).await;
        let Some(mut server) = self.owned_server.take() else { return Ok(()); };
        let status = match timeout(SHUTDOWN_GRACE, server.child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                server.child.kill().await?;
                server.child.wait().await?
            }
        };
        server.stderr_task.abort();
        if status.success() {
            Ok(())
        } else {
            Err(NorthError::AppServerExit(status))
        }
    }

    async fn initialize(&mut self) -> NorthResult<()> {
        self.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "north",
                    "title": "North fleet",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }),
        )
        .await?;
        self.send(&json!({"method": "initialized", "params": {}}))
            .await
    }

    async fn start_thread(
        &mut self,
        cwd: &Path,
        selection: &ModelSelection,
    ) -> NorthResult<String> {
        let result = self
            .request("thread/start", thread_start_params(cwd, selection))
            .await?;
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| self.protocol_error("thread/start omitted thread.id", &result))
    }



    async fn model_catalog(&mut self) -> NorthResult<Vec<ModelOption>> {
        let result = self
            .request("model/list", json!({"limit": 100, "includeHidden": false}))
            .await?;
        decode_model_catalog(&result).map_err(|message| self.protocol_error(&message, &result))
    }

    async fn request(&mut self, method: &str, params: Value) -> NorthResult<Value> {
        self.rpc.request(method, params).await
    }

    async fn send(&mut self, message: &Value) -> NorthResult<()> {
        self.rpc.send(message.clone()).await
    }



    fn require_thread_id(&self) -> NorthResult<&str> {
        self.thread_id
            .as_deref()
            .ok_or_else(|| NorthError::Protocol("Codex has no active conversation".into()))
    }

    fn protocol_error(&mut self, message: &str, value: &Value) -> NorthError {
        let mut stderr = VecDeque::new();
        if let Some(server) = &mut self.owned_server {
            while let Ok(line) = server.stderr.try_recv() {
                if stderr.len() == 8 {
                    stderr.pop_front();
                }
                stderr.push_back(line);
            }
        }
        let suffix = if stderr.is_empty() {
            String::new()
        } else {
            format!(
                "; app-server stderr: {}",
                stderr.into_iter().collect::<Vec<_>>().join(" | ")
            )
        };
        NorthError::Protocol(format!("{message}: {value}{suffix}"))
    }
}

pub struct TurnSession {
    rpc: Rpc,
    events: Events,
    thread_id: Option<String>,
    model: String,
}

impl TurnSession {
    pub async fn resume(rpc: Rpc, conversation: &str) -> NorthResult<ResumedConversation> {
        // Subscribe before resume: completion can precede the RPC response.
        let mut session = Self::new(rpc, conversation.into(), String::new());
        let result = session.request("thread/resume", json!({"threadId": conversation})).await?;
        let snapshot = decode_conversation_snapshot(&result)
            .map_err(|message| session.protocol_error(&message, &result))?;
        if snapshot.id != conversation {
            return Err(session.protocol_error("thread/resume returned a different conversation", &result));
        }
        session.model = snapshot.model.clone();
        Ok(ResumedConversation { snapshot, session })
    }

    pub fn new(rpc: Rpc, thread_id: String, model: String) -> Self {
        let events = rpc.subscribe();
        Self { rpc, events, thread_id: Some(thread_id), model }
    }

    #[cfg(test)]
    pub async fn run_turn(&mut self, prompt: &str) -> NorthResult<TurnOutcome> {
        let (_interrupt_tx, interrupt_rx) = oneshot::channel();
        self.run_turn_interruptible(prompt, &[], None, interrupt_rx).await
    }

    pub async fn run_turn_interruptible(
        &mut self,
        prompt: &str,
        local_images: &[PathBuf],
        client_id: Option<&str>,
        interrupt: oneshot::Receiver<()>,
    ) -> NorthResult<TurnOutcome> {
        self.events = self.rpc.subscribe();
        let thread_id = self.require_thread_id()?.to_owned();
        let turn_id = self
            .start_turn(json!({
                "threadId": thread_id.clone(),
                "clientUserMessageId": client_id,
                "input": turn_input(prompt, local_images)
            }))
            .await?;
        self.wait_for_turn(&turn_id, interrupt).await
    }

    pub async fn wait_for_turn(
        &mut self,
        turn_id: &str,
        interrupt: oneshot::Receiver<()>,
    ) -> NorthResult<TurnOutcome> {
        let params = self.wait_for_completion(turn_id, interrupt).await?;
        turn_outcome(&params).map_err(|message| self.protocol_error(&message, &params))
    }

    pub async fn compact(&mut self, interrupt: oneshot::Receiver<()>) -> NorthResult<()> {
        self.events = self.rpc.subscribe();
        let thread = self.require_thread_id()?.to_owned();
        self.request("thread/compact/start", json!({"threadId": thread})).await?;
        let turn = loop {
            let event = self.read_message().await?;
            if event["method"] == "turn/started" && event["params"]["threadId"] == thread {
                break event["params"]["turn"]["id"].as_str()
                    .ok_or_else(|| self.protocol_error("Compaction start omitted turn identity", &event))?.to_owned();
            }
        };
        let params = self.wait_for_completion(&turn, interrupt).await?;
        if params["turn"]["status"] == "completed" { Ok(()) }
        else { Err(self.protocol_error("Compaction did not complete", &params)) }
    }

    async fn wait_for_completion(
        &mut self,
        turn_id: &str,
        mut interrupt: oneshot::Receiver<()>,
    ) -> NorthResult<Value> {
        let mut interrupt_sent = false;

        loop {
            let message = tokio::select! {
                signal = &mut interrupt, if !interrupt_sent => {
                    interrupt_sent = true;
                    if signal.is_ok() {
                        self.send_interrupt(turn_id).await?;
                    }
                    continue;
                }
                message = self.read_message() => message?,
            };
            if message.get("method").and_then(Value::as_str) != Some("turn/completed") {
                continue;
            }
            let params = message
                .get("params")
                .ok_or_else(|| self.protocol_error("turn/completed omitted params", &message))?;
            if params.get("threadId").and_then(Value::as_str) != self.thread_id.as_deref() {
                continue;
            }
            if params.pointer("/turn/id").and_then(Value::as_str) != Some(turn_id) {
                continue;
            }
            if params.pointer("/turn/status").and_then(Value::as_str) == Some("interrupted") {
                return Err(NorthError::Interrupted);
            }
            return Ok(params.clone());
        }
    }

    pub async fn run_delegate_interruptible<F>(
        &mut self,
        prompt: &str,
        local_images: &[PathBuf],
        mut child_spawned: F,
        mut interrupt: oneshot::Receiver<()>,
    ) -> NorthResult<DelegationOutcome>
    where
        F: FnMut(&str) -> NorthResult<()>,
    {
        self.events = self.rpc.subscribe();
        let model = self.model.clone();
        let thread_id = self.require_thread_id()?.to_owned();
        let turn_id = self
            .start_turn(json!({
                "threadId": thread_id.clone(),
                "input": turn_input(prompt, local_images),
                "collaborationMode": {
                    "mode": "default",
                    "settings": {
                        "model": model,
                        "reasoning_effort": "high",
                        "developer_instructions": DELEGATION_COORDINATOR_INSTRUCTIONS
                    }
                }
            }))
            .await?;
        let mut tracker = DelegationTracker::new(thread_id, turn_id);
        let mut interrupt_sent = false;

        loop {
            let message = tokio::select! {
                signal = &mut interrupt, if !interrupt_sent => {
                    interrupt_sent = true;
                    if signal.is_ok() {
                        self.send_interrupt(&tracker.parent_turn_id).await?;
                    }
                    continue;
                }
                message = self.read_message() => message?,
            };
            let observation = match tracker.observe(&message) {
                Ok(observation) => observation,
                Err(error) => return Err(self.protocol_error(&error, &message)),
            };
            if let Some(child_id) = observation {
                child_spawned(&child_id)?;
            }
            if message.get("method").and_then(Value::as_str) != Some("turn/completed") {
                continue;
            }
            let params = message
                .get("params")
                .ok_or_else(|| self.protocol_error("turn/completed omitted params", &message))?;
            if !tracker.is_parent_completion(params) {
                continue;
            }
            if params.pointer("/turn/status").and_then(Value::as_str) == Some("interrupted") {
                return Err(NorthError::Interrupted);
            }
            return tracker
                .finish(params)
                .map_err(|message| self.protocol_error(&message, params));
        }
    }

    async fn start_turn(&mut self, params: Value) -> NorthResult<String> {
        let result = self.request("turn/start", params).await?;
        result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| self.protocol_error("turn/start omitted turn.id", &result))
    }

    async fn send_interrupt(&mut self, turn_id: &str) -> NorthResult<()> {
        let thread_id = self.require_thread_id()?.to_owned();
        self.request("turn/interrupt", json!({"threadId": thread_id, "turnId": turn_id}))
            .await.map(|_| ())
    }

    async fn request(&self, method: &str, params: Value) -> NorthResult<Value> {
        self.rpc.request(method, params).await
    }

    async fn read_message(&mut self) -> NorthResult<Value> {
        rpc::next_event(&mut self.events).await
    }

    fn require_thread_id(&self) -> NorthResult<&str> {
        self.thread_id.as_deref().ok_or_else(|| NorthError::Protocol("No conversation selected".into()))
    }

    fn protocol_error(&self, message: &str, value: &Value) -> NorthError {
        NorthError::Protocol(format!("{message}: {value}"))
    }
}

pub async fn steer_turn(
    rpc: &Rpc, thread_id: &str, turn_id: &str, text: &str, images: &[PathBuf], client_id: &str,
) -> NorthResult<()> {
    let result = rpc.request("turn/steer", json!({
        "threadId": thread_id, "expectedTurnId": turn_id, "input": turn_input(text, images),
        "clientUserMessageId": client_id,
    })).await?;
    if result["turnId"].as_str() != Some(turn_id) {
        return Err(NorthError::Protocol("Steering receipt did not identify the expected turn".into()));
    }
    Ok(())
}

impl Drop for Codex {
    fn drop(&mut self) {
        self.rpc_task.abort();
        if let Some(server) = &self.owned_server {
            server.stderr_task.abort();
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
struct ModelSelection {
    model: String,
    reasoning_effort: String,
}

fn decode_model_catalog(result: &Value) -> Result<Vec<ModelOption>, String> {
    let models = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "model/list omitted data".to_string())?;
    models
        .iter()
        .map(|model| {
            let slug = model
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| "model entry omitted model".to_string())?;
            let default_effort = model
                .get("defaultReasoningEffort")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("model {slug} omitted defaultReasoningEffort"))?;
            let reasoning = model
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("model {slug} omitted supportedReasoningEfforts"))?
                .iter()
                .map(|option| {
                    Ok(ReasoningOption {
                        effort: option
                            .get("reasoningEffort")
                            .and_then(Value::as_str)
                            .ok_or_else(|| format!("model {slug} has an unnamed effort"))?
                            .to_owned(),
                        description: option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok(ModelOption {
                model: slug.to_owned(),
                description: model
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                reasoning,
                default_effort: default_effort.to_owned(),
                is_default: model.get("isDefault").and_then(Value::as_bool) == Some(true),
            })
        })
        .collect()
}

fn default_model_selection(models: &[ModelOption]) -> Result<ModelSelection, String> {
    let defaults = models
        .iter()
        .filter(|model| model.is_default)
        .collect::<Vec<_>>();
    let [selected] = defaults.as_slice() else {
        return Err("model/list did not identify exactly one default model".into());
    };
    Ok(ModelSelection {
        model: selected.model.clone(),
        reasoning_effort: selected.default_effort.clone(),
    })
}

fn decode_conversations(
    result: &Value,
    current_thread_id: Option<&str>,
) -> Result<Vec<ConversationOption>, String> {
    let conversations = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "thread/list omitted data".to_string())?;
    conversations
        .iter()
        .map(|conversation| {
            let id = conversation
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "thread/list entry omitted id".to_string())?;
            let preview = conversation
                .get("preview")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_owned();
            let title = conversation
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .or_else(|| (!preview.is_empty()).then_some(preview.as_str()))
                .unwrap_or("Untitled conversation")
                .to_owned();
            Ok(ConversationOption {
                id: id.to_owned(),
                title,
                preview,
                current: current_thread_id == Some(id),
            })
        })
        .collect()
}

fn decode_conversation_snapshot(result: &Value) -> Result<ConversationSnapshot, String> {
    let thread = result
        .get("thread")
        .ok_or_else(|| "thread/resume omitted thread".to_string())?;
    let id = thread
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "thread/resume omitted thread.id".to_string())?;
    let model = result
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| "thread/resume omitted model".to_string())?;
    let reasoning_effort = result
        .get("reasoningEffort")
        .and_then(Value::as_str)
        .unwrap_or("default");
    let turns = thread
        .get("turns")
        .and_then(Value::as_array)
        .ok_or_else(|| "thread/resume omitted thread.turns".to_string())?;
    let mut entries = Vec::new();
    let mut observations = Vec::new();
    let mut accepted_inputs = Vec::new();
    for turn in turns {
        decode_turn_history(id, turn, &mut entries)?;
        accepted_inputs.extend(accepted_user_inputs(turn["items"].as_array().map(Vec::as_slice).unwrap_or_default()));
        observations.push(TurnObservation {
            id: turn["id"].as_str().ok_or("thread turn omitted id")?.into(),
            status: turn["status"].as_str().unwrap_or_default().into(),
        });
    }
    Ok(ConversationSnapshot {
        id: id.to_owned(),
        model: model.to_owned(),
        reasoning_effort: reasoning_effort.to_owned(),
        entries,
        turns: observations,
        accepted_inputs,
    })
}

pub fn accepted_user_inputs(items: &[Value]) -> impl Iterator<Item = String> + '_ {
    items.iter().filter(|item| item["type"] == "userMessage")
        .filter_map(|item| item["clientId"].as_str().map(str::to_owned))
}

fn decode_turn_history(conversation: &str, turn: &Value, entries: &mut Vec<ChatUpdate>) -> Result<(), String> {
    let turn_id = turn["id"].as_str().ok_or("thread turn omitted id")?;
    let items = turn
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "thread turn omitted items".to_string())?;
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("userMessage") => {
                if let Some(message) = decode_user_message(item)? {
                    let key = item["clientId"].as_str().or_else(|| item["id"].as_str()).ok_or("userMessage omitted id")?;
                    entries.push(ChatUpdate { conversation: conversation.into(), turn: turn_id.into(), key: key.into(), kind: "userMessage".into(), text: message, status: "completed".into(), append: false });
                }
            }
            Some("agentMessage" | "commandExecution" | "fileChange") => {
                entries.push(decode_chat_item(conversation, turn_id, item, "completed")
                    .ok_or("Stored conversation item is incomplete")?);
            }
            _ => {}
        }
    }
    Ok(())
}

fn decode_user_message(item: &Value) -> Result<Option<String>, String> {
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "userMessage omitted content".to_string())?;
    let mut parts = Vec::new();
    let mut image_number = 1;
    for input in content {
        match input.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = input.get("text").and_then(Value::as_str)
                    && !text.is_empty()
                {
                    parts.push(text.to_owned());
                }
            }
            Some("image" | "localImage") => {
                parts.push(format!("[Image #{image_number}]"));
                image_number += 1;
            }
            Some("audio" | "localAudio") => parts.push("[Audio]".into()),
            _ => {}
        }
    }
    let message = parts.join(" ");
    Ok((!message.is_empty()).then_some(message))
}

fn thread_start_params(cwd: &Path, selection: &ModelSelection) -> Value {
    json!({
        "cwd": cwd,
        "model": selection.model,
        "config": {"model_reasoning_effort": selection.reasoning_effort},
        "sandbox": "workspace-write"
    })
}

fn thread_settings_update_params(thread_id: &str, model: &str, effort: &str) -> Value {
    json!({"threadId": thread_id, "model": model, "effort": effort})
}

fn turn_input(prompt: &str, local_images: &[PathBuf]) -> Vec<Value> {
    let mut input = vec![json!({"type": "text", "text": prompt})];
    input.extend(
        local_images
            .iter()
            .map(|path| json!({"type": "localImage", "path": path})),
    );
    input
}

struct DelegationTracker {
    parent_thread_id: String,
    parent_turn_id: String,
    child_id: Option<String>,
    child_completed: bool,
}

impl DelegationTracker {
    fn new(parent_thread_id: String, parent_turn_id: String) -> Self {
        Self {
            parent_thread_id,
            parent_turn_id,
            child_id: None,
            child_completed: false,
        }
    }

    fn observe(&mut self, message: &Value) -> Result<Option<String>, String> {
        match message.get("method").and_then(Value::as_str) {
            Some("item/completed") => self.observe_item_completed(message),
            Some("turn/completed") => {
                self.observe_child_turn_completed(message)?;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn observe_item_completed(&mut self, message: &Value) -> Result<Option<String>, String> {
        let params = message
            .get("params")
            .ok_or_else(|| "item/completed omitted params".to_string())?;
        if params.get("threadId").and_then(Value::as_str) != Some(self.parent_thread_id.as_str())
            || params.get("turnId").and_then(Value::as_str) != Some(self.parent_turn_id.as_str())
        {
            return Ok(None);
        }
        let item = params
            .get("item")
            .ok_or_else(|| "item/completed omitted item".to_string())?;
        if item.get("type").and_then(Value::as_str) != Some("subAgentActivity") {
            return Ok(None);
        }
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| "subAgentActivity omitted kind".to_string())?;
        let child_id = item
            .get("agentThreadId")
            .and_then(Value::as_str)
            .ok_or_else(|| "subAgentActivity omitted agentThreadId".to_string())?;
        match kind {
            "started" => {
                if self.child_id.is_some() {
                    return Err("parent started more than one delegated child".into());
                }
                self.child_id = Some(child_id.to_owned());
                Ok(Some(child_id.to_owned()))
            }
            "interacted" | "completed" => Ok(None),
            "interrupted" => Err(format!("delegated child {child_id} was interrupted")),
            other => Err(format!("subAgentActivity reported unknown kind {other}")),
        }
    }

    fn observe_child_turn_completed(&mut self, message: &Value) -> Result<(), String> {
        let params = message
            .get("params")
            .ok_or_else(|| "turn/completed omitted params".to_string())?;
        let Some(child_id) = self.child_id.as_deref() else {
            return Ok(());
        };
        if params.get("threadId").and_then(Value::as_str) != Some(child_id) {
            return Ok(());
        }
        match params.pointer("/turn/status").and_then(Value::as_str) {
            Some("completed") => {
                self.child_completed = true;
                Ok(())
            }
            Some(status) => Err(format!("delegated child {child_id} ended as {status}")),
            None => Err("child turn/completed omitted turn.status".into()),
        }
    }

    fn is_parent_completion(&self, params: &Value) -> bool {
        params.get("threadId").and_then(Value::as_str) == Some(self.parent_thread_id.as_str())
            && params.pointer("/turn/id").and_then(Value::as_str)
                == Some(self.parent_turn_id.as_str())
    }

    fn finish(self, params: &Value) -> Result<DelegationOutcome, String> {
        let answer = final_answer(params)?;
        let child_id = self.child_id.ok_or_else(|| {
            "parent completed without a native subAgentActivity start receipt".to_string()
        })?;
        if !self.child_completed {
            return Err(format!(
                "native child {child_id} lacked a terminal completed turn"
            ));
        }
        Ok(DelegationOutcome { child_id, answer })
    }
}

fn final_answer(params: &Value) -> Result<String, String> {
    let turn = params
        .get("turn")
        .ok_or_else(|| "turn/completed omitted turn".to_string())?;
    let status = turn
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "turn/completed omitted turn.status".to_string())?;
    if status != "completed" {
        return Err(format!(
            "Codex turn ended as {status}: {}",
            turn.get("error").unwrap_or(&Value::Null)
        ));
    }
    let items = turn
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "turn/completed omitted turn.items".to_string())?;
    let messages = items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("agentMessage"))
        .collect::<Vec<_>>();
    let selected = messages
        .iter()
        .rev()
        .find(|item| item.get("phase").and_then(Value::as_str) == Some("final_answer"))
        .copied()
        .or_else(|| messages.last().copied())
        .ok_or_else(|| "completed Codex turn contained no agent message".to_string())?;
    selected
        .get("text")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "final Codex agent message omitted text".to_string())
}

fn turn_outcome(params: &Value) -> Result<TurnOutcome, String> {
    let answer = final_answer(params)?;
    let items = params
        .pointer("/turn/items")
        .and_then(Value::as_array)
        .ok_or_else(|| "turn/completed omitted turn.items".to_string())?;
    let commands = items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("commandExecution"))
        .filter_map(|item| {
            let command = item.get("command")?.as_str()?.to_owned();
            let status = item.get("status")?.as_str()?;
            match status {
                "completed" => Some(CommandOutcome {
                    command,
                    succeeded: true,
                }),
                "failed" | "declined" => Some(CommandOutcome {
                    command,
                    succeeded: false,
                }),
                _ => None,
            }
        })
        .collect();
    Ok(TurnOutcome { answer, commands })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recording() -> Value {
        serde_json::from_str(include_str!("fixtures/codex-0.160.0-2026-10-11.json")).unwrap()
    }

    #[test]
    fn turn_outcome_preserves_generated_commands_and_prefers_terminal_answers() {
        let fixture = recording();
        let turn = fixture["snapshot"]["thread"]["turns"].as_array().unwrap().iter()
            .find(|turn| turn["items"].as_array().unwrap().iter().any(|item| item["type"] == "commandExecution")).unwrap();
        let actual = turn_outcome(&json!({"turn":turn})).unwrap();
        let final_item = turn["items"].as_array().unwrap().iter().rev().find(|item| item["type"] == "agentMessage").unwrap();
        assert_eq!(actual.answer, final_item["text"].as_str().unwrap());
        let command = turn["items"].as_array().unwrap().iter().find(|item| item["type"] == "commandExecution").unwrap();
        assert_eq!(actual.commands, [CommandOutcome { command: command["command"].as_str().unwrap().into(), succeeded: command["status"] == "completed" }]);
        for count in 0..32usize {
            for mask in 0..16usize {
                let mut items = Vec::new();
                let mut commands = Vec::new();
                for index in 0..count {
                    let command = format!("command-{count}-{index}");
                    let succeeded = mask & (1 << (index % 4)) != 0;
                    items.push(json!({"type":"commandExecution","command":command,"status":if succeeded { "completed" } else { "failed" }}));
                    commands.push(CommandOutcome { command, succeeded });
                }
                let answer = format!("answer-{count}-{mask}");
                items.push(json!({"type":"agentMessage","phase":"commentary","text":"before"}));
                items.push(json!({"type":"agentMessage","phase":"final_answer","text":answer}));
                items.push(json!({"type":"agentMessage","phase":"commentary","text":"after"}));
                assert_eq!(
                    turn_outcome(&json!({"turn":{"status":"completed","items":items}})).unwrap(),
                    TurnOutcome { answer, commands }
                );
            }
        }
    }

    #[test]
    fn conversation_decode_preserves_generated_identity_title_fallback_and_selection() {
        let fixture = recording();
        let input = &fixture["conversations"];
        let current = input["data"][0]["id"].as_str().unwrap();
        let recorded = decode_conversations(input, Some(current)).unwrap();
        assert_eq!(recorded[0].id, current);
        assert!(recorded[0].current);
        assert_eq!(recorded[0].preview, input["data"][0]["preview"].as_str().unwrap());
        for count in 0..24usize {
            for selected in 0..=count {
                let mut inputs = Vec::new();
                let mut expected = Vec::new();
                for index in 0..count {
                    let id = format!("thread-{index}");
                    let name = if index % 3 == 0 {
                        format!(" title-{index} ")
                    } else {
                        " ".into()
                    };
                    let preview = if index % 3 != 2 {
                        format!(" preview-{index} ")
                    } else {
                        " ".into()
                    };
                    let title = match index % 3 {
                        0 => Some(format!("title-{index}")),
                        1 => Some(format!("preview-{index}")),
                        _ => None,
                    };
                    inputs.push(json!({"id":id,"name":name,"preview":preview}));
                    expected.push((id, title, preview.trim().to_owned(), index == selected));
                }
                let current = format!("thread-{selected}");
                let actual = decode_conversations(&json!({"data":inputs}), Some(&current)).unwrap();
                assert_eq!(actual.len(), expected.len());
                for (conversation, (id, title, preview, current)) in actual.iter().zip(&expected) {
                    assert_eq!((&conversation.id, &conversation.preview, conversation.current), (id, preview, *current));
                    if let Some(title) = title { assert_eq!(&conversation.title, title); }
                    else { assert!(!conversation.title.trim().is_empty()); }
                }
            }
        }
    }

    #[test]
    fn codex_0160_recording_preserves_snapshot_order_identity_and_payloads() {
        let fixture = recording();
        let input = &fixture["snapshot"];
        let snapshot = decode_conversation_snapshot(input).unwrap();
        assert_eq!(snapshot.id, input["thread"]["id"].as_str().unwrap());
        assert_eq!(snapshot.model, input["model"].as_str().unwrap());
        assert_eq!(snapshot.reasoning_effort, input["reasoningEffort"].as_str().unwrap());
        let actual = snapshot.entries.iter().map(|entry| json!({
            "conversation":entry.conversation,"turn":entry.turn,"key":entry.key,
            "kind":entry.kind,"text":entry.text,"status":entry.status,"append":entry.append,
        })).collect::<Vec<_>>();
        assert_eq!(json!(actual), fixture["expectedEntries"]);
        let turns = input["thread"]["turns"].as_array().unwrap();
        assert_eq!(snapshot.turns.iter().map(|turn| (&*turn.id, &*turn.status)).collect::<Vec<_>>(),
            turns.iter().map(|turn| (turn["id"].as_str().unwrap(), turn["status"].as_str().unwrap())).collect::<Vec<_>>());
        assert_eq!(snapshot.accepted_inputs, turns.iter().flat_map(|turn| turn["items"].as_array().unwrap()).filter_map(|item| item["clientId"].as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn model_selection_and_requests_preserve_generated_default_authority() {
        let fixture = recording();
        let recorded = decode_model_catalog(&fixture["models"]).unwrap();
        let selection = default_model_selection(&recorded).unwrap();
        let default = fixture["models"]["data"].as_array().unwrap().iter().find(|model| model["isDefault"] == true).unwrap();
        assert_eq!(selection.model, default["model"].as_str().unwrap());
        assert_eq!(selection.reasoning_effort, default["defaultReasoningEffort"].as_str().unwrap());
        for count in 1..=16usize {
            for selected in 0..count {
                let inputs = (0..count).map(|index| json!({"model":format!("model-{index}"),"description":format!("description-{index}"),"defaultReasoningEffort":format!("effort-{index}"),"supportedReasoningEfforts":[{"reasoningEffort":format!("first-{index}"),"description":"first reason"},{"reasoningEffort":format!("effort-{index}"),"description":"reason"}],"isDefault":index == selected})).collect::<Vec<_>>();
                let models = decode_model_catalog(&json!({"data":inputs})).unwrap();
                let model = format!("model-{selected}");
                let effort = format!("effort-{selected}");
                assert_eq!(
                    models[selected].reasoning[0].effort,
                    format!("first-{selected}")
                );
                assert_eq!(models[selected].reasoning[0].description, "first reason");
                assert_eq!(models[selected].reasoning[1].effort, effort);
                assert_eq!(models[selected].reasoning[1].description, "reason");
                let selection = default_model_selection(&models).unwrap();
                assert_eq!(
                    selection,
                    ModelSelection {
                        model: model.clone(),
                        reasoning_effort: effort.clone()
                    }
                );
                let cwd = PathBuf::from(format!("/tmp/project-{count}"));
                assert_eq!(
                    thread_start_params(&cwd, &selection),
                    json!({"cwd":cwd,"model":model,"config":{"model_reasoning_effort":effort},"sandbox":"workspace-write"})
                );
                let thread = format!("thread-{count}-{selected}");
                assert_eq!(
                    thread_settings_update_params(&thread, &model, &effort),
                    json!({"threadId":thread,"model":model,"effort":effort})
                );
            }
        }
    }

    #[test]
    fn delegation_receipts_match_the_single_child_state_table() {
        let parent = "parent".to_owned();
        let turn = "turn".to_owned();
        let child = "child".to_owned();
        let other = "other".to_owned();
        let answer = "answer".to_owned();
        let activity = |id: &str, kind: &str| json!({"method":"item/completed","params":{"threadId":parent,"turnId":turn,"item":{"type":"subAgentActivity","kind":kind,"agentThreadId":id}}});
        let completion = |id: &str, status: &str| json!({"method":"turn/completed","params":{"threadId":id,"turn":{"id":"child-turn","status":status,"items":[]}}});
        let finish = json!({"threadId":parent,"turn":{"id":turn,"status":"completed","items":[{"type":"agentMessage","phase":"final_answer","text":answer}]}});
        for receipt in [false, true] {
            for terminal in ["absent", "unrelated", "completed", "failed"] {
                let mut tracker = DelegationTracker::new(parent.clone(), turn.clone());
                if receipt {
                    assert_eq!(
                        tracker.observe(&activity(&child, "started")).unwrap(),
                        Some(child.clone())
                    );
                }
                match terminal {
                    "unrelated" => {
                        tracker.observe(&completion(&other, "completed")).unwrap();
                    }
                    "completed" => {
                        tracker.observe(&completion(&child, "completed")).unwrap();
                    }
                    "failed" => {
                        assert_eq!(
                            tracker.observe(&completion(&child, "failed")).is_err(),
                            receipt
                        );
                    }
                    _ => {}
                }
                tracker.observe(&activity(&child, "completed")).unwrap();
                let result = tracker.finish(&finish);
                if receipt && terminal == "completed" {
                    assert_eq!(
                        result.unwrap(),
                        DelegationOutcome {
                            child_id: child.clone(),
                            answer: answer.clone()
                        }
                    );
                } else {
                    assert!(result.is_err());
                }
            }
        }
        let mut tracker = DelegationTracker::new(parent.clone(), turn.clone());
        tracker.observe(&activity(&child, "started")).unwrap();
        assert!(tracker.observe(&activity(&other, "started")).is_err());
    }

}
