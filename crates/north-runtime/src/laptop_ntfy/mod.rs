mod http;
#[cfg(test)]
mod tests;

use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};

use self::http::{Log, Request, Respond};
use crate::foundation::clock::Timestamp;
use crate::ops::laptop::{Ctx, Zone};

const USAGE: &str = "north laptop-ntfy serve|notify|press: phone signals for the laptop availability state.

serve         HTTP endpoint the ntfy Open/Close buttons call: POST /laptop/open or /laptop/close with the action bearer token
notify        publish the state with Open/Close buttons once a day at the window's open edge
press VERB    run `north laptop VERB` as the phone would

Environment: LAPTOP_NTFY_URL, LAPTOP_NTFY_TOPIC, LAPTOP_NTFY_TOKEN_FILE (publisher), LAPTOP_NTFY_ACTION_TOKEN_FILE,
LAPTOP_NTFY_ACTION_URL (what the buttons call), LAPTOP_NTFY_LISTEN (host:port), LAPTOP_NTFY_STATE (directory),
LAPTOP_WINDOW_FILE, LAPTOP_NOW.
";
const REMINDER_SPAN: i64 = 1800;
const PUBLISH_WAIT: Duration = Duration::from_secs(15);

type Laptop = Arc<dyn Fn() -> Ctx + Send + Sync>;

struct Config {
    url: String,
    topic: String,
    token_file: PathBuf,
    action_token_file: PathBuf,
    action_url: String,
    listen: String,
    state_dir: PathBuf,
    window: PathBuf,
    laptop: Laptop,
}

impl Config {
    fn from_env() -> Self {
        let set = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let home = PathBuf::from(set("HOME").unwrap_or_default());
        Self {
            url: set("LAPTOP_NTFY_URL").unwrap_or_else(|| "http://10.77.0.1:2586".into()),
            topic: set("LAPTOP_NTFY_TOPIC").unwrap_or_else(|| "socrates-laptop".into()),
            token_file: set("LAPTOP_NTFY_TOKEN_FILE").map(PathBuf::from).unwrap_or_default(),
            action_token_file: set("LAPTOP_NTFY_ACTION_TOKEN_FILE").map(PathBuf::from).unwrap_or_default(),
            action_url: set("LAPTOP_NTFY_ACTION_URL").unwrap_or_else(|| "http://10.77.0.1:2588".into()),
            listen: set("LAPTOP_NTFY_LISTEN").unwrap_or_else(|| "10.77.0.1:2588".into()),
            state_dir: set("LAPTOP_NTFY_STATE").map_or_else(|| home.join(".local/state/laptop-ntfy"), PathBuf::from),
            window: set("LAPTOP_WINDOW_FILE").map_or_else(|| "/etc/placement/laptop-window.json".into(), PathBuf::from),
            laptop: Arc::new(Ctx::from_env),
        }
    }

    fn status(&self) -> Result<Value, String> {
        let out = (self.laptop)().main(&["status".into(), "--json".into()]).map_err(|die| die.message)?;
        serde_json::from_str(&out).map_err(|error| format!("laptop status --json: {error}"))
    }

    fn press(&self, verb: &str) -> Result<String, String> {
        match (self.laptop)().with_by("ntfy").main(&[verb.to_owned()]) {
            Ok(out) => Ok(out.trim().to_owned()),
            Err(die) if die.message.is_empty() => Err(format!("laptop {verb} exited {}", die.code)),
            Err(die) => Err(die.message),
        }
    }

    fn window(&self) -> Option<(Zone, String)> {
        let document = serde_json::from_str::<Value>(&fs::read_to_string(&self.window).ok()?).ok()?;
        let text = |key: &str| document.get(key).and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_owned);
        Some((Zone::named(Some(&text("tz")?)), text("open")?))
    }

    fn publish(&self, title: &str, message: &str, state: &str) -> Result<(), String> {
        let action_token = secret(&self.action_token_file)?;
        let actions: Vec<Value> = [("Open", "open"), ("Close", "close")]
            .iter()
            .map(|(label, verb)| {
                json!({"action": "http", "label": label, "url": format!("{}/laptop/{verb}", self.action_url), "method": "POST",
                       "headers": {"Authorization": format!("Bearer {action_token}")}, "clear": true})
            })
            .collect();
        let tag = match state {
            "available" => "green_circle",
            "draining" => "orange_circle",
            "unavailable" => "red_circle",
            _ => "laptop",
        };
        let body = json!({"topic": self.topic, "title": title, "message": message, "priority": 3, "tags": [tag], "actions": actions});
        http::post_json(&self.url, &secret(&self.token_file)?, &body.to_string(), PUBLISH_WAIT)
    }

    fn notify(&self, now: i64) -> Result<Option<&'static str>, String> {
        let status = self.status()?;
        let mut last = fs::read_to_string(self.state_dir.join("last.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Map<String, Value>>(&text).ok())
            .unwrap_or_default();
        let mut sent = None;
        if let Some((zone, opens)) = self.window() {
            let edge = zone.at_today(now, &opens).ok_or_else(|| format!("the window open time {opens} is not HH:MM"))?;
            let today = zone.day(now).to_string();
            if (edge..edge + REMINDER_SPAN).contains(&now) && last.get("reminded").and_then(Value::as_str) != Some(&today) {
                let text = |key: &str| status.get(key).and_then(Value::as_str).unwrap_or_default();
                let state = text("state");
                self.publish(&format!("Laptop window opens {opens}"), &format!("The laptop is {state} {}", describe(&status, &zone)), state)?;
                last.insert("reminded".into(), Value::String(today));
                sent = Some("laptop-ntfy: sent window reminder");
            }
        }
        fs::create_dir_all(&self.state_dir).map_err(|error| format!("{}: {error}", self.state_dir.display()))?;
        let temporary = self.state_dir.join("last.tmp");
        fs::write(&temporary, format!("{}\n", Value::Object(last))).map_err(|error| format!("{}: {error}", temporary.display()))?;
        fs::rename(&temporary, self.state_dir.join("last.json")).map_err(|error| error.to_string())?;
        Ok(sent)
    }

    fn respond(&self, request: &Request) -> (u16, String) {
        match request.method.as_str() {
            "POST" => {}
            "GET" => return (405, "POST only".into()),
            _ => return (501, "unsupported method".into()),
        }
        let want = match secret(&self.action_token_file) {
            Ok(token) => format!("Bearer {token}"),
            Err(error) => return (500, error),
        };
        if !same(request.authorization.as_bytes(), want.as_bytes()) {
            return (401, "unauthorized".into());
        }
        let verb = request.path.strip_prefix("/laptop/").unwrap_or_default();
        if !matches!(verb, "open" | "close") {
            return (404, "unknown action".into());
        }
        match self.press(verb) {
            Ok(out) => (200, out),
            Err(error) => (500, format!("laptop {verb} failed: {error}")),
        }
    }
}

fn same(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).fold(0, |difference, (a, b)| difference | (a ^ b)) == 0
}

fn secret(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map(|text| text.trim().to_owned()).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn describe(status: &Value, zone: &Zone) -> String {
    let text = |key: &str| status.get(key).and_then(Value::as_str).unwrap_or_default();
    let hhmm = |iso: &str| Timestamp::parse_rfc3339(iso).map_or_else(|| iso.to_owned(), |at| zone.hhmm(at.unix_seconds()));
    let mut out = format!("since {}", hhmm(text("since")));
    if !text("until").is_empty() {
        out.push_str(&format!(" until {}", hhmm(text("until"))));
    }
    format!("{out}, {} by {}", text("reason"), text("by"))
}

fn now() -> Result<i64, String> {
    match std::env::var("LAPTOP_NOW").ok().filter(|value| !value.is_empty()) {
        Some(text) => Timestamp::parse_rfc3339(&text).map(Timestamp::unix_seconds).ok_or_else(|| format!("LAPTOP_NOW is not a timestamp: {text}")),
        None => Ok(Timestamp::now().unix_seconds()),
    }
}

fn serve(config: Config) -> Result<(), String> {
    let (host, port) = config.listen.rsplit_once(':').ok_or_else(|| format!("LAPTOP_NTFY_LISTEN {} is not host:port", config.listen))?;
    let port: u16 = port.parse().map_err(|_| format!("LAPTOP_NTFY_LISTEN {} is not host:port", config.listen))?;
    let listener = TcpListener::bind((host, port)).map_err(|error| format!("{}: {error}", config.listen))?;
    let config = Arc::new(config);
    let respond: Respond = Arc::new(move |request| config.respond(request));
    let log: Log = Arc::new(|line| println!("laptop-ntfy: {line}"));
    http::serve(&listener, &respond, &log);
    Ok(())
}

fn command(arguments: &[String]) -> Result<(), (u8, String)> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let fail = |error: String| (1, error);
    match words.as_slice() {
        [] | ["-h" | "--help" | "help"] => print!("{USAGE}"),
        ["serve"] => serve(Config::from_env()).map_err(fail)?,
        ["notify"] => {
            if let Some(line) = Config::from_env().notify(now().map_err(fail)?).map_err(fail)? {
                println!("{line}");
            }
        }
        ["press", verb @ ("open" | "close")] => println!("{}", Config::from_env().press(verb).map_err(|error| fail(format!("laptop {verb} failed: {error}")))?),
        _ => return Err((2, "usage: north laptop-ntfy serve|notify|press open|close".into())),
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> u8 {
    match command(arguments) {
        Ok(()) => 0,
        Err((code, message)) => {
            eprintln!("laptop-ntfy: {message}");
            code
        }
    }
}
