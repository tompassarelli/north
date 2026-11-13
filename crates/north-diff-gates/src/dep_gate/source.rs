use super::{Dependency, Parsed, matches, string};
use serde_json::Value;

fn unquote(text: &str) -> String {
    percent_encoding::percent_decode_str(text)
        .decode_utf8_lossy()
        .into_owned()
}

fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => (),
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if !absolute => parts.push(part),
            ".." => (),
            _ => parts.push(part),
        }
    }
    format!("{}{}", if absolute { "/" } else { "" }, parts.join("/"))
}

fn internal_path(path: &str, manifest: &str) -> bool {
    let path = unquote(path);
    if path.starts_with(['/', '~']) || path.contains('\\') {
        return false;
    }
    let directory = manifest
        .rsplit_once('/')
        .map_or("", |(directory, _)| directory);
    let resolved = normalize(&if directory.is_empty() {
        path
    } else {
        format!("{directory}/{path}")
    });
    resolved != ".." && !resolved.starts_with("../")
}

fn owned_github(source: &str) -> Parsed<bool> {
    let source = source.strip_prefix("git+").unwrap_or(source);
    if matches(r"\A(?:github:)?tompassarelli/[^/#\s]+(?:#\S+)?\z", source) {
        return Ok(true);
    }
    let source = if let Some(path) = source.strip_prefix("git@github.com:") {
        format!("ssh://git@github.com/{path}")
    } else {
        source.to_owned()
    };
    let Some((scheme, rest)) = source.split_once("://") else {
        return Ok(false);
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest[..end].rsplit('@').next().unwrap_or("");
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(h, p)| (h, Some(p)));
    let port = match port {
        Some("") | None => None,
        Some(p) => Some(p.parse::<u16>().map_err(|_| "ValueError")?),
    };
    let path = rest[end..].split(['?', '#']).next().unwrap_or("");
    let decoded = unquote(path);
    let parts: Vec<_> = decoded.trim_matches('/').split('/').collect();
    Ok(
        ["https", "http", "ssh", "git"].contains(&scheme.to_ascii_lowercase().as_str())
            && host.eq_ignore_ascii_case("github.com")
            && port.is_none_or(|p| [22, 80, 443].contains(&p))
            && parts.len() >= 2
            && parts[0] == "tompassarelli"
            && !parts[1].is_empty()
            && parts.iter().all(|p| *p != "." && *p != ".."),
    )
}

pub(super) fn source_dependency(
    name: &Value,
    source: &Value,
    manifest: &str,
    kind: &str,
) -> Parsed<Dependency> {
    let name = string(name)?.to_owned();
    let source = string(source)?;
    let (identity, allowed) = if kind == "path" {
        (format!("path:{source}"), internal_path(source, manifest))
    } else if let Some(value) = source.strip_prefix("workspace:") {
        let allowed = if value.starts_with(['.', '/', '~']) {
            internal_path(value, manifest)
        } else {
            matches(
                r"\A(?:\*|[~^]|[~^]?\d[^\s]*|@?[^\s:/]+(?:/[^\s:/]+)?@(?:\*|[~^]|[~^]?\d[^\s]*))\z",
                value,
            )
        };
        if !allowed {
            return Err("ValueError");
        }
        (source.to_owned(), true)
    } else if let Some(path) = source.strip_prefix("file:") {
        (
            source.to_owned(),
            internal_path(path, manifest)
                && normalize(&unquote(path)).starts_with("vendor/")
                && !path.starts_with('/'),
        )
    } else if owned_github(source)? {
        (source.to_owned(), true)
    } else if kind == "url"
        || source.contains([':', '/', '\\'])
        || source.starts_with(['.', '~'])
        || [".tgz", ".tar.gz", ".tar", ".zip"]
            .iter()
            .any(|s| source.ends_with(s))
    {
        if source.starts_with("npm:") && matches(r"\Anpm:(?:@[^/\s]+/)?[^@\s]+@[^\s:/]+\z", source)
        {
            ("registry".to_owned(), true)
        } else {
            (source.to_owned(), false)
        }
    } else {
        if !matches(r"\A[A-Za-z0-9*^~<>=|+!., -]+\z", source) {
            return Err("ValueError");
        }
        ("registry".to_owned(), true)
    };
    Ok(Dependency {
        name,
        source: identity,
        allowed,
    })
}
