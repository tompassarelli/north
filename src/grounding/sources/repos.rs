use super::*;
use std::collections::HashMap;
pub fn owned_mains(code: &Path) -> Vec<PathBuf> {
    let owner = env::var("NORTH_GROUNDING_OWNER").unwrap_or_else(|_| "tompassarelli".into());
    entries(code)
        .into_iter()
        .map(|p| p.join("main"))
        .filter(|p| {
            p.join(".git").exists()
                && command(
                    "git",
                    &["-C", &p.to_string_lossy(), "remote", "get-url", "origin"],
                )
                .is_ok_and(|s| s.contains(&owner))
        })
        .collect()
}
pub fn mention_index(
    code: &[(PathBuf, Vec<PathBuf>)],
    corpora: &[(PathBuf, Vec<PathBuf>)],
) -> HashMap<PathBuf, Vec<String>> {
    let mut owners: HashMap<String, Option<PathBuf>> = HashMap::new();
    let mut claim = |key: String, path: &Path| {
        let value = owners.entry(key).or_insert_with(|| Some(path.to_owned()));
        if value.as_deref() != Some(path) {
            *value = None;
        }
    };
    for (root, files) in code {
        for path in files {
            let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            let parts: Vec<_> = rel.split('/').collect();
            for i in 0..parts.len().saturating_sub(1) {
                claim(parts[i..].join("/"), path);
            }
            let base = parts.last().copied().unwrap_or("");
            if base.len() >= 5 && re(r"[-_.]").is_match(base) {
                claim(base.into(), path);
            } else if base.len() >= 3 && parts.contains(&"bin") {
                claim(format!("`{base}`"), path);
            }
        }
    }
    let mut found: HashMap<PathBuf, Vec<(String, bool, usize, usize)>> = HashMap::new();
    let mut order = 0;
    for (root, files) in corpora {
        for path in files {
            if !path.extension().is_some_and(|e| e == "md" || e == "toml") {
                continue;
            }
            let text = cut(&read(path), 262144);
            let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
            let priority = if re(r"(^|/)skills/").is_match(&rel) {
                0
            } else if re(r"rules?[^/]*\.toml$|AGENTS\.md$").is_match(&rel) {
                1
            } else if re(r"(^|/)docs/").is_match(&rel) {
                2
            } else {
                3
            };
            let label = if let Some(c) = re(r"(^|/)skills/([^/]+)/").captures(&rel) {
                format!("skill {}", &c[2])
            } else {
                rel.split('/')
                    .rev()
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect::<Vec<_>>()
                    .join("/")
            };
            for raw in text.split('\n') {
                let line = re(r"^[-*>\s]+").replace(raw.trim(), "").into_owned();
                if ulen(&line) < 20 || line.starts_with("```") {
                    continue;
                }
                let prose =
                    re(r"[A-Za-z0-9_.~$@/\-]+").replace_all(&line, |c: &regex::Captures<'_>| {
                        if c[0].contains(['/', '.']) {
                            " ".to_owned()
                        } else {
                            c[0].to_owned()
                        }
                    });
                if re(r"[A-Za-z]{3,}").find_iter(&prose).count() < 4 {
                    continue;
                }
                order += 1;
                let mut seen = std::collections::HashSet::new();
                for m in re(r"[A-Za-z0-9_.~$@/\-]+").find_iter(&line) {
                    let raw = m.as_str().trim_end_matches(['.', ',', ':', ';', ')', '-']);
                    let token = re(r"^(~|\.|\$[A-Z_]+|/home/[^/]+)/").replace(raw, "");
                    let mut key = None;
                    let mut full = false;
                    if token.contains('/') {
                        let parts: Vec<_> = token.split('/').collect();
                        for i in 0..parts.len().saturating_sub(1) {
                            let suffix = parts[i..].join("/");
                            if owners.get(&suffix).is_some_and(Option::is_some) {
                                key = Some(suffix);
                                full = true;
                                break;
                            }
                        }
                    } else if owners.get(token.as_ref()).is_some_and(Option::is_some) {
                        key = Some(token.to_string());
                    } else {
                        let quoted = format!("`{token}`");
                        if owners.get(&quoted).is_some_and(Option::is_some)
                            && line.contains(&quoted)
                        {
                            key = Some(quoted);
                        }
                    }
                    let Some(owner) = key.and_then(|k| owners.get(&k)).and_then(Option::as_ref)
                    else {
                        continue;
                    };
                    if !seen.insert(owner.clone()) {
                        continue;
                    }
                    let start = if ulen(&line) > 200 {
                        ulen(&line[..m.start()])
                            .saturating_sub(80)
                            .min(ulen(&line) - 200)
                    } else {
                        0
                    };
                    found.entry(owner.clone()).or_default().push((
                        format!("{label}: {}", slice(&line, start, 200)),
                        full,
                        priority,
                        order,
                    ));
                }
            }
        }
    }
    found
        .into_iter()
        .map(|(owner, mut all)| {
            all.sort_by_key(|(_, full, priority, order)| (!*full, *priority, *order));
            let mut picked = Vec::new();
            let mut per_label = HashMap::<String, usize>::new();
            for (text, _, _, _) in all {
                let label = text.split(": ").next().unwrap_or("").to_owned();
                if picked.contains(&text)
                    || picked.len() >= 5
                    || per_label.get(&label).copied().unwrap_or(0) >= 2
                {
                    continue;
                }
                *per_label.entry(label).or_default() += 1;
                picked.push(text);
            }
            (owner, picked)
        })
        .collect()
}
