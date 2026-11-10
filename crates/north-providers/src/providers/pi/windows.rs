//! The interactive windows of the Pi harness. Pi keeps no session socket of its own, so its
//! `north-fleet.ts` extension registers each live window in `<state>/pi-windows` and listens on
//! `<state>/pi-sessions/<pid>.sock`; this module reads that registry, finds the window a caller runs
//! in from `PI_SESSION_ID`, and delivers a report over its socket. `adopt` installs the extension,
//! so a later Pi session loads it without anyone copying a file.
use std::fs;
use std::io::Write;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::host::alive;
use crate::fleet::model::Account;
use crate::fleet::runtime::{LiveWindow, WindowInbox, now_ms};

const SESSION_ENV: &str = "PI_SESSION_ID";
const SESSION_FILE_ENV: &str = "PI_SESSION_FILE";
const AGENT_ENV: &str = "PI_CODING_AGENT_DIR";
const EXTENSION: &str = r#"// The north fleet bridge for Pi: register this window so `north fleet adopt` and dispatch see it
// live, and listen on its socket so a report to the name reaches this session as a user message.
// It also hands each tool call to `north hook`, as Claude Code's hooks do, so a Pi session meets the
// same refusals (north#1041).
// `north fleet adopt` installs this file under <agent>/extensions; Pi loads it on start or /reload.
import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { spawnSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:net";
import { homedir } from "node:os";
import { join } from "node:path";

const xdg = process.env.XDG_STATE_HOME;
const state = xdg && xdg.startsWith("/") ? join(xdg, "north") : join(homedir(), ".local/state/north");
const windows = join(state, "pi-windows");
const sockets = join(state, "pi-sessions");
const hookTools: Record<string, string> = { bash: "Bash", edit: "Edit", write: "Write" };

function hookInput(tool: string, input: Record<string, unknown>): Record<string, unknown> {
  if (tool === "Bash") return { command: input.command };
  const { path, ...rest } = input;
  return { ...rest, file_path: path };
}

function refusal(toolName: string, input: Record<string, unknown>, cwd: string, session: string): string | undefined {
  const tool = hookTools[toolName];
  if (!tool) return undefined;
  const event = { hook_event_name: "PreToolUse", tool_name: tool, tool_input: hookInput(tool, input), cwd, session_id: session };
  const run = spawnSync("north", ["hook"], { input: JSON.stringify(event), encoding: "utf8", timeout: 30_000 });
  if (run.error || run.status !== 0) return `north hook could not judge this ${tool} call (${run.error ?? run.stderr}), so it is refused`;
  const decision = run.stdout.trim() ? JSON.parse(run.stdout)?.hookSpecificOutput : undefined;
  return decision?.permissionDecision === "deny" ? String(decision.permissionDecisionReason) : undefined;
}

export default function (pi: ExtensionAPI) {
  let server: Server | undefined;
  let context: ExtensionContext | undefined;
  let registry = "";

  pi.on("tool_call", async (event, ctx) => {
    const reason = refusal(event.toolName, event.input as Record<string, unknown>, ctx.cwd, ctx.sessionManager.getSessionId());
    return reason ? { block: true, reason } : undefined;
  });

  pi.on("session_shutdown", () => {
    server?.close();
    server = undefined;
    if (registry) rmSync(registry, { force: true });
    registry = "";
  });

  pi.on("session_start", async (_event, ctx) => {
    server?.close();
    if (registry) rmSync(registry, { force: true });
    context = ctx;
    const id = ctx.sessionManager.getSessionId();
    let file: string | undefined;
    try {
      file = ctx.sessionManager.getSessionFile();
    } catch {
      file = undefined;
    }
    mkdirSync(windows, { recursive: true });
    mkdirSync(sockets, { recursive: true });
    const socket = join(sockets, `${process.pid}.sock`);
    rmSync(socket, { force: true });
    server = createServer((connection) => {
      const chunks: Buffer[] = [];
      connection.on("data", (chunk) => chunks.push(chunk));
      connection.on("end", () => deliver(Buffer.concat(chunks).toString("utf8")));
    });
    await new Promise<void>((resolve, reject) => server!.once("error", reject).listen(socket, () => resolve()));
    registry = join(windows, `${process.pid}.json`);
    writeFileSync(registry, JSON.stringify({ pid: process.pid, conversation: id, session_file: file, socket, cwd: ctx.cwd, started_ms: Date.now(), busy: false }));
  });

  function deliver(text: string) {
    const message = text.trim();
    if (!message || !context) return;
    try {
      if (context.isIdle()) pi.sendUserMessage(message);
      else pi.sendUserMessage(message, { deliverAs: "followUp" });
    } catch (error) {
      context.ui.notify(`north fleet: could not deliver a report: ${error}`, "error");
    }
  }
}
"#;
const EXTENSION_FILE: &str = "north-fleet.ts";
const WINDOWS: &str = "pi-windows";
const SOCKETS: &str = "pi-sessions";
const DEADLINE: Duration = Duration::from_secs(5);

pub struct PiWindows {
    state: PathBuf,
    agent: PathBuf,
}

impl PiWindows {
    pub fn live() -> Self {
        Self::at(state_root(), agent_dir())
    }

    pub fn at(state: PathBuf, agent: PathBuf) -> Self {
        Self { state, agent }
    }

    /// The conversation a Pi caller runs in: Pi sets `PI_SESSION_ID` for every shell tool command.
    pub fn conversation_of(read: impl Fn(&str) -> Option<String>) -> Option<String> {
        read(SESSION_ENV).filter(|conversation| !conversation.is_empty())
    }

    fn registry(&self) -> PathBuf {
        self.state.join(WINDOWS)
    }

    fn sockets(&self) -> PathBuf {
        self.state.join(SOCKETS)
    }

    fn records(&self) -> Vec<Value> {
        fs::read_dir(self.registry())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
            })
            .filter_map(|entry| serde_json::from_slice(&fs::read(entry.path()).ok()?).ok())
            .collect()
    }

    fn inbox(&self, record: &Value, account: &str) -> Option<WindowInbox> {
        let pid = u32::try_from(record["pid"].as_u64()?).ok()?;
        let socket = PathBuf::from(record["socket"].as_str()?);
        alive(pid).then_some(WindowInbox {
            account: account.to_owned(),
            pid,
            socket,
            token: String::new(),
            busy: record["busy"].as_bool().unwrap_or(false),
        })
    }

    /// The window this process runs in, while `adopt` runs before the extension has registered it:
    /// the enclosing Pi process and the session file and id Pi gave the shell tool.
    fn caller_window(&self, accounts: &[Account], conversation: &str) -> Option<LiveWindow> {
        if Self::conversation_of(|name| std::env::var(name).ok()).as_deref() != Some(conversation) {
            return None;
        }
        let pid = pi_ancestor()?;
        let account = account(accounts);
        let socket = self.sockets().join(format!("{pid}.sock"));
        let cwd = std::env::current_dir().unwrap_or_default();
        Some(LiveWindow {
            conversation: conversation.to_owned(),
            started_ms: now_ms(),
            cwd,
            inbox: WindowInbox {
                account,
                pid,
                socket,
                token: String::new(),
                busy: false,
            },
        })
    }

    fn install(&self) -> NorthResult<()> {
        let extensions = self.agent.join("extensions");
        fs::create_dir_all(&extensions).map_err(|error| {
            NorthError::State(format!("cannot create {}: {error}", extensions.display()))
        })?;
        let file = extensions.join(EXTENSION_FILE);
        if fs::read_to_string(&file).ok().as_deref() != Some(EXTENSION) {
            fs::write(&file, EXTENSION).map_err(|error| {
                NorthError::State(format!("cannot write {}: {error}", file.display()))
            })?;
        }
        Ok(())
    }

    fn register(&self, conversation: &str, inbox: &WindowInbox) -> NorthResult<()> {
        fs::create_dir_all(self.registry()).map_err(|error| {
            NorthError::State(format!(
                "cannot create {}: {error}",
                self.registry().display()
            ))
        })?;
        let file = self.registry().join(format!("{}.json", inbox.pid));
        let record = json!({
            "pid": inbox.pid,
            "conversation": conversation,
            "session_file": std::env::var_os(SESSION_FILE_ENV).map(|file| PathBuf::from(file).display().to_string()),
            "socket": inbox.socket.display().to_string(),
            "cwd": std::env::current_dir().map(|cwd| cwd.display().to_string()).unwrap_or_default(),
            "started_ms": now_ms(),
        });
        fs::write(&file, record.to_string())
            .map_err(|error| NorthError::State(format!("cannot write {}: {error}", file.display())))
    }
}

impl PiWindows {
    pub(crate) fn caller_conversation(&self) -> Option<String> {
        Self::conversation_of(|name| std::env::var(name).ok())
    }

    pub(crate) fn windows(&self, accounts: &[Account]) -> Vec<LiveWindow> {
        let account = account(accounts);
        let mut windows: Vec<LiveWindow> = self
            .records()
            .iter()
            .filter_map(|record| {
                let inbox = self.inbox(record, &account)?;
                let conversation = record["conversation"].as_str()?.to_owned();
                let started_ms = record["started_ms"].as_u64().unwrap_or(0);
                let cwd = PathBuf::from(record["cwd"].as_str().unwrap_or("/"));
                Some(LiveWindow {
                    conversation,
                    started_ms,
                    cwd,
                    inbox,
                })
            })
            .collect();
        if let Some(conversation) = self.caller_conversation()
            && !windows
                .iter()
                .any(|window| window.conversation == conversation)
            && let Some(window) = self.caller_window(accounts, &conversation)
        {
            windows.push(window);
        }
        windows.sort_by_key(|window| std::cmp::Reverse(window.started_ms));
        windows
    }

    pub(crate) fn window_inbox(&self, accounts: &[Account], conversation: &str) -> Option<WindowInbox> {
        let account = account(accounts);
        self.records()
            .iter()
            .find(|record| record["conversation"].as_str() == Some(conversation))
            .and_then(|record| self.inbox(record, &account))
            .or_else(|| {
                self.caller_window(accounts, conversation)
                    .map(|window| window.inbox)
            })
    }

    pub(crate) fn tell_window(&self, inbox: &WindowInbox, text: &str) -> NorthResult<()> {
        let refused = |error: std::io::Error| {
            NorthError::State(format!(
                "the Pi window of process {} at {} refused the report: {error}; run /reload in Pi to load the north fleet extension",
                inbox.pid,
                inbox.socket.display()
            ))
        };
        let mut stream = UnixStream::connect(&inbox.socket).map_err(refused)?;
        stream.set_write_timeout(Some(DEADLINE)).map_err(refused)?;
        stream.write_all(text.as_bytes()).map_err(refused)?;
        stream.shutdown(Shutdown::Write).map_err(refused)
    }

    pub(crate) fn adopt_window(&self, conversation: &str, inbox: &WindowInbox) -> NorthResult<()> {
        self.install()?;
        self.register(conversation, inbox)
    }
}

fn account(accounts: &[Account]) -> String {
    accounts
        .first()
        .map_or_else(|| "go".to_owned(), |account| account.label.clone())
}

fn state_root() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|state| state.is_absolute())
        .unwrap_or_else(|| home.join(".local/state"));
    state.join("north")
}

fn agent_dir() -> PathBuf {
    if let Some(agent) = std::env::var_os(AGENT_ENV) {
        return PathBuf::from(agent);
    }
    let from_session = std::env::var_os(SESSION_FILE_ENV).map(PathBuf::from).and_then(|file| {
        let sessions = file.parent()?.parent()?;
        sessions.file_name().filter(|name| *name == "sessions")?;
        sessions.parent().map(Path::to_path_buf)
    });
    from_session.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".pi/agent"))
}

/// The Pi process a shell tool command runs under, through the shell that started it.
fn pi_ancestor() -> Option<u32> {
    let mut pid = std::os::unix::process::parent_id();
    while pid > 1 {
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        if comm.trim() == "pi" {
            let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let words: Vec<&[u8]> = cmdline.split(|byte| *byte == 0).collect();
            let headless = words.windows(2).any(|pair| pair[0] == b"--mode" && pair[1] == b"json") || words.iter().any(|word| *word == b"--print" || *word == b"-p");
            return (!headless).then_some(pid);
        }
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        pid = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|ppid| ppid.parse().ok())?;
    }
    None
}
