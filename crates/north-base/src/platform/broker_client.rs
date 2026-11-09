use std::collections::BTreeMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::broker_wire::{Request, read_response, write_request};

pub const SOCKET_ENV: &str = "NORTH_BROKER_SOCKET";
pub const SYSTEM_SOCKET: &str = "/run/north-broker/broker.sock";
pub const AGENT_APP: &str = "agent";
pub const LANDER_APP: &str = "lander";
const START_LIMIT: Duration = Duration::from_secs(15);
const CALL_LIMIT: Duration = Duration::from_secs(90);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    pub token: String,
    pub expires_at: String,
    pub repositories: Option<Vec<String>>,
    pub permissions: BTreeMap<String, String>,
}

pub fn user_socket() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR").filter(|runtime| !runtime.is_empty()).map(|runtime| Path::new(&runtime).join("north-broker/broker.sock"))
}

pub struct Client {
    socket: Option<PathBuf>,
}

impl Client {
    pub fn discover() -> Self {
        Self { socket: None }
    }

    pub fn at(socket: &Path) -> Self {
        Self { socket: Some(socket.to_owned()) }
    }

    pub fn github_token(&self, app: &str, repositories: &[String], permissions: &BTreeMap<String, String>) -> Result<Grant, String> {
        let body = json!({ "app": app, "repositories": repositories, "permissions": permissions });
        let answer = self.call("POST", "/github/token", Some(&body))?;
        let text = |name: &str| answer[name].as_str().map(str::to_owned);
        let (Some(token), Some(expires_at)) = (text("token"), text("expires_at")) else {
            return Err("the broker's answer holds no token".to_owned());
        };
        Ok(Grant { token, expires_at, repositories: names(&answer["repositories"]), permissions: levels(&answer["permissions"]) })
    }

    pub fn inspect(&self, token: &str) -> Result<Value, String> {
        self.call("POST", "/github/inspect", Some(&json!({ "token": token })))
    }

    pub fn call(&self, method: &str, target: &str, body: Option<&Value>) -> Result<Value, String> {
        let stream = match &self.socket {
            Some(socket) => UnixStream::connect(socket).map_err(|error| format!("the broker socket {} does not answer: {error}", socket.display()))?,
            None => connect()?,
        };
        exchange(&stream, method, target, body)
    }
}

fn exchange(stream: &UnixStream, method: &str, target: &str, body: Option<&Value>) -> Result<Value, String> {
    stream.set_read_timeout(Some(CALL_LIMIT)).and_then(|()| stream.set_write_timeout(Some(CALL_LIMIT))).map_err(|error| error.to_string())?;
    let request = Request { method: method.to_owned(), target: target.to_owned(), headers: vec![("Content-Type".to_owned(), "application/json".to_owned())], body: body.map(Value::to_string).unwrap_or_default().into_bytes() };
    write_request(stream, &request).map_err(|error| format!("cannot send to the broker: {error}"))?;
    let response = read_response(stream).map_err(|error| format!("the broker's answer is unreadable: {error}"))?;
    let answer: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    if response.status / 100 != 2 {
        let why = answer["error"].as_str().map_or_else(|| format!("HTTP {}", response.status), str::to_owned);
        return Err(format!("the broker refused: {why}"));
    }
    Ok(answer)
}

fn names(value: &Value) -> Option<Vec<String>> {
    value.as_array().map(|list| list.iter().filter_map(|name| name.as_str().map(str::to_owned)).collect())
}

fn levels(value: &Value) -> BTreeMap<String, String> {
    value.as_object().map(|map| map.iter().filter_map(|(name, level)| Some((name.clone(), level.as_str()?.to_owned()))).collect()).unwrap_or_default()
}

fn connect() -> Result<UnixStream, String> {
    if let Some(path) = std::env::var_os(SOCKET_ENV).filter(|path| !path.is_empty()) {
        return UnixStream::connect(&path).map_err(|error| format!("the broker socket {} does not answer: {error}", Path::new(&path).display()));
    }
    if let Ok(stream) = UnixStream::connect(SYSTEM_SOCKET) {
        return Ok(stream);
    }
    let socket = user_socket().ok_or_else(|| format!("no broker answers at {SYSTEM_SOCKET} and XDG_RUNTIME_DIR is unset"))?;
    if let Ok(stream) = UnixStream::connect(&socket) {
        return Ok(stream);
    }
    start_user_broker(&socket)?;
    UnixStream::connect(&socket).map_err(|error| format!("the broker socket {} does not answer: {error}", socket.display()))
}

fn start_user_broker(socket: &Path) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let north = std::env::current_exe().map_err(|error| format!("cannot find the north binary: {error}"))?;
    Command::new(north).args(["broker", "serve", "--user"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn().map_err(|error| format!("cannot start the broker: {error}"))?;
    let started = Instant::now();
    while started.elapsed() < START_LIMIT {
        if UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(format!("no broker answers at {SYSTEM_SOCKET}, and the one started for this user did not answer at {} within {} s", socket.display(), START_LIMIT.as_secs()))
}
