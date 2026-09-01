use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;
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

pub struct Codex {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: mpsc::Receiver<String>,
    stderr_task: JoinHandle<()>,
    next_id: u64,
    thread_id: String,
    model: String,
    reasoning_effort: String,
}

impl Codex {
    pub async fn start(cwd: &Path) -> NorthResult<Self> {
        let mut command = Command::new("codex");
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
            thread_id: String::new(),
            model: String::new(),
            reasoning_effort: String::new(),
        };
        codex.initialize().await?;
        let selection = codex.default_model_selection().await?;
        codex.thread_id = codex.start_thread(cwd, &selection).await?;
        codex.model = selection.model;
        codex.reasoning_effort = selection.reasoning_effort;
        Ok(codex)
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn reasoning_effort(&self) -> &str {
        &self.reasoning_effort
    }

    pub async fn run_turn(&mut self, prompt: &str) -> NorthResult<String> {
        let thread_id = self.thread_id.clone();
        let turn_id = self
            .start_turn(json!({
                "threadId": thread_id,
                "input": [{"type": "text", "text": prompt}]
            }))
            .await?;

        loop {
            let message = self.read_message().await?;
            if message.get("method").and_then(Value::as_str) != Some("turn/completed") {
                continue;
            }
            let params = message
                .get("params")
                .ok_or_else(|| self.protocol_error("turn/completed omitted params", &message))?;
            if params.get("threadId").and_then(Value::as_str) != Some(self.thread_id.as_str()) {
                continue;
            }
            if params.pointer("/turn/id").and_then(Value::as_str) != Some(turn_id.as_str()) {
                continue;
            }
            return final_answer(params).map_err(|message| self.protocol_error(&message, params));
        }
    }

    pub async fn run_delegate<F>(
        &mut self,
        prompt: &str,
        mut child_spawned: F,
    ) -> NorthResult<DelegationOutcome>
    where
        F: FnMut(&str) -> NorthResult<()>,
    {
        let model = self.model.clone();
        let thread_id = self.thread_id.clone();
        let turn_id = self
            .start_turn(json!({
                "threadId": thread_id,
                "input": [{"type": "text", "text": prompt}],
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
        let mut tracker = DelegationTracker::new(self.thread_id.clone(), turn_id);

        loop {
            let message = self.read_message().await?;
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

    async fn default_model_selection(&mut self) -> NorthResult<ModelSelection> {
        let result = self
            .request("model/list", json!({"limit": 100, "includeHidden": false}))
            .await?;
        decode_default_model_selection(&result)
            .map_err(|message| self.protocol_error(&message, &result))
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

fn decode_default_model_selection(result: &Value) -> Result<ModelSelection, String> {
    let models = result
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "model/list omitted data".to_string())?;
    let defaults = models
        .iter()
        .filter(|model| model.get("isDefault").and_then(Value::as_bool) == Some(true))
        .collect::<Vec<_>>();
    let [selected] = defaults.as_slice() else {
        return Err("model/list did not identify exactly one default model".into());
    };
    let model = selected
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| "default model omitted model".to_string())?
        .to_owned();
    let reasoning_effort = selected
        .get("defaultReasoningEffort")
        .and_then(Value::as_str)
        .ok_or_else(|| "default model omitted defaultReasoningEffort".to_string())?
        .to_owned();
    Ok(ModelSelection {
        model,
        reasoning_effort,
    })
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
    fn default_model_and_effort_become_explicit_thread_authority() {
        let selection = decode_default_model_selection(&json!({
            "data": [
                {
                    "model": "gpt-example",
                    "defaultReasoningEffort": "high",
                    "isDefault": true
                }
            ]
        }))
        .unwrap();
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
    async fn installed_app_server_handshake_and_shutdown() {
        let cwd = std::env::current_dir().expect("current directory is available");
        let codex = Codex::start(&cwd)
            .await
            .expect("installed Codex app-server initializes and starts a thread");
        codex
            .shutdown()
            .await
            .expect("installed Codex app-server exits when stdin closes");
    }
}
