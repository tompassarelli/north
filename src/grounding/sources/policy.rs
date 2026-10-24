use super::*;
use std::collections::HashMap;
fn limited_read(path: &Path) -> String {
    cut(&read(path), 262144)
}
fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default()
}
fn is_code(path: &Path) -> bool {
    [".py", ".sh", ".ts", ".mjs", ".rs", ".js", ""].contains(&extension(path).as_str())
}
pub fn frontmatter(text: &str) -> (HashMap<String, String>, String) {
    if !text.starts_with("---\n") {
        return (HashMap::new(), text.into());
    }
    let Some(end) = text[4..].find("\n---").map(|n| n + 4) else {
        return (HashMap::new(), text.into());
    };
    let mut meta = HashMap::new();
    let mut key = String::new();
    for line in text[4..end].split('\n') {
        if let Some(c) = re(r"^([A-Za-z_-]+):\s*(.*)$").captures(line) {
            key = c[1].into();
            meta.insert(
                key.clone(),
                c[2].trim()
                    .trim_start_matches(['>', '|', '-'])
                    .trim()
                    .to_owned(),
            );
        } else if !key.is_empty() && line.starts_with(' ') {
            let v = meta.get_mut(&key).unwrap();
            *v = format!("{} {}", v, line.trim()).trim().to_owned();
        }
    }
    (meta, text[end + 4..].into())
}
pub fn markdown_sections(text: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut heading = String::new();
    let mut body = Vec::new();
    for line in text.split('\n') {
        if re(r"^#{1,4} ").is_match(line) {
            if !body.join("").trim().is_empty() {
                sections.push((heading.clone(), body.join("\n")));
            }
            heading = line.trim_start_matches('#').trim().into();
            body.clear();
        } else {
            body.push(line);
        }
    }
    if !body.join("").trim().is_empty() {
        sections.push((heading, body.join("\n")));
    }
    let mut out = Vec::new();
    for (h, b) in sections {
        for i in (0..ulen(&b).max(1)).step_by(1800) {
            out.push((h.clone(), slice(&b, i, 1800)));
        }
    }
    out
}
fn leading_comments(text: &str) -> Vec<String> {
    let mut keep = Vec::new();
    for line in text.split('\n').take(120) {
        let s = line.trim();
        if keep.is_empty() && (s.starts_with("#!") || s.is_empty()) {
            continue;
        }
        if re(r"^(#|//|/\*|\*|--)").is_match(s) {
            keep.push(
                s.trim_start_matches(['#', '/', '*', '-', ' '])
                    .trim()
                    .to_owned(),
            );
            continue;
        }
        if s.is_empty() || re(r"^(set -|import |from |use |const |export |source |\. )").is_match(s)
        {
            continue;
        }
        break;
    }
    keep.into_iter()
        .filter(|l| !l.is_empty() && !l.starts_with("shellcheck"))
        .collect()
}
fn words(name: &str) -> String {
    re(r"[_-]+")
        .replace_all(&re(r"([a-z0-9])([A-Z])").replace_all(name, "$1 $2"), " ")
        .to_lowercase()
}
fn defined_names(text: &str, ext: &str) -> Vec<String> {
    let patterns: Vec<&str> = if ext == ".py" {
        vec![
            r"(?m)^(?:async\s+)?(?:def|class)\s+([A-Za-z]\w*)",
            r#"add_parser\(\s*["']([\w-]+)["']"#,
            r#"add_argument\(\s*["'](--?[\w-]+)["']"#,
        ]
    } else if ext == ".rs" {
        vec![r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:fn|struct|enum|trait)\s+([A-Za-z]\w*)"]
    } else {
        vec![
            r"(?m)^(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:function\*?|class)\s+([A-Za-z]\w*)",
            r"(?m)^(?:export\s+)?const\s+([A-Za-z]\w*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z]\w*)\s*(?::[^=]+)?=>",
            r"(?m)^([A-Za-z][\w-]*)\(\)\s*\{",
            r#"\.command\(\s*["'`]([\w-]+)"#,
            r"(?m)^\s{0,8}([a-z][\w-]*(?:\|[a-z][\w-]*)*)\)\s*(?:#.*)?$",
            r"(?m)^\s{0,8}(-{1,2}[a-z][\w-]*(?:\|-{1,2}[a-z][\w-]*)*)\)",
        ]
    };
    let mut found = Vec::new();
    for pattern in patterns {
        for c in re(pattern).captures_iter(text) {
            for n in c[1].split('|') {
                let n = n.trim_start_matches('-');
                if n.len() >= 3 && !n.starts_with('_') && !found.iter().any(|v| v == n) {
                    found.push(n.to_owned());
                }
            }
        }
    }
    found.truncate(90);
    found
}
pub fn code_header(text: &str, path: &Path, mentions: &[String]) -> String {
    let mut ext = extension(path);
    if ext.is_empty() {
        let shebang = if text.starts_with("#!") {
            text.lines().next().unwrap_or("")
        } else {
            ""
        };
        ext = if shebang.contains("python") {
            ".py"
        } else if re(r"\b(bun|node|deno)\b").is_match(shebang) {
            ".ts"
        } else {
            ".sh"
        }
        .into();
    }
    let mut prose = Vec::new();
    if ext == ".py" {
        let head = text.split('\n').take(200).collect::<Vec<_>>().join("\n");
        if let Some(c) = re("(?m)^[rRuU]?(\"\"\"|''')").captures(&head) {
            let start = c.get(0).unwrap().end();
            if let Some(end) = head[start..].find(&c[1]) {
                prose.push(cut(head[start..start + end].trim(), 1200));
            }
        }
    }
    prose.extend(leading_comments(text));
    prose.extend(
        re(r#"(?:description|help)\s*[=:]\s*['"`]([^'"`]{12,200})['"`]"#)
            .captures_iter(text)
            .take(14)
            .map(|c| c[1].to_owned()),
    );
    let lines: Vec<_> = text.split('\n').take(400).collect();
    if let Some(at) = lines.iter().position(|l| re(r"(?i)usage:").is_match(l)) {
        let mut usage = Vec::new();
        for line in lines.iter().skip(at).take(9) {
            for c in
                re(r#""([^"]{8,400})"|'([^']{8,400})'"#).captures_iter(&line.replace("\\\"", "'"))
            {
                usage.push(
                    c.get(1)
                        .or_else(|| c.get(2))
                        .unwrap()
                        .as_str()
                        .replace("\\n", " ")
                        .trim()
                        .to_owned(),
                );
            }
        }
        prose.extend(usage.into_iter().take(9));
    }
    let names = defined_names(text, &ext);
    if !names.is_empty() {
        prose.push(format!(
            "defines: {}",
            names
                .iter()
                .map(|n| if words(n) == *n {
                    n.clone()
                } else {
                    format!("{n} {}", words(n))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut messages = Vec::new();
    for c in re(r#"(?:raise\s+\w+\(|die\s+|fail\s+|error\(|Error\(|exit\([^'"`\n]*|print\(|console\.(?:log|error)\(|echo\s+(?:-e\s+)?)\s*f?(?:"([^"\n]{25,200})"|'([^'\n]{25,200})')"#).captures_iter(text) {
        let line = c.get(1).or_else(|| c.get(2)).unwrap().as_str().trim().to_owned();
        if line.contains(char::is_whitespace) && !messages.contains(&line) {
            messages.push(line);
        }
        if messages.len() >= 8 {
            break;
        }
    }
    prose.extend(messages);
    let mut constants = Vec::new();
    for c in re(r"(?m)^(?:export\s+)?(?:const\s+)?([A-Z][A-Z0-9_]{3,})\s*[:=]([^\n]{0,240})")
        .captures_iter(text)
    {
        constants.push(c[1].to_lowercase());
        constants.extend(
            re(r#"["']([A-Za-z][\w-]{2,})["']"#)
                .captures_iter(&c[2])
                .take(12)
                .map(|x| x[1].to_owned()),
        );
    }
    constants.truncate(40);
    if !constants.is_empty() {
        prose.push(format!("constants: {}", constants.join(", ")));
    }
    if !path.as_os_str().is_empty() {
        prose.push(format!(
            "file: {} {}",
            words(&path.file_stem().unwrap_or_default().to_string_lossy()),
            name(path)
        ));
    }
    if !mentions.is_empty() {
        prose.push(format!("Mentioned in:\n{}", mentions.join("\n")));
    }
    prose
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .into()
}
pub fn markdown_docs(path: &Path, label: &str, at: f64, decays: bool, source: &str) -> Vec<Doc> {
    let text = limited_read(path);
    if text.is_empty() {
        return vec![];
    }
    let reference = display(path);
    let mut out = Vec::new();
    let (meta, body) = frontmatter(&text);
    if name(path) == "SKILL.md" {
        let n = meta
            .get("name")
            .cloned()
            .unwrap_or_else(|| name(path.parent().unwrap_or(path)));
        let description = meta.get("description").cloned().unwrap_or_default();
        out.push(doc(
            source,
            format!("{reference}#skill"),
            reference.clone(),
            format!("skill {n}: {description}"),
            format!("{n} {description}"),
            at,
            false,
        ));
    }
    for (i, (heading, section)) in markdown_sections(&body).into_iter().enumerate() {
        out.push(doc(
            source,
            format!("{reference}#{i}"),
            reference.clone(),
            if heading.is_empty() {
                label.into()
            } else {
                format!("{label} § {heading}")
            },
            section,
            at,
            decays,
        ));
    }
    out
}
fn toml_docs(path: &Path, at: f64) -> Vec<Doc> {
    let reference = display(path);
    let mut parts = Vec::new();
    let mut heading = name(path);
    let mut body = Vec::new();
    let text = limited_read(path);
    for line in text.split('\n') {
        if re(r"^\s*\[\[?[^\]]+\]\]?\s*$").is_match(line) {
            if !body.join("").trim().is_empty() {
                parts.push((heading.clone(), body.join("\n")));
            }
            heading = line.trim().into();
            body.clear();
        } else {
            body.push(line);
        }
    }
    if !body.join("").trim().is_empty() {
        parts.push((heading, body.join("\n")));
    }
    let mut out = Vec::new();
    for (i, (h, section)) in parts.into_iter().enumerate() {
        for start in (0..ulen(&section).max(1)).step_by(1800) {
            out.push(doc(
                "policy",
                format!("{reference}#{i}.{start}"),
                reference.clone(),
                format!("{} {h}", name(path)),
                slice(&section, start, 1800),
                at,
                false,
            ));
        }
    }
    out
}
fn rules_docs(path: &Path, at: f64) -> Vec<Doc> {
    let Ok(data) = limited_read(path).parse::<toml_edit::Document<String>>() else {
        return vec![];
    };
    let mut out = Vec::new();
    if let Some(rules) = data.get("rule").and_then(|v| v.as_array_of_tables()) {
        for rule in rules {
            let get = |k: &str| rule.get(k).and_then(|v| v.as_str()).unwrap_or("");
            let reference = format!("{}#{}", display(path), get("id"));
            out.push(doc(
                "policy",
                reference.clone(),
                reference,
                format!(
                    "rule {} ({}, {})",
                    get("id"),
                    get("section"),
                    get("placement")
                ),
                format!("{}\nFixes: {}", get("text"), get("fixes")),
                at,
                false,
            ));
        }
    }
    out
}
fn owners_docs(path: &Path, at: f64) -> Vec<Doc> {
    let Ok(data) = limited_read(path).parse::<toml_edit::Document<String>>() else {
        return vec![];
    };
    let mut out = Vec::new();
    let guards: Vec<&dyn toml_edit::TableLike> = match data.get("guard") {
        Some(toml_edit::Item::ArrayOfTables(tables)) => tables
            .iter()
            .map(|t| t as &dyn toml_edit::TableLike)
            .collect(),
        Some(item) => item
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_inline_table())
            .map(|t| t as &dyn toml_edit::TableLike)
            .collect(),
        None => Vec::new(),
    };
    for g in guards {
        let get = |k: &str| g.get(k).and_then(|v| v.as_str()).unwrap_or("");
        let source = path
            .parent()
            .unwrap_or(path)
            .join("hooks")
            .join(get("command"));
        let header = if source.exists() {
            code_header(&limited_read(&source), &source, &[])
        } else {
            String::new()
        };
        let mut events = Vec::new();
        for key in ["claude", "codex"] {
            for v in g.get(key).and_then(|v| v.as_array()).into_iter().flatten() {
                if let Some(e) = v.as_str() {
                    events.push(if key == "codex" {
                        format!("codex {e}")
                    } else {
                        e.into()
                    });
                }
            }
        }
        out.push(doc(
            "policy",
            format!("{}#{}", display(path), get("unit")),
            display(&source),
            format!("hook {} ({})", get("unit"), events.join(", ")),
            format!("{header}\n{} {}", get("claude_absent"), get("codex_absent")),
            at,
            false,
        ));
    }
    out
}
pub(super) fn file_docs(
    path: &Path,
    label: &str,
    mentions: &HashMap<PathBuf, Vec<String>>,
) -> Vec<Doc> {
    let Ok(st) = fs::metadata(path) else {
        return vec![];
    };
    let at = mtime(path);
    let n = name(path);
    let ext = extension(path);
    if n == "rules.toml" {
        return rules_docs(path, at);
    }
    if n == "policy-owners.toml" {
        let mut out = owners_docs(path, at);
        out.extend(toml_docs(path, at));
        return out;
    }
    if [".md", ".txt", ".yml", ".yaml"].contains(&ext.as_str()) {
        return markdown_docs(path, label, at, false, "policy");
    }
    if ext == ".toml" {
        return toml_docs(path, at);
    }
    if is_code(path) && st.len() < 2_000_000 {
        let text = limited_read(path);
        if ext.is_empty() && !text.starts_with("#!") {
            return vec![];
        }
        let said = mentions.get(path).map(Vec::as_slice).unwrap_or(&[]);
        let body = if re(r"/hooks/[^/]+\.sh$").is_match(&path.to_string_lossy()) {
            format!(
                "{}\n{}",
                cut(&text, 4000),
                if said.is_empty() {
                    String::new()
                } else {
                    format!("Mentioned in:\n{}", said.join("\n"))
                }
            )
        } else {
            code_header(&text, path, said)
        };
        return vec![doc(
            "policy",
            display(path),
            display(path),
            label.into(),
            body,
            at,
            false,
        )];
    }
    vec![]
}
pub fn iter_docs() -> Vec<Doc> {
    let north = north_dir();
    let south = env_path("NORTH_GROUNDING_SOUTH", home().join("code/south/main"));
    let north_files = tracked(&north, ".");
    let south_files = tracked(&south, "agents");
    let corpora = vec![
        (north.clone(), north_files.clone()),
        (south.clone(), south_files.clone()),
    ];
    let code = corpora
        .iter()
        .map(|(r, files)| {
            (
                r.clone(),
                files
                    .iter()
                    .filter(|p| {
                        is_code(p)
                            && (!extension(p).is_empty() || p.to_string_lossy().contains("/bin/"))
                    })
                    .cloned()
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    let mentions = repos::mention_index(&code, &corpora);
    let mut out = Vec::new();
    for (repo, files) in corpora {
        let n = repo
            .parent()
            .unwrap_or(&repo)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        for path in files {
            if !fs::symlink_metadata(&path).is_ok_and(|s| s.is_file() && !s.is_symlink()) {
                continue;
            }
            out.extend(file_docs(
                &path,
                &format!(
                    "{n}:{}",
                    path.strip_prefix(&repo).unwrap_or(&path).display()
                ),
                &mentions,
            ));
        }
    }
    for path in entries(&handoffs_dir()) {
        let n = name(&path);
        if !re(r"(?i)(brief|protocol)").is_match(&n)
            || !re(r"\.(md|txt)$").is_match(&n)
            || !path.is_file()
        {
            continue;
        }
        out.extend(markdown_docs(
            &path,
            &format!("handoff {n}"),
            mtime(&path),
            true,
            "policy",
        ));
    }
    out
}
