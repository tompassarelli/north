use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Range, git};

const CORE_TOOLS: [&str; 4] = ["agents", "threads", "safe-push", "capacity-watchdog"];

struct Layout {
    own: &'static str,
    peer: PathBuf,
    peer_bin: &'static str,
}

fn layout(directory: &Path, home: &Path) -> Option<Layout> {
    let url = git::text(directory, &["remote", "get-url", "--push", "origin"])?;
    let url = url.strip_suffix('\n').unwrap_or(&url);
    let url = url.strip_suffix(".git").unwrap_or(url);
    if url.ends_with("/north") {
        Some(Layout {
            own: "bin",
            peer: home.join("code/nixos-config/main"),
            peer_bin: "dotfiles/bin",
        })
    } else if url.ends_with("/firn") {
        Some(Layout {
            own: "dotfiles/bin",
            peer: home.join("code/north/main"),
            peer_bin: "bin",
        })
    } else {
        None
    }
}

fn tracked_executable(directory: &Path, rev: &str, path: &str) -> bool {
    let listing = git::bytes(directory, &["ls-tree", rev, "--", path]).unwrap_or_default();
    listing.starts_with(b"100755 ") || listing.starts_with(b"120000 ")
}

fn removes_tool_trailers(messages: &str) -> Vec<&str> {
    messages
        .lines()
        .filter_map(|line| line.strip_prefix("Removes-tool:"))
        .map(|rest| {
            rest.trim_start()
                .split(char::is_whitespace)
                .next()
                .unwrap_or_default()
        })
        .collect()
}

fn removed_executables(directory: &Path, from: &str, head: &str, own: &str) -> Vec<String> {
    let raw = git::bytes(
        directory,
        &[
            "diff",
            "--raw",
            "-z",
            "--no-renames",
            "--diff-filter=D",
            from,
            head,
            "--",
            &format!("{own}/"),
        ],
    )
    .unwrap_or_default();
    let fields: Vec<&[u8]> = raw.split(|byte| *byte == 0).collect();
    fields
        .chunks_exact(2)
        .filter(|entry| entry[0].starts_with(b":100755 ") || entry[0].starts_with(b":120000 "))
        .map(|entry| String::from_utf8_lossy(entry[1]).into_owned())
        .collect()
}

pub fn scan(directory: &Path, home: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let Some(Layout {
        own,
        peer,
        peer_bin,
    }) = layout(directory, home)
    else {
        return 0;
    };
    let messages = git::messages(directory, base, head);
    let trailers = removes_tool_trailers(&messages);
    let from = git::merge_base(directory, base, head);
    let mut refused = false;
    for path in removed_executables(directory, &from, head, own) {
        let name = path.strip_prefix(&format!("{own}/")).unwrap_or(&path);
        if name.contains('/') || name.contains(".test.") {
            continue;
        }
        if tracked_executable(directory, head, &format!("{own}/{name}"))
            || tracked_executable(
                &peer,
                "refs/remotes/origin/main",
                &format!("{peer_bin}/{name}"),
            )
        {
            continue;
        }
        let line = if CORE_TOOLS.contains(&name) {
            format!(
                "safe-push: ✗ removes core tool {name} with no same-name replacement in {own}/ or {peer_bin}/ — a core tool cannot be removed; NOT pushed."
            )
        } else if trailers.contains(&name) {
            continue;
        } else {
            format!(
                "safe-push: ✗ removes tool {name} from {own}/ with no same-name replacement — add 'Removes-tool: {name}' to the commit message if intended; NOT pushed."
            )
        };
        let _ = writeln!(out, "{line}");
        refused = true;
    }
    u8::from(refused)
}
