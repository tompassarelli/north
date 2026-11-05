use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;

use super::{Range, git};

type Declarations = BTreeMap<String, BTreeSet<String>>;

const UNREADABLE: &str = "cannot read committed generated authority";

fn read(directory: &Path, arguments: &[&str]) -> Result<Vec<u8>, String> {
    git::bytes(directory, arguments).ok_or_else(|| UNREADABLE.to_owned())
}

fn declarations(directory: &Path, side: &str) -> Result<Declarations, String> {
    let blob = format!("{side}:.safe-push");
    if git::bytes(directory, &["cat-file", "-e", &blob]).is_none() {
        return Ok(Declarations::new());
    }
    let listing = read(directory, &["config", "--blob", &blob, "--null", "--list"])?;
    let mut records = Declarations::new();
    for entry in listing.split(|byte| *byte == 0) {
        let (key, value) = entry
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or((entry, &[][..]), |at| (&entry[..at], &entry[at + 1..]));
        let key = String::from_utf8_lossy(key);
        let Some(output) = declared_output(&key) else {
            continue;
        };
        let inputs = records.entry(output.to_owned()).or_default();
        if !value.is_empty() {
            inputs.insert(String::from_utf8_lossy(value).into_owned());
        }
    }
    Ok(records)
}

fn declared_output(key: &str) -> Option<&str> {
    const PREFIX: &str = "generated.";
    const SUFFIX: &str = ".input";
    (key.starts_with(PREFIX) && key.ends_with(SUFFIX)).then(|| {
        key.get(PREFIX.len()..key.len() - SUFFIX.len())
            .unwrap_or_default()
    })
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\0', '*', '?', '[', ']'])
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

fn tree(directory: &Path, side: &str) -> Result<HashMap<String, Vec<u8>>, String> {
    let listing = read(directory, &["ls-tree", "-r", "-z", side])?;
    Ok(listing
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let tab = entry.iter().position(|byte| *byte == b'\t')?;
            let oid = entry[..tab].split(|byte| *byte == b' ').nth(2)?;
            Some((
                String::from_utf8_lossy(&entry[tab + 1..]).into_owned(),
                oid.to_vec(),
            ))
        })
        .collect())
}

fn rows(text: &[u8]) -> Vec<&[u8]> {
    let mut rows = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < text.len() {
        match text[index] {
            b'\n' => {
                rows.push(&text[start..index]);
                start = index + 1;
            }
            b'\r' => {
                rows.push(&text[start..index]);
                if text.get(index + 1) == Some(&b'\n') {
                    index += 1;
                }
                start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    if start < text.len() {
        rows.push(&text[start..]);
    }
    rows
}

fn removed_rows(old: &[&[u8]], new: &[&[u8]]) -> usize {
    let mut remaining: HashMap<&[u8], usize> = HashMap::new();
    for row in new {
        *remaining.entry(row).or_default() += 1;
    }
    old.iter()
        .filter(|row| match remaining.get_mut(*row) {
            Some(count) if *count > 0 => {
                *count -= 1;
                false
            }
            _ => true,
        })
        .count()
}

fn judge(directory: &Path, base: &str, head: &str) -> Result<(), String> {
    let (old, new) = (
        declarations(directory, base)?,
        declarations(directory, head)?,
    );
    if old.is_empty() && new.is_empty() {
        return Ok(());
    }
    let (before, after) = (tree(directory, base)?, tree(directory, head)?);
    let tracked = |path: &str| before.contains_key(path) || after.contains_key(path);
    let outputs: BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    for output in outputs {
        let inputs: BTreeSet<&String> = old
            .get(output)
            .into_iter()
            .chain(new.get(output))
            .flatten()
            .collect();
        if !valid_path(output)
            || inputs.is_empty()
            || inputs.contains(output)
            || !tracked(output)
            || old.contains_key(output) && !new.contains_key(output)
            || inputs
                .iter()
                .any(|path| !valid_path(path) || !tracked(path))
        {
            return Err(format!(
                "{output}: missing or invalid generated input authority; declare repeated input = <tracked primary file> in [generated \"{output}\"]"
            ));
        }
        if [&old, &new]
            .iter()
            .any(|side| side.get(output).is_some_and(BTreeSet::is_empty))
        {
            return Err(format!(
                "{output}: enabled generated declaration has no primary inputs"
            ));
        }
        if before.get(output.as_str()) == after.get(output.as_str())
            || !before.contains_key(output.as_str())
        {
            continue;
        }
        let old_text = read(directory, &["show", &format!("{base}:{output}")])?;
        let new_text = if after.contains_key(output.as_str()) {
            read(directory, &["show", &format!("{head}:{output}")])?
        } else {
            Vec::new()
        };
        let old_rows = rows(&old_text);
        let removed = removed_rows(&old_rows, &rows(&new_text));
        let changed = inputs
            .iter()
            .filter(|path| before.get(path.as_str()) != after.get(path.as_str()))
            .count();
        if removed * 2 > old_rows.len() && changed * 20 < inputs.len() {
            return Err(format!(
                "{output}: rewrites {removed}/{} old rows with {changed}/{} tracked primary inputs changed; keep generation proportional or fix its producer",
                old_rows.len(),
                inputs.len()
            ));
        }
    }
    Ok(())
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    match judge(directory, base, head) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(out, "safe-push: ✗ {error}; NOT pushed.");
            1
        }
    }
}
