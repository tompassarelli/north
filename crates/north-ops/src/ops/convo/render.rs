use std::collections::HashMap;
use std::path::Path;

use super::json::quote;
use super::query::{Hit, Info};
use super::rank::{Group, snippet};
use super::text::{char_count, char_prefix, short_project, splitlines, strip};

pub(super) const SHOW_CAP: usize = 8000;

pub(super) struct Palette {
    pub bold: &'static str,
    pub dim: &'static str,
    pub cyan: &'static str,
    pub yellow: &'static str,
    pub off: &'static str,
}

impl Palette {
    pub(super) fn new(on: bool) -> Self {
        if on {
            Self { bold: "\x1b[1m", dim: "\x1b[2m", cyan: "\x1b[36m", yellow: "\x1b[33m", off: "\x1b[0m" }
        } else {
            Self { bold: "", dim: "", cyan: "", yellow: "", off: "" }
        }
    }
}

pub(super) fn stamp(ts: Option<&str>) -> String {
    char_prefix(ts.unwrap_or(""), 16).replace('T', " ")
}

pub(super) fn who(hit: &Hit) -> String {
    hit.tool.as_ref().map_or_else(|| hit.role.clone(), |tool| format!("{}:{tool}", hit.role))
}

fn describe(info: Option<&Info>, hit: &Hit) -> (String, String) {
    let base = Path::new(&hit.session).file_name().map_or(String::new(), |name| name.to_string_lossy().into_owned());
    let agent = hit.agent.as_ref().map_or(String::new(), |agent| format!("/{}", char_prefix(agent, 8)));
    let project = info.and_then(|info| info.project.as_deref()).filter(|project| !project.is_empty()).or(hit.project.as_deref());
    (format!("{}{agent}", char_prefix(&base, 8)), short_project(project))
}

pub(super) fn full_lines(hit: &Hit, indent: &str) -> Vec<String> {
    let text = char_prefix(&hit.text, SHOW_CAP);
    let more = char_count(&hit.text) - char_count(text);
    let mut lines: Vec<String> = splitlines(text).into_iter().filter(|line| !strip(line).is_empty()).map(|line| format!("{indent}{line}")).collect();
    if more > 0 {
        lines.push(format!("{indent}… {more} more chars: north convo show {} --full", hit.id));
    }
    lines
}

pub(super) fn render_sessions(groups: &[Group], infos: &HashMap<String, Info>, terms: &[String], pal: &Palette, full: bool) -> Vec<String> {
    let mut out = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if group.partial && (index == 0 || !groups[index - 1].partial) {
            out.push(format!("{}no message in these sessions has every word; ranked by words covered{}", pal.dim, pal.off));
        }
        let hit = &group.best;
        let info = infos.get(&group.session);
        let (sid, project) = describe(info, hit);
        let title = char_prefix(info.and_then(|info| info.title.as_deref()).unwrap_or(""), 60);
        let provider = info.map_or("?", |info| info.provider.as_str());
        out.push(format!(
            "{}{:2}{} {}{sid}{}  {}  {}{project}{}  {title}  {}({provider}, {} hit{}){}",
            pal.dim, index + 1, pal.off, pal.bold, pal.off, stamp(hit.ts.as_deref()), pal.cyan, pal.off, pal.dim, group.hits, if group.hits == 1 { "" } else { "s" }, pal.off
        ));
        if full {
            out.push(format!("   {}{}{} [{}]", pal.yellow, who(hit), pal.off, hit.id));
            out.extend(full_lines(hit, "   "));
        } else {
            out.push(format!("   {}{}{} · {}  {}[{}]{}", pal.yellow, who(hit), pal.off, snippet(&hit.text, terms, 200), pal.dim, hit.id, pal.off));
        }
    }
    out.push(format!("{}open: north convo show <id>  ·  transcripts: north convo session <session>{}", pal.dim, pal.off));
    out
}

pub(super) fn render_messages(hits: &[Hit], infos: &HashMap<String, Info>, terms: &[String], pal: &Palette, full: bool, partial: bool) -> Vec<String> {
    let mut out = Vec::new();
    if partial {
        out.push(format!("{}no message has every term; showing partial matches{}", pal.dim, pal.off));
    }
    for (index, hit) in hits.iter().enumerate() {
        let (sid, project) = describe(infos.get(&hit.session), hit);
        let head = format!(
            "{}{:2}{} {}  {}{sid}{}  {}{project}{}  {}{}{}",
            pal.dim, index + 1, pal.off, stamp(hit.ts.as_deref()), pal.bold, pal.off, pal.cyan, pal.off, pal.yellow, who(hit), pal.off
        );
        if full {
            out.push(format!("{head}  [{}]", hit.id));
            out.extend(full_lines(hit, "   "));
        } else {
            out.push(format!("{head} · {}  {}[{}]{}", snippet(&hit.text, terms, 160), pal.dim, hit.id, pal.off));
        }
    }
    out
}

pub(super) struct Row<'a> {
    pub hit: &'a Hit,
    pub info: Option<&'a Info>,
    pub terms: &'a [String],
    pub hits: Option<usize>,
    pub full: bool,
    pub partial: Option<bool>,
}

fn nullable(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), |text| quote(text, false))
}

/// One result as the JSON object convo has always printed, keys in this order.
pub(super) fn row_json(row: &Row) -> String {
    let hit = row.hit;
    let info = row.info;
    let project = info.and_then(|info| info.project.as_deref()).filter(|project| !project.is_empty()).or(hit.project.as_deref());
    let mut fields = vec![
        ("session", quote(&hit.session, false)),
        ("agent", nullable(hit.agent.as_deref())),
        ("title", nullable(info.and_then(|info| info.title.as_deref()))),
        ("project", quote(&short_project(project), false)),
        ("provider", info.map_or_else(|| "null".to_owned(), |info| quote(&info.provider, false))),
        ("id", hit.id.to_string()),
        ("ts", nullable(hit.ts.as_deref())),
        ("role", quote(&hit.role, false)),
        ("tool", nullable(hit.tool.as_deref())),
        ("snippet", quote(&snippet(&hit.text, row.terms, 200), false)),
        ("path", quote(&hit.path, false)),
        ("line", hit.line.to_string()),
    ];
    if let Some(hits) = row.hits {
        fields.push(("hits", hits.to_string()));
    }
    if row.full {
        fields.push(("text", quote(&hit.text, false)));
    }
    if let Some(partial) = row.partial {
        fields.push(("partial", partial.to_string()));
    }
    format!("{{{}}}", fields.iter().map(|(key, value)| format!("\"{key}\": {value}")).collect::<Vec<_>>().join(", "))
}

pub(super) fn json_list(rows: &[String]) -> String {
    format!("[\n{}\n]", rows.join(",\n"))
}

