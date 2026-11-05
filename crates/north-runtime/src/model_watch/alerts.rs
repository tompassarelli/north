use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use super::geometry::{Location, nearby, weather_nearby};
use super::host::{Host, shell_quote};
use super::models::{ATOM, Source, clean, date, pattern};
use super::xml::{self, Element};
use super::{Options, toml_json};
use crate::foundation::clock::Timestamp;

const CAP: &str = "{urn:oasis:names:tc:emergency:cap:1.2}";
const GDACS: &str = "{http://www.gdacs.org}";
const GEO: &str = "{http://www.w3.org/2003/01/geo/wgs84_pos#}";
const DC: &str = "{http://purl.org/dc/elements/1.1/}";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS watch_alerts(
  key TEXT PRIMARY KEY, sent_at TEXT NOT NULL, source TEXT NOT NULL,
  title TEXT NOT NULL, url TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS watch_advisory_levels(
  source TEXT NOT NULL, country TEXT NOT NULL, level INTEGER NOT NULL,
  PRIMARY KEY(source,country)
) STRICT;
";

#[derive(Debug, PartialEq)]
pub(super) struct Alert {
    pub(super) key: String,
    pub(super) title: String,
    pub(super) url: String,
    pub(super) level: Option<i64>,
    pub(super) country: String,
}

pub(super) fn location_config(path: &Path) -> Result<Location, String> {
    let text = std::fs::read_to_string(super::host::expand_home(&path.to_string_lossy())).map_err(|error| error.to_string())?;
    let document = text.parse::<toml_edit::DocumentMut>().map_err(|error| error.to_string())?;
    let table = toml_json(document.as_item());
    if table["place"].as_str().is_none_or(|place| place.trim().is_empty()) {
        return Err("location place required".into());
    }
    let number = |name: &str, default: Option<f64>| -> Result<f64, String> {
        match table.get(name) {
            Some(Value::Number(number)) => number.as_f64().ok_or_else(|| "invalid location".to_owned()),
            Some(Value::String(text)) => text.trim().parse().map_err(|_| "invalid location".to_owned()),
            None => default.ok_or_else(|| "invalid location".to_owned()),
            Some(_) => Err("invalid location".into()),
        }
    };
    let (lat, lon, radius_km) = (number("lat", None)?, number("lon", None)?, number("radius_km", Some(150.0))?);
    let finite = [lat, lon, radius_km].iter().all(|value| value.is_finite());
    let inside = (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && radius_km > 0.0 && radius_km <= 20001.6;
    if !finite || !inside {
        return Err("invalid location".into());
    }
    let option = |name: &str| -> Result<Option<String>, String> {
        match table.get(name) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => Err("invalid location option".into()),
        }
    };
    Ok(Location { lat, lon, radius_km, weather_feed: option("weather_feed")?, advisory_country: option("advisory_country")?.unwrap_or_default() })
}

fn http_url(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once(':') else { return false };
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
}

struct Rows<'a> {
    source: &'a str,
    rows: Vec<Alert>,
}

impl Rows<'_> {
    fn add(&mut self, identifier: &str, title: &str, url: &str, level: Option<i64>, country: &str) {
        if !identifier.is_empty() && !title.is_empty() && http_url(url) {
            self.rows.push(Alert { key: format!("{}:{identifier}", self.source), title: clean(title), url: url.into(), level, country: country.into() });
        }
    }
}

fn first_text(candidates: &[Option<String>]) -> String {
    candidates.iter().flatten().find(|text| !text.is_empty()).cloned().unwrap_or_default()
}

pub(super) fn cap_links(payload: &[u8]) -> Result<Vec<String>, String> {
    let root = xml::parse(payload)?;
    if root.tag == format!("{CAP}alert") {
        return Ok(Vec::new());
    }
    let mut links = Vec::new();
    for item in root.descendants_named("item") {
        let link = item.text_or("link", "").trim().to_owned();
        let severe = item.find_text("severity").is_some_and(|text| !text.is_empty());
        if !severe && item.find_all(&format!("{CAP}info")).is_empty() && link.ends_with(".xml") {
            links.push(link);
        }
    }
    links.truncate(40);
    Ok(links)
}

pub(super) fn alert_rows(source: &Source, payload: &[u8], location: &Location, now: &str) -> Result<Vec<Alert>, String> {
    let mut found = Rows { source: &source.name, rows: Vec::new() };
    if source.parser == "usgs" {
        let data: Value = serde_json::from_slice(payload).map_err(|error| error.to_string())?;
        for event in data["features"].as_array().ok_or("features required")? {
            let props = &event["properties"];
            let coordinates = event["geometry"]["coordinates"].as_array().ok_or("coordinates required")?;
            let (lon, lat) = (coordinates.first().and_then(Value::as_f64).ok_or("lon required")?, coordinates.get(1).and_then(Value::as_f64).ok_or("lat required")?);
            if props["mag"].as_f64().is_some_and(|magnitude| magnitude >= 4.5) && nearby(location, lat, lon, 0.0)? {
                let identifier = match &event["id"] {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                found.add(&identifier, props["title"].as_str().ok_or("title required")?, props["url"].as_str().ok_or("url required")?, None, "");
            }
        }
        return Ok(found.rows);
    }
    let root = xml::parse(payload)?;
    match source.parser.as_str() {
        "gdacs" => gdacs(&mut found, &root, location)?,
        "travel" => travel(&mut found, &root, location),
        "weather" => weather(&mut found, &root, location, now)?,
        _ => return Err("unknown alert parser".into()),
    }
    Ok(found.rows)
}

fn gdacs(found: &mut Rows, root: &Element, location: &Location) -> Result<(), String> {
    for entry in root.descendants_named("item") {
        if !matches!(entry.text_or(&format!("{GDACS}alertlevel"), "").to_lowercase().as_str(), "orange" | "red") {
            continue;
        }
        if entry.text_or(&format!("{GDACS}iscurrent"), "").to_lowercase() != "true" {
            continue;
        }
        let Some(point) = entry.find(&format!("{GEO}Point")) else { continue };
        let coordinate = |name: &str| -> Result<f64, String> {
            point.find_text(&format!("{GEO}{name}")).ok_or("coordinate required")?.trim().parse().map_err(|_| "invalid coordinate".to_owned())
        };
        if nearby(location, coordinate("lat")?, coordinate("long")?, 0.0)? {
            let identifier = entry.text_or(&format!("{GDACS}eventid"), "");
            let key = if identifier.is_empty() { String::new() } else { entry.text_or(&format!("{GDACS}eventtype"), "") + &identifier };
            found.add(&key, &entry.text_or("title", ""), &entry.text_or("link", ""), None, "");
        }
    }
    Ok(())
}

fn level_in(text: &str) -> Option<i64> {
    static FORM: OnceLock<Regex> = OnceLock::new();
    pattern(&FORM, r"(?i)\bLevel\s*([1-4])\b").captures(text).and_then(|found| found[1].parse().ok())
}

fn travel(found: &mut Rows, root: &Element, location: &Location) {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let country = location.advisory_country.to_lowercase();
    for entry in root.descendants_named("item") {
        let title = entry.text_or("title", "");
        let name = pattern(&SPLIT, r"(?i)\s*-\s*(?:Level|Travel Advisory)").splitn(&title, 2).next().unwrap_or("").trim().to_lowercase();
        let categories = entry.find_all("category");
        let tags: Vec<String> = categories.iter().filter(|category| category.attribute("domain") == Some("Country-Tag")).map(|category| clean(&category.text).to_lowercase()).collect();
        if country.is_empty() || !(country == name || tags.contains(&country)) {
            continue;
        }
        let threat = categories.iter().find(|category| category.attribute("domain") == Some("Threat-Level")).and_then(|category| level_in(&category.text));
        if let Some(level) = threat.or_else(|| level_in(&title)) {
            let identifier = first_text(&[entry.find_text("guid"), entry.find_text(&format!("{DC}identifier"))]);
            found.add(&identifier, &title, &entry.text_or("link", ""), Some(level), &country);
        }
    }
}

fn weather(found: &mut Rows, root: &Element, location: &Location, now: &str) -> Result<(), String> {
    let cap = |name: &str| format!("{CAP}{name}");
    let entries: Vec<&Element> = if root.tag == cap("alert") {
        vec![root]
    } else {
        let atom = root.find_all(&format!("{ATOM}entry"));
        if atom.is_empty() { root.descendants_named("item") } else { atom }
    };
    for entry in entries {
        let status = entry.find_text(&cap("status")).unwrap_or_else(|| entry.text_or("status", "Actual"));
        if status != "Actual" || entry.text_or(&cap("msgType"), "") == "Cancel" {
            continue;
        }
        let mut infos = entry.find_all(&cap("info"));
        if infos.is_empty() {
            infos = vec![entry];
        }
        for info in infos {
            let field = |name: &str| info.find_text(&cap(name)).unwrap_or_else(|| info.text_or(name, ""));
            if !matches!(field("severity").to_lowercase().as_str(), "severe" | "extreme" | "orange" | "red") {
                continue;
            }
            let expires = field("expires");
            if !expires.is_empty() && date(&Value::String(expires)).is_none_or(|expires| expires.as_str() <= now) {
                continue;
            }
            if !weather_nearby(info, location)? {
                continue;
            }
            let atom_link = entry.find(&format!("{ATOM}link")).map(|link| link.attribute("href").unwrap_or("").to_owned());
            let url = first_text(&[info.find_text(&cap("web")), entry.find_text("link"), atom_link]);
            let identifier = first_text(&[entry.find_text(&cap("identifier")), entry.find_text(&format!("{ATOM}id")), entry.find_text("guid")]);
            let title = first_text(&[info.find_text(&cap("headline")), entry.find_text(&format!("{ATOM}title")), entry.find_text("title"), info.find_text(&cap("event"))]);
            found.add(&identifier, &title, &url, None, "");
            break;
        }
    }
    Ok(())
}

fn form_encode(text: &str) -> String {
    text.bytes().map(|byte| match byte {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => (byte as char).to_string(),
        b' ' => "+".to_owned(),
        other => format!("%{other:02X}"),
    }).collect()
}

fn usgs_query(location: &Location, now: Timestamp) -> String {
    let start = Timestamp::from_unix_seconds(now.unix_seconds() - 86_400).iso();
    let fields = [
        ("format", "geojson".to_owned()),
        ("latitude", format!("{:?}", location.lat)),
        ("longitude", format!("{:?}", location.lon)),
        ("maxradiuskm", format!("{:?}", location.radius_km)),
        ("minmagnitude", "4.5".to_owned()),
        ("starttime", start),
    ];
    fields.iter().map(|(name, value)| format!("{name}={}", form_encode(value))).collect::<Vec<_>>().join("&")
}

fn deliver(host: &dyn Host, options: &Options, alert: &Alert) -> Result<(), String> {
    let topic = options.ntfy_topic.as_str();
    if topic.is_empty() || !topic.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err("invalid ntfy topic".into());
    }
    let title = "Socrates: location warning";
    let message = format!("{}\n{}", alert.title, alert.url);
    let timeout = Duration::from_secs_f64(options.timeout + 15.0);
    let finished = match &options.ntfy_url {
        None => {
            let remote = ["nexus-alert", topic, &message, title, &alert.url].map(shell_quote).join(" ");
            let args = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "nexus", &remote].map(str::to_owned);
            host.command("ssh", &args, None, timeout, &[])?
        }
        Some(base) => {
            let mut args: Vec<String> = ["-sS", "--fail", "--max-time", &options.timeout.to_string(), "-X", "POST"].map(str::to_owned).to_vec();
            for header in [format!("Title: {title}"), "Priority: 5".into(), format!("Click: {}", alert.url), "Tags: warning".into(), "Content-Type: text/plain; charset=utf-8".into()] {
                args.extend(["-H".into(), header]);
            }
            if let Some(file) = &options.ntfy_token_file {
                let token = std::fs::read_to_string(file).map_err(|_| "OSError".to_owned())?;
                args.extend(["-H".into(), format!("Authorization: Bearer {}", token.trim())]);
            }
            args.extend(["--data-binary".into(), "@-".into(), format!("{}/{topic}", base.trim_end_matches('/'))]);
            host.command("curl", &args, Some(&message), timeout, &[])?
        }
    };
    if finished.code == 0 { Ok(()) } else { Err("CalledProcessError".into()) }
}

pub(super) fn prepare(source: &mut Source, location: &Location, now: Timestamp) -> bool {
    match source.parser.as_str() {
        "weather" => match &location.weather_feed {
            Some(feed) if !feed.is_empty() => source.endpoint = feed.clone(),
            _ => return false,
        },
        "travel" if location.advisory_country.is_empty() => return false,
        "usgs" => source.endpoint = format!("{}?{}", source.endpoint, usgs_query(location, now)),
        _ => {}
    }
    true
}

fn sources_of(config: &Value) -> Vec<Source> {
    config["alert_sources"].as_array().into_iter().flatten().map(|item| Source {
        name: item["name"].as_str().unwrap_or_default().into(),
        parser: item["parser"].as_str().unwrap_or_default().into(),
        endpoint: item["endpoint"].as_str().unwrap_or_default().into(),
    }).collect()
}

fn qualifies(alert: &Alert, previous: Option<i64>) -> bool {
    alert.level.is_none_or(|level| level >= 3 || previous.is_some_and(|before| level > before))
}

fn deliver_all(db: &Connection, host: &dyn Host, options: &Options, name: &str, rows: &[Alert], now: &str, failed: &mut Option<String>) -> Result<i64, String> {
    let sql = |error: rusqlite::Error| error.to_string();
    let mut sent = 0;
    for alert in rows {
        let previous: Option<i64> = match alert.level {
            Some(_) => db.query_row("SELECT level FROM watch_advisory_levels WHERE source=? AND country=?", params![name, alert.country], |row| row.get(0)).optional().map_err(sql)?,
            None => None,
        };
        db.execute_batch("BEGIN IMMEDIATE").map_err(sql)?;
        let outcome = (|| -> Result<bool, String> {
            let mut delivered = false;
            if qualifies(alert, previous) && db.query_row("SELECT 1 FROM watch_alerts WHERE key=?", [&alert.key], |_| Ok(())).optional().map_err(sql)?.is_none() {
                if let Err(error) = deliver(host, options, alert) {
                    *failed = Some(error.clone());
                    return Err(error);
                }
                db.execute("INSERT INTO watch_alerts VALUES(?,?,?,?,?)", params![alert.key, now, name, alert.title, alert.url]).map_err(sql)?;
                delivered = true;
            }
            if let Some(level) = alert.level {
                db.execute("INSERT OR REPLACE INTO watch_advisory_levels VALUES(?,?,?)", params![name, alert.country, level]).map_err(sql)?;
            }
            Ok(delivered)
        })();
        match outcome {
            Ok(delivered) => {
                db.execute_batch("COMMIT").map_err(sql)?;
                sent += i64::from(delivered);
            }
            Err(error) => {
                let _ = db.execute_batch("ROLLBACK");
                return Err(error);
            }
        }
    }
    Ok(sent)
}

pub(super) fn run(host: &dyn Host, options: &Options, config: &Value) -> u8 {
    let location = match location_config(&options.location) {
        Ok(location) => location,
        Err(_) => {
            eprintln!("model-watch: location unavailable; skipped location alerts");
            return 0;
        }
    };
    if let Some(parent) = options.db.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(db) = Connection::open(&options.db) else {
        eprintln!("model-watch: ledger unavailable");
        return 1;
    };
    let _ = db.busy_timeout(Duration::from_secs(30));
    if let Err(error) = db.execute_batch(SCHEMA) {
        eprintln!("model-watch: {error}");
        return 1;
    }
    let (mut total, mut summaries, mut failed) = (0, Vec::new(), None);
    for mut source in sources_of(config) {
        if !prepare(&mut source, &location, host.now()) {
            continue;
        }
        let name = source.name.clone();
        let outcome = (|| -> Result<i64, String> {
            let now = host.now().iso();
            let payload = host.fetch(&source, options.timeout)?;
            let mut rows = alert_rows(&source, &payload, &location, &now)?;
            if source.parser == "weather" {
                for link in cap_links(&payload)? {
                    let linked = Source { name: source.name.clone(), parser: source.parser.clone(), endpoint: link };
                    rows.extend(alert_rows(&linked, &host.fetch(&linked, options.timeout)?, &location, &now)?);
                }
            }
            deliver_all(&db, host, options, &name, &rows, &now, &mut failed)
        })();
        match outcome {
            Ok(sent) => {
                total += sent;
                summaries.push(json!({"source": name, "sent": sent}));
            }
            Err(error) => summaries.push(json!({"source": name, "error": error})),
        }
    }
    let mut result = json!({"alerts": total, "sources": summaries, "invocations": 0});
    if let Some(error) = &failed {
        result["delivery_failed"] = json!(error);
    }
    println!("{result}");
    u8::from(failed.is_some())
}
