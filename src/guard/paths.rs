use std::path::{Component, Path, PathBuf};

pub fn realpath(path: &Path) -> Option<String> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return None;
    }
    let full = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let mut pending = full;
    let mut links = 0;
    loop {
        let components: Vec<_> = pending.components().collect();
        let mut out = PathBuf::new();
        let mut restart = None;
        for (index, part) in components.iter().enumerate() {
            match part {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                _ => out.push(part.as_os_str()),
            }
            if let Ok(dest) = std::fs::read_link(&out) {
                links += 1;
                if links > 40 {
                    continue;
                }
                out.pop();
                out.push(dest);
                for rest in &components[index + 1..] {
                    out.push(rest.as_os_str());
                }
                restart = Some(out.clone());
                break;
            }
        }
        if let Some(next) = restart {
            pending = next;
        } else {
            return Some(out.to_string_lossy().into_owned());
        }
    }
}

pub fn expanduser(word: &str, home: &str) -> String {
    if word == "~" {
        return home.to_owned();
    }
    if let Some(tail) = word.strip_prefix("~/") {
        return format!("{home}/{tail}");
    }
    if let Some(user) = word.strip_prefix('~') {
        let (name, tail) = user.split_once('/').unwrap_or((user, ""));
        if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
            for row in passwd.lines() {
                let fields: Vec<_> = row.split(':').collect();
                if fields.first() == Some(&name) && fields.len() > 5 {
                    return if tail.is_empty() {
                        fields[5].into()
                    } else {
                        format!("{}/{tail}", fields[5])
                    };
                }
            }
        }
    }
    word.into()
}

pub fn expand_home(raw: &str, home: &str) -> String {
    for prefix in ["${HOME}", "$HOME"] {
        if let Some(tail) = raw.strip_prefix(prefix)
            && (tail.is_empty() || tail.starts_with('/'))
        {
            return expanduser(&format!("{home}{tail}"), home);
        }
    }
    expanduser(raw, home)
}

pub fn normpath(path: &str) -> String {
    if path.is_empty() {
        return ".".into();
    }
    let initial = if path.starts_with("//") && !path.starts_with("///") {
        2
    } else {
        usize::from(path.starts_with('/'))
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part != ".."
            || (initial == 0 && parts.is_empty())
            || parts.last() == Some(&"..")
        {
            parts.push(part);
        } else if !parts.is_empty() {
            parts.pop();
        }
    }
    let joined = format!("{}{}", "/".repeat(initial), parts.join("/"));
    if joined.is_empty() { ".".into() } else { joined }
}

pub fn join(base: &str, tail: &str) -> String {
    if tail.starts_with('/') || base.is_empty() {
        tail.to_owned()
    } else if base.ends_with('/') {
        format!("{base}{tail}")
    } else {
        format!("{base}/{tail}")
    }
}

pub fn basename(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

pub fn dirname(path: &str) -> &str {
    let Some(split) = path.rfind('/') else {
        return "";
    };
    let head = &path[..=split];
    let trimmed = head.trim_end_matches('/');
    if trimmed.is_empty() { head } else { trimmed }
}
