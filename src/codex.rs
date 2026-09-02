use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::error::{NorthError, NorthResult};

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
pub enum ConversationEntry {
    Operator(String),
    Agent(String),
    Command(CommandOutcome),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationSnapshot {
    pub id: String,
    pub model: String,
    pub reasoning_effort: String,
    pub entries: Vec<ConversationEntry>,
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
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: mpsc::Receiver<String>,
    stderr_task: JoinHandle<()>,
    next_id: u64,
    thread_id: Option<String>,
    model: String,
    reasoning_effort: String,
    models: Vec<ModelOption>,
}

impl Codex {
    pub async fn start(cwd: &Path) -> NorthResult<Self> {
        let mut codex = Self::connect(cwd).await?;
        codex.start_new_conversation(cwd).await?;
        Ok(codex)
    }

    pub async fn connect(cwd: &Path) -> NorthResult<Self> {
        Self::connect_with_command(cwd, Command::new("codex")).await
    }

    #[cfg(test)]
    async fn connect_with_home(cwd: &Path, codex_home: &Path) -> NorthResult<Self> {
        let mut command = Command::new("codex");
        command
            .env("CODEX_HOME", codex_home)
            .env("CODEX_SQLITE_HOME", codex_home);
        Self::connect_with_command(cwd, command).await
    }

    async fn connect_with_command(cwd: &Path, mut command: Command) -> NorthResult<Self> {
        command
            .args([
                "app-server",
                "--listen",
                "stdio://",
                "--enable",
                "multi_agent_v2",
            ])
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
        let mut codex = Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout).lines(),
            stderr,
            stderr_task,
            next_id: 1,
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

    pub async fn start_new_conversation(&mut self, cwd: &Path) -> NorthResult<String> {
        let selection = ModelSelection {
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
        };
        let thread_id = self.start_thread(cwd, &selection).await?;
        self.thread_id = Some(thread_id.clone());
        Ok(thread_id)
    }

    pub async fn conversations(&mut self, cwd: &Path) -> NorthResult<Vec<ConversationOption>> {
        let result = self
            .request(
                "thread/list",
                json!({
                    "limit": 100,
                    "sortKey": "recency_at",
                    "sortDirection": "desc",
                    "cwd": cwd,
                    "sourceKinds": ["appServer", "cli", "vscode", "unknown"],
                    "useStateDbOnly": true
                }),
            )
            .await?;
        decode_conversations(&result, self.thread_id.as_deref())
            .map_err(|message| self.protocol_error(&message, &result))
    }

    pub async fn resume_conversation(
        &mut self,
        conversation_id: &str,
    ) -> NorthResult<ConversationSnapshot> {
        let result = self
            .request("thread/resume", json!({"threadId": conversation_id}))
            .await?;
        let snapshot = decode_conversation_snapshot(&result)
            .map_err(|message| self.protocol_error(&message, &result))?;
        self.thread_id = Some(snapshot.id.clone());
        self.model = snapshot.model.clone();
        self.reasoning_effort = snapshot.reasoning_effort.clone();
        Ok(snapshot)
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

    #[cfg(test)]
    pub async fn run_turn(&mut self, prompt: &str) -> NorthResult<TurnOutcome> {
        let (_interrupt_tx, interrupt_rx) = oneshot::channel();
        self.run_turn_interruptible(prompt, &[], interrupt_rx).await
    }

    pub async fn run_turn_interruptible(
        &mut self,
        prompt: &str,
        local_images: &[PathBuf],
        mut interrupt: oneshot::Receiver<()>,
    ) -> NorthResult<TurnOutcome> {
        let thread_id = self.require_thread_id()?.to_owned();
        let turn_id = self
            .start_turn(json!({
                "threadId": thread_id.clone(),
                "input": turn_input(prompt, local_images)
            }))
            .await?;
        let mut interrupt_sent = false;

        loop {
            let message = tokio::select! {
                signal = &mut interrupt, if !interrupt_sent => {
                    interrupt_sent = true;
                    if signal.is_ok() {
                        self.send_interrupt(&turn_id).await?;
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
            if params.pointer("/turn/id").and_then(Value::as_str) != Some(turn_id.as_str()) {
                continue;
            }
            if params.pointer("/turn/status").and_then(Value::as_str) == Some("interrupted") {
                return Err(NorthError::Interrupted);
            }
            return turn_outcome(params).map_err(|message| self.protocol_error(&message, params));
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

    pub async fn shutdown(mut self) -> NorthResult<()> {
        self.stdin.take();
        let status = match timeout(SHUTDOWN_GRACE, self.child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                self.child.kill().await?;
                self.child.wait().await?
            }
        };
        self.stderr_task.abort();
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
                    "title": "North TUI",
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

    async fn start_turn(&mut self, params: Value) -> NorthResult<String> {
        let result = self.request("turn/start", params).await?;
        result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| self.protocol_error("turn/start omitted turn.id", &result))
    }

    async fn send_interrupt(&mut self, turn_id: &str) -> NorthResult<()> {
        let id = self.allocate_id();
        let thread_id = self.require_thread_id()?.to_owned();
        self.send(&json!({
            "method": "turn/interrupt",
            "id": id,
            "params": {"threadId": thread_id, "turnId": turn_id}
        }))
        .await
    }

    async fn model_catalog(&mut self) -> NorthResult<Vec<ModelOption>> {
        let result = self
            .request("model/list", json!({"limit": 100, "includeHidden": false}))
            .await?;
        decode_model_catalog(&result).map_err(|message| self.protocol_error(&message, &result))
    }

    async fn request(&mut self, method: &str, params: Value) -> NorthResult<Value> {
        let id = self.allocate_id();
        self.send(&json!({"method": method, "id": id, "params": params}))
            .await?;
        loop {
            let message = self.read_message().await?;
            if message.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                return Err(self.protocol_error(&format!("{method} was rejected"), error));
            }
            return message
                .get("result")
                .cloned()
                .ok_or_else(|| self.protocol_error(&format!("{method} omitted result"), &message));
        }
    }

    async fn send(&mut self, message: &Value) -> NorthResult<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| NorthError::Protocol("Codex app-server stdin is closed".into()))?;
        let mut encoded = serde_json::to_vec(message)?;
        encoded.push(b'\n');
        stdin.write_all(&encoded).await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn read_message(&mut self) -> NorthResult<Value> {
        loop {
            if let Some(line) = self.stdout.next_line().await? {
                if !line.trim().is_empty() {
                    return Ok(serde_json::from_str(&line)?);
                }
                continue;
            }
            let status = self.child.wait().await?;
            return Err(NorthError::AppServerExit(status));
        }
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn require_thread_id(&self) -> NorthResult<&str> {
        self.thread_id
            .as_deref()
            .ok_or_else(|| NorthError::Protocol("Codex has no active conversation".into()))
    }

    fn protocol_error(&mut self, message: &str, value: &Value) -> NorthError {
        let mut stderr = VecDeque::new();
        while let Ok(line) = self.stderr.try_recv() {
            if stderr.len() == 8 {
                stderr.pop_front();
            }
            stderr.push_back(line);
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
    for turn in turns {
        decode_turn_history(turn, &mut entries)?;
    }
    Ok(ConversationSnapshot {
        id: id.to_owned(),
        model: model.to_owned(),
        reasoning_effort: reasoning_effort.to_owned(),
        entries,
    })
}

fn decode_turn_history(turn: &Value, entries: &mut Vec<ConversationEntry>) -> Result<(), String> {
    let items = turn
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| "thread turn omitted items".to_string())?;
    let mut agent_messages = Vec::new();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("userMessage") => {
                if let Some(message) = decode_user_message(item)? {
                    entries.push(ConversationEntry::Operator(message));
                }
            }
            Some("commandExecution") => {
                let Some(command) = item.get("command").and_then(Value::as_str) else {
                    continue;
                };
                let succeeded = match item.get("status").and_then(Value::as_str) {
                    Some("completed") => true,
                    Some("failed" | "declined") => false,
                    _ => continue,
                };
                entries.push(ConversationEntry::Command(CommandOutcome {
                    command: command.to_owned(),
                    succeeded,
                }));
            }
            Some("agentMessage") => {
                let Some(text) = item.get("text").and_then(Value::as_str) else {
                    continue;
                };
                agent_messages.push((item.get("phase").and_then(Value::as_str), text.to_owned()));
            }
            _ => {}
        }
    }
    if let Some((_, message)) = agent_messages
        .iter()
        .rev()
        .find(|(phase, _)| *phase == Some("final_answer"))
        .or_else(|| agent_messages.last())
    {
        entries.push(ConversationEntry::Agent(message.clone()));
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
        "approvalPolicy": "never",
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

    const PARENT: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a1";
    const CHILD: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a2";
    const OTHER: &str = "01993fe1-a327-7fc0-a476-3e4bb23ac4a3";
    const TURN: &str = "turn-j2";

    fn delegation_tracker() -> DelegationTracker {
        DelegationTracker::new(PARENT.into(), TURN.into())
    }

    fn child_activity(child_id: &str, kind: &str) -> Value {
        json!({
            "method": "item/completed",
            "params": {
                "threadId": PARENT,
                "turnId": TURN,
                "item": {
                    "type": "subAgentActivity",
                    "kind": kind,
                    "agentThreadId": child_id,
                    "agentPath": "/root/create_j2_proof"
                }
            }
        })
    }

    fn child_completion(child_id: &str, status: &str) -> Value {
        json!({
            "method": "turn/completed",
            "params": {
                "threadId": child_id,
                "turn": {"id": "child-turn", "status": status, "items": []}
            }
        })
    }

    fn parent_completion() -> Value {
        json!({
            "threadId": PARENT,
            "turn": {
                "id": TURN,
                "status": "completed",
                "items": [
                    {"type": "agentMessage", "phase": "final_answer", "text": "DELEGATED_DONE"}
                ]
            }
        })
    }

    #[test]
    fn final_answer_prefers_the_terminal_message() {
        let params = json!({
            "turn": {
                "status": "completed",
                "items": [
                    {"type": "agentMessage", "phase": "commentary", "text": "working"},
                    {"type": "agentMessage", "phase": "final_answer", "text": "DONE"}
                ]
            }
        });
        assert_eq!(final_answer(&params).unwrap(), "DONE");
    }

    #[test]
    fn completed_turn_collects_command_results_for_the_tui() {
        let params = json!({
            "turn": {
                "status": "completed",
                "items": [
                    {
                        "type": "commandExecution",
                        "command": "cargo test",
                        "status": "completed"
                    },
                    {
                        "type": "commandExecution",
                        "command": "cargo build",
                        "status": "failed"
                    },
                    {"type": "agentMessage", "phase": "final_answer", "text": "DONE"}
                ]
            }
        });

        assert_eq!(
            turn_outcome(&params).unwrap(),
            TurnOutcome {
                answer: "DONE".into(),
                commands: vec![
                    CommandOutcome {
                        command: "cargo test".into(),
                        succeeded: true,
                    },
                    CommandOutcome {
                        command: "cargo build".into(),
                        succeeded: false,
                    }
                ],
            }
        );
    }

    #[test]
    fn thread_list_decodes_titles_and_marks_the_active_conversation() {
        let conversations = decode_conversations(
            &json!({
                "data": [
                    {"id": "thread-a", "name": "North roadmap", "preview": "first prompt"},
                    {"id": "thread-b", "preview": "Fix the composer"}
                ]
            }),
            Some("thread-b"),
        )
        .unwrap();

        assert_eq!(conversations[0].title, "North roadmap");
        assert_eq!(conversations[0].preview, "first prompt");
        assert!(!conversations[0].current);
        assert_eq!(conversations[1].title, "Fix the composer");
        assert!(conversations[1].current);
    }

    #[test]
    fn resumed_thread_replays_user_images_commands_and_final_answers() {
        let snapshot = decode_conversation_snapshot(&json!({
            "model": "gpt-5.6-sol",
            "reasoningEffort": "high",
            "thread": {
                "id": "thread-a",
                "turns": [{
                    "items": [
                        {
                            "type": "userMessage",
                            "content": [
                                {"type": "text", "text": "inspect this"},
                                {"type": "localImage", "path": "/tmp/image.png"}
                            ]
                        },
                        {"type": "agentMessage", "phase": "commentary", "text": "working"},
                        {"type": "commandExecution", "command": "cargo test", "status": "completed"},
                        {"type": "agentMessage", "phase": "final_answer", "text": "Done."}
                    ]
                }]
            }
        }))
        .unwrap();

        assert_eq!(snapshot.id, "thread-a");
        assert_eq!(snapshot.model, "gpt-5.6-sol");
        assert_eq!(snapshot.reasoning_effort, "high");
        assert_eq!(
            snapshot.entries,
            vec![
                ConversationEntry::Operator("inspect this [Image #1]".into()),
                ConversationEntry::Command(CommandOutcome {
                    command: "cargo test".into(),
                    succeeded: true,
                }),
                ConversationEntry::Agent("Done.".into()),
            ]
        );
    }

    #[test]
    fn turn_input_sends_clipboard_images_as_native_local_images() {
        assert_eq!(
            turn_input(
                "compare these",
                &[
                    PathBuf::from("/tmp/north-clipboard-one.png"),
                    PathBuf::from("/tmp/north-clipboard-two.png"),
                ],
            ),
            vec![
                json!({"type": "text", "text": "compare these"}),
                json!({
                    "type": "localImage",
                    "path": "/tmp/north-clipboard-one.png"
                }),
                json!({
                    "type": "localImage",
                    "path": "/tmp/north-clipboard-two.png"
                }),
            ]
        );
    }

    #[test]
    fn default_model_and_effort_become_explicit_thread_authority() {
        let models = decode_model_catalog(&json!({
            "data": [
                {
                    "model": "gpt-example",
                    "description": "Example model",
                    "defaultReasoningEffort": "high",
                    "supportedReasoningEfforts": [
                        {"reasoningEffort": "low", "description": "Fast"},
                        {"reasoningEffort": "high", "description": "Deep"}
                    ],
                    "isDefault": true
                }
            ]
        }))
        .unwrap();
        let selection = default_model_selection(&models).unwrap();
        assert_eq!(models[0].reasoning[1].effort, "high");
        assert_eq!(
            selection,
            ModelSelection {
                model: "gpt-example".into(),
                reasoning_effort: "high".into(),
            }
        );
        assert_eq!(
            thread_start_params(Path::new("/tmp/project"), &selection),
            json!({
                "cwd": "/tmp/project",
                "model": "gpt-example",
                "config": {"model_reasoning_effort": "high"},
                "approvalPolicy": "never",
                "sandbox": "workspace-write"
            })
        );
        assert_eq!(
            thread_settings_update_params("thread-1", "gpt-example", "high"),
            json!({
                "threadId": "thread-1",
                "model": "gpt-example",
                "effort": "high"
            })
        );
    }

    #[test]
    fn delegation_requires_one_receipt_linked_completed_child() {
        let mut tracker = delegation_tracker();
        assert_eq!(
            tracker.observe(&child_activity(CHILD, "started")).unwrap(),
            Some(CHILD.into())
        );
        tracker
            .observe(&child_completion(CHILD, "completed"))
            .unwrap();
        tracker
            .observe(&child_activity(CHILD, "completed"))
            .unwrap();

        assert_eq!(
            tracker.finish(&parent_completion()).unwrap(),
            DelegationOutcome {
                child_id: CHILD.into(),
                answer: "DELEGATED_DONE".into(),
            }
        );
    }

    #[test]
    fn unrelated_child_completion_does_not_settle_the_receipt_child() {
        let mut tracker = delegation_tracker();
        tracker.observe(&child_activity(CHILD, "started")).unwrap();
        tracker
            .observe(&child_completion(OTHER, "completed"))
            .unwrap();

        assert!(
            tracker
                .finish(&parent_completion())
                .unwrap_err()
                .contains("lacked a terminal completed turn")
        );
    }

    #[test]
    fn delegation_rejects_parent_completion_without_spawn_receipt() {
        let tracker = delegation_tracker();

        assert!(
            tracker
                .finish(&parent_completion())
                .unwrap_err()
                .contains("without a native subAgentActivity start receipt")
        );
    }

    #[test]
    fn delegation_rejects_a_second_started_child() {
        let mut tracker = delegation_tracker();
        tracker.observe(&child_activity(CHILD, "started")).unwrap();

        assert_eq!(
            tracker
                .observe(&child_activity(OTHER, "started"))
                .unwrap_err(),
            "parent started more than one delegated child"
        );
    }

    #[test]
    fn delegation_rejects_terminal_child_failure() {
        let mut tracker = delegation_tracker();
        tracker.observe(&child_activity(CHILD, "started")).unwrap();

        assert_eq!(
            tracker
                .observe(&child_completion(CHILD, "errored"))
                .unwrap_err(),
            format!("delegated child {CHILD} ended as errored")
        );
    }

    #[tokio::test]
    #[ignore = "requires the installed Codex app-server"]
    async fn installed_app_server_accepts_model_and_effort_updates() {
        let cwd = PathBuf::from(
            std::env::var_os("HOME").expect("the installed-user home directory is available"),
        );
        let mut codex = Codex::start(&cwd)
            .await
            .expect("installed Codex app-server initializes and starts a thread");
        let model = codex.model().to_owned();
        let effort = codex.reasoning_effort().to_owned();

        codex
            .set_model_and_effort(&model, &effort)
            .await
            .expect("installed app-server accepts thread settings updates");
        codex
            .shutdown()
            .await
            .expect("installed Codex app-server exits when stdin closes");
    }

    #[tokio::test]
    #[ignore = "requires the installed Codex app-server"]
    async fn installed_app_server_persists_and_resumes_a_north_conversation() {
        let codex_home = tempfile::tempdir().expect("an isolated Codex home is available");
        let cwd = tempfile::tempdir().expect("an isolated working directory is available");
        let mut codex = timeout(
            Duration::from_secs(10),
            Codex::connect_with_home(cwd.path(), codex_home.path()),
        )
        .await
        .expect("Codex startup completes within ten seconds")
        .expect("installed Codex app-server initializes");
        let thread_id = timeout(
            Duration::from_secs(10),
            codex.start_new_conversation(cwd.path()),
        )
        .await
        .expect("thread/start completes within ten seconds")
        .expect("a conversation starts without a model request");
        timeout(
            Duration::from_secs(10),
            codex.request(
                "thread/inject_items",
                json!({
                    "threadId": thread_id,
                    "items": [
                        {
                            "type": "message",
                            "role": "user",
                            "content": [{"type": "input_text", "text": "North resume canary"}]
                        },
                        {
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": "Persisted without authentication"}]
                        }
                    ]
                }),
            ),
        )
        .await
        .expect("thread/inject_items completes within ten seconds")
        .expect("history injection persists without a model request");
        timeout(Duration::from_secs(10), codex.shutdown())
            .await
            .expect("first Codex shutdown completes within ten seconds")
            .expect("first installed Codex app-server exits when stdin closes");

        let mut codex = timeout(
            Duration::from_secs(10),
            Codex::connect_with_home(cwd.path(), codex_home.path()),
        )
        .await
        .expect("Codex restart completes within ten seconds")
        .expect("installed Codex app-server restarts against persisted state");

        let snapshot = timeout(
            Duration::from_secs(10),
            codex.resume_conversation(&thread_id),
        )
        .await
        .expect("thread/resume completes within ten seconds")
        .expect("the persisted North conversation resumes by identity");
        assert_eq!(snapshot.id, thread_id);
        assert_eq!(codex.thread_id.as_deref(), Some(snapshot.id.as_str()));
        timeout(Duration::from_secs(10), codex.shutdown())
            .await
            .expect("Codex shutdown completes within ten seconds")
            .expect("installed Codex app-server exits when stdin closes");
    }

    #[tokio::test]
    #[ignore = "requires the installed Codex app-server"]
    async fn installed_app_server_reuses_one_thread_for_repeated_turns() {
        let cwd = std::env::current_dir().expect("current directory is available");
        let mut codex = Codex::start(&cwd)
            .await
            .expect("installed Codex app-server initializes and starts a thread");
        assert_eq!(
            codex
                .run_turn("Remember the codeword ORCHID. Reply with exactly ACK.")
                .await
                .expect("first turn completes")
                .answer,
            "ACK"
        );
        assert_eq!(
            codex
                .run_turn("Reply with only the codeword I gave you.")
                .await
                .expect("second turn completes on the same thread")
                .answer,
            "ORCHID"
        );
        codex
            .shutdown()
            .await
            .expect("installed Codex app-server exits when stdin closes");
    }
}
