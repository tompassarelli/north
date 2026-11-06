use super::shared_build::runs_head_north;

pub const MARK: &str = "land batch: check ";
const KILLED: i32 = 125;

pub fn joins(command: &str) -> bool {
    runs_head_north(command) && !command.contains("land-tests") && !command.contains("cargo ")
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

pub fn script(checks: &[(&str, u64)]) -> String {
    let mut script = String::from("export LC_ALL=C\nd=$(mktemp -d)\ntrap 'rm -rf \"$d\"' EXIT\nrun() { local s=$EPOCHREALTIME rc=0; timeout -k 5 \"$2\" bash -euo pipefail -c \"$3\" >\"$d/$1\" 2>&1 </dev/null || rc=$?; printf '%s %s %s\\n' \"$rc\" \"$s\" \"$EPOCHREALTIME\" >\"$d/$1.rc\"; }\n");
    for (index, (command, timeout)) in checks.iter().enumerate() {
        script.push_str(&format!("run {index} {timeout} {} &\n", quote(command)));
    }
    script.push_str(&format!("wait\nfor i in $(seq 0 {}); do\n  if [ -f \"$d/$i.rc\" ]; then read -r rc s e <\"$d/$i.rc\"; else rc={KILLED} s=0 e=0; fi\n  printf '{MARK}%s exit %s from %s to %s\\n' \"$i\" \"$rc\" \"$s\" \"$e\"\n  cat \"$d/$i\" 2>/dev/null || true\ndone\n", checks.len().saturating_sub(1)));
    script
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub status: i32,
    pub offset: f64,
    pub seconds: f64,
    pub text: String,
}

fn header(line: &str) -> Option<(usize, i32, f64, f64)> {
    let rest = line.trim().strip_prefix(MARK)?;
    let words: Vec<&str> = rest.split_whitespace().collect();
    let [index, "exit", status, "from", start, "to", end] = words.as_slice() else { return None };
    Some((index.parse().ok()?, status.parse().ok()?, start.parse().ok()?, end.parse().ok()?))
}

pub fn split(log: &str, count: usize) -> Vec<Option<Part>> {
    let mut parts: Vec<Option<Part>> = vec![None; count];
    let mut current: Option<usize> = None;
    let mut first = f64::INFINITY;
    for line in log.lines() {
        if let Some((index, status, start, end)) = header(line).filter(|header| header.0 < count) {
            if start > 0.0 {
                first = first.min(start);
            }
            parts[index] = Some(Part { status, offset: start, seconds: (end - start).max(0.0), text: String::new() });
            current = Some(index);
        } else if let Some(part) = current.and_then(|index| parts[index].as_mut()) {
            part.text.push_str(line);
            part.text.push('\n');
        }
    }
    for part in parts.iter_mut().flatten() {
        part.offset = if part.offset > 0.0 && first.is_finite() { part.offset - first } else { 0.0 };
    }
    parts
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
