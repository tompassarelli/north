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

pub struct Codex {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Lines<BufReader<ChildStdout>>,
    stderr: mpsc::Receiver<String>,
    stderr_task: JoinHandle<()>,
    next_id: u64,
    thread_id: String,
}

impl Codex {
    pub async fn start(cwd: &Path) -> NorthResult<Self> {
        let mut command = Command::new("codex");
        command
            .args(["app-server", "--listen", "stdio://"])
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
        };
        codex.initialize().await?;
        codex.thread_id = codex.start_thread(cwd).await?;
        Ok(codex)
    }

    pub async fn run_turn(&mut self, prompt: &str) -> NorthResult<String> {
        let id = self.allocate_id();
        let thread_id = self.thread_id.clone();
        self.send(&json!({
            "method": "turn/start",
            "id": id,
            "params": {
                "threadId": thread_id,
                "input": [{"type": "text", "text": prompt}]
            }
        }))
        .await?;

        let mut response_seen = false;
        loop {
            let message = self.read_message().await?;
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(self.protocol_error("turn/start was rejected", error));
                }
                response_seen = true;
                continue;
            }
            if message.get("method").and_then(Value::as_str) != Some("turn/completed") {
                continue;
            }
            let params = message
                .get("params")
                .ok_or_else(|| self.protocol_error("turn/completed omitted params", &message))?;
            if params.get("threadId").and_then(Value::as_str) != Some(self.thread_id.as_str()) {
                continue;
            }
            if !response_seen {
                return Err(self.protocol_error(
                    "turn completed before turn/start was acknowledged",
                    &message,
                ));
            }
            return final_answer(params).map_err(|message| self.protocol_error(&message, params));
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
                }
            }),
        )
        .await?;
        self.send(&json!({"method": "initialized", "params": {}}))
            .await
    }

    async fn start_thread(&mut self, cwd: &Path) -> NorthResult<String> {
        let result = self
            .request(
                "thread/start",
                json!({
                    "cwd": cwd,
                    "approvalPolicy": "never",
                    "sandbox": "workspace-write",
                    "ephemeral": true
                }),
            )
            .await?;
        result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| self.protocol_error("thread/start omitted thread.id", &result))
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
