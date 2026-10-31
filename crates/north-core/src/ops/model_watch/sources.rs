use std::collections::BTreeSet;

use regex::Regex;
use serde_json::{Value, json};

use super::html::PublicPage;
use super::models::{ATOM, PHRASE, Row, Rows, Source, clean, date, model_names, probability, sections, splitlines, string_field, text_of, truthy};
use super::xml;

struct Collector<'a> {
    rows: Rows,
    endpoint: &'a str,
}

struct Detail<'a> {
    at: Option<String>,
    p: Option<f64>,
    url: &'a str,
    basis: &'a str,
}

type Entries = Vec<(String, String, Option<String>)>;

impl Collector<'_> {
    fn add(&mut self, key: &str, model: &str, kind: &str, title: &str, detail: Detail) {
        self.rows.insert(
            format!("{key}|{model}"),
            Row { model: model.into(), kind: kind.into(), title: clean(title), at: detail.at, p: detail.p, url: detail.url.into(), basis: detail.basis.into() },
        );
    }
}

fn utf8(payload: &[u8]) -> Result<&str, String> {
    std::str::from_utf8(payload).map_err(|_| "payload is not UTF-8".to_owned())
}

pub(super) fn normalize(source: &Source, payload: &[u8]) -> Result<Rows, String> {
    let mut collector = Collector { rows: Rows::new(), endpoint: &source.endpoint };
    match source.parser.as_str() {
        "claude-models" => claude_models(&mut collector, payload)?,
        "rss" | "atom" | "anthropic-news" | "changelog" => announcements(&mut collector, source, payload)?,
        "manifold" | "polymarket" | "kalshi" => markets(&mut collector, &source.parser, payload)?,
        _ => return Err("unknown parser".into()),
    }
    Ok(collector.rows)
}

fn claude_models(collector: &mut Collector, payload: &[u8]) -> Result<(), String> {
    let text = PublicPage::parse(utf8(payload)?).text.join(" ");
    let ids = Regex::new(r"\bclaude-(?:opus|sonnet|haiku|fable)-\d+(?:-\d+)*\b").expect("model id pattern");
    let slugs: BTreeSet<&str> = ids.find_iter(&text).map(|found| found.as_str()).collect();
    for slug in &slugs {
        let detail = Detail { at: None, p: None, url: collector.endpoint, basis: "public model overview availability; first-seen is not a release date" };
        collector.add(slug, &format!("anthropic:{slug}"), "released", slug, detail);
    }
    if slugs.is_empty() {
        return Err("no model IDs in overview".into());
    }
    Ok(())
}

fn feed_entries(parser: &str, payload: &[u8]) -> Result<Entries, String> {
    let root = xml::parse(payload)?;
    let prefix = if parser == "atom" { ATOM } else { "" };
    let found = if prefix.is_empty() { root.descendants_named("item") } else { root.find_all(&format!("{prefix}entry")) };
    let mut entries = Vec::new();
    for entry in found {
        let title = entry.text_or(&format!("{prefix}title"), "");
        let url = match entry.find(&format!("{prefix}link")) {
            Some(link) if !prefix.is_empty() => link.attribute("href").unwrap_or("").to_owned(),
            _ => entry.text_or("link", ""),
        };
        let published = entry.find_text(&format!("{prefix}published")).filter(|text| !text.is_empty()).or_else(|| entry.find_text("pubDate"));
        entries.push((url, title, published.and_then(|text| date(&Value::String(text)))));
    }
    Ok(entries)
}

fn news_entries(payload: &[u8]) -> Result<Entries, String> {
    let observed = Regex::new(r"\b[A-Z][a-z]{2} \d{1,2}, \d{4}\b").expect("observed date pattern");
    let mut entries = Vec::new();
    for (href, title) in PublicPage::parse(utf8(payload)?).links {
        if href.starts_with("/news/") || href.starts_with("/claude-") {
            let at = observed.find(&title).and_then(|found| date(&Value::String(found.as_str().into())));
            entries.push((format!("https://www.anthropic.com{href}"), title, at));
        }
    }
    Ok(entries)
}

fn changelog_entries(endpoint: &str, payload: &[u8]) -> Result<Entries, String> {
    let mut entries = Vec::new();
    for (version, body) in sections(utf8(payload)?) {
        for line in splitlines(&body) {
            if !model_names(line).is_empty() {
                entries.push((format!("{endpoint}#{}", version.trim()), line.to_owned(), None));
            }
        }
    }
    Ok(entries)
}

fn announcements(collector: &mut Collector, source: &Source, payload: &[u8]) -> Result<(), String> {
    let entries = match source.parser.as_str() {
        "rss" | "atom" => feed_entries(&source.parser, payload)?,
        "anthropic-news" => news_entries(payload)?,
        _ => changelog_entries(&source.endpoint, payload)?,
    };
    let phrase = Regex::new(&format!(r"(?i)\b(?:introducing|meet|added support for)\s+(?:the\s+)?({PHRASE})\b")).expect("release phrase");
    let expected = Regex::new(r"(?i)\b(upcoming|coming soon|will release|expected|preview)\b").expect("expected phrase");
    for (url, text, at) in entries {
        let mut names = model_names(&text);
        if names.is_empty() {
            continue;
        }
        let (kind, basis) = if let Some(release) = phrase.captures(&text) {
            names = model_names(&release[1]);
            ("released", "explicit official availability/support wording")
        } else if expected.is_match(&text) {
            ("expected", "official preview/expectation wording; not confirmed availability")
        } else {
            continue;
        };
        for model in names {
            collector.add(&url, &model, kind, &text, Detail { at: at.clone(), p: None, url: &url, basis });
        }
    }
    Ok(())
}

fn market_list(parser: &str, data: &Value) -> Result<Vec<Value>, String> {
    match parser {
        "manifold" => data.as_array().cloned().ok_or_else(|| "market list required".to_owned()),
        _ if !data.is_object() => Err("market object required".into()),
        "kalshi" => data["markets"].as_array().cloned().ok_or_else(|| "markets required".to_owned()),
        _ => {
            let mut all = Vec::new();
            for event in data["events"].as_array().ok_or("events required")? {
                let title = event.get("title").cloned().unwrap_or_else(|| json!(""));
                for market in event.get("markets").and_then(Value::as_array).into_iter().flatten() {
                    let mut market = market.clone();
                    market["event_title"] = title.clone();
                    all.push(market);
                }
            }
            Ok(all)
        }
    }
}

struct Quote {
    key: String,
    p: Option<f64>,
    at: Option<String>,
    url: String,
    basis: &'static str,
}

fn quote(parser: &str, endpoint: &str, market: &Value) -> Result<Option<Quote>, String> {
    let null = Value::Null;
    let field = |name: &str| market.get(name).unwrap_or(&null);
    Ok(Some(match parser {
        "manifold" => Quote {
            key: text_of(market.get("id").ok_or("id required")?),
            p: probability(field("probability")),
            at: date(field("closeTime")),
            url: market.get("url").map_or_else(|| endpoint.to_owned(), text_of),
            basis: "Manifold probability; market close time, not an announced release date",
        },
        "polymarket" => {
            let listed = |name: &str| -> Result<Vec<Value>, String> {
                serde_json::from_str(market.get(name).and_then(Value::as_str).unwrap_or("[]")).map_err(|error| error.to_string())
            };
            let (outcomes, prices) = (listed("outcomes")?, listed("outcomePrices")?);
            let Some(yes) = outcomes.iter().position(|outcome| outcome.as_str() == Some("Yes")).filter(|_| outcomes.len() == prices.len()) else { return Ok(None) };
            Quote {
                key: text_of(market.get("id").ok_or("id required")?),
                p: probability(&prices[yes]),
                at: date(field("endDate")),
                url: format!("https://polymarket.com/market/{}", string_field(market, "slug")?),
                basis: "Yes outcome price (market inference); market end time, not an announced release date",
            }
        }
        _ => {
            let midpoint = match (probability(field("yes_bid_dollars")), probability(field("yes_ask_dollars"))) {
                (Some(bid), Some(ask)) if bid <= ask => format!("{:.6}", (bid + ask) / 2.0).parse().ok(),
                _ => None,
            };
            Quote {
                key: string_field(market, "ticker")?.to_owned(),
                p: midpoint,
                at: date(field("close_time")),
                url: endpoint.to_owned(),
                basis: "Yes bid/ask midpoint (market inference); market close time, not an announced release date",
            }
        }
    }))
}

fn markets(collector: &mut Collector, parser: &str, payload: &[u8]) -> Result<(), String> {
    let data: Value = serde_json::from_slice(payload).map_err(|error| error.to_string())?;
    let release_words = Regex::new(r"(?i)\b(releas\w*|launch\w*)\b").expect("market pattern");
    for market in &market_list(parser, &data)? {
        let question = market.get("question").or_else(|| market.get("title")).cloned().unwrap_or_else(|| json!(""));
        let title = clean(question.as_str().ok_or("question must be text")?);
        let text = format!("{} {title}", market.get("event_title").and_then(Value::as_str).unwrap_or(""));
        if !release_words.is_match(&text) || truthy(market.get("isResolved")) || truthy(market.get("closed")) {
            continue;
        }
        let names = model_names(&text);
        if names.is_empty() {
            continue;
        }
        let Some(quote) = quote(parser, collector.endpoint, market)? else { continue };
        if quote.p.is_some() {
            for model in &names {
                collector.add(&quote.key, model, "market", &title, Detail { at: quote.at.clone(), p: quote.p, url: &quote.url, basis: quote.basis });
            }
        }
    }
    Ok(())
}
