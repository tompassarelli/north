const GLOBAL: &[&str] = &["-H", "-P", "-O0", "-O1", "-O2", "-O3", "-L"];
const FLAG_OPTIONS: &[&str] = &["-i", "-H", "-L", "-0", "-p", "--one-file-system"];

fn extension(pattern: &str) -> Option<&str> {
    let ext = pattern.strip_prefix("*.")?;
    (!ext.is_empty()
        && ext
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'-')))
    .then_some(ext)
}

fn glob_meta(part: &str) -> bool {
    part.contains(['*', '?', '['])
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

fn name_terms(kind: &str, pattern: &str, options: &mut Vec<String>) -> Option<Option<String>> {
    if matches!(kind, "-name" | "-iname") {
        if pattern.contains('/') {
            return None;
        }
        if kind == "-iname" {
            options.push("-i".into());
        }
        if pattern.starts_with('.') {
            options.push("-H".into());
        }
        if let Some(ext) = extension(pattern) {
            options.extend(["-e".into(), ext.into()]);
            return Some(None);
        }
        options.push("-g".into());
        return Some(Some(pattern.to_owned()));
    }
    let parts: Vec<&str> = pattern.split('/').collect();
    if parts.len() < 2 || parts[0] != "*" {
        return None;
    }
    let middle = &parts[1..parts.len() - 1];
    if middle.iter().any(|part| part.is_empty() || glob_meta(part)) {
        return None;
    }
    let last = parts[parts.len() - 1];
    let last = if last == "*" {
        "**".to_owned()
    } else if glob_meta(last) {
        format!("**/{last}")
    } else if last.is_empty() {
        return None;
    } else {
        last.to_owned()
    };
    if kind == "-ipath" {
        options.push("-i".into());
    }
    options.extend(["-p".into(), "-g".into()]);
    let mut joined = vec!["**"];
    joined.extend(middle);
    joined.push(&last);
    Some(Some(joined.join("/")))
}

struct Expression<'a> {
    tokens: &'a [String],
    position: usize,
}

impl Expression<'_> {
    fn take(&mut self) -> Option<&str> {
        let value = self.tokens.get(self.position)?;
        self.position += 1;
        Some(value)
    }

    fn peek_is_or(&self) -> bool {
        self.tokens
            .get(self.position)
            .is_some_and(|t| t == "-o" || t == "-or")
    }
}

#[derive(Default)]
struct Plan {
    options: Vec<String>,
    searching: bool,
    max_depth: Option<String>,
    pattern: Option<String>,
    extensions: Vec<String>,
    top_level_or: bool,
    executor: Vec<String>,
}

impl Plan {
    fn extension_only(&mut self, kind: &str, value: &str) -> Option<()> {
        if kind != "-name" {
            return None;
        }
        self.extensions.push(extension(value)?.to_owned());
        self.searching = true;
        Some(())
    }

    fn group(&mut self, expression: &mut Expression) -> Option<()> {
        let rest = &expression.tokens[expression.position..];
        let closing = rest.iter().position(|t| t == ")")?;
        let group = &rest[..closing];
        expression.position += closing + 1;
        if group.len() < 5 || !(group.len() + 1).is_multiple_of(3) {
            return None;
        }
        for start in (0..group.len()).step_by(3) {
            self.extension_only(&group[start], &group[start + 1])?;
            if start + 2 < group.len() && group[start + 2] != "-o" && group[start + 2] != "-or" {
                return None;
            }
        }
        Some(())
    }

    fn name(&mut self, token: &str, expression: &mut Expression) -> Option<()> {
        let kind = if token == "-wholename" {
            "-path"
        } else {
            token
        };
        let value = expression.take()?.to_owned();
        if expression.peek_is_or() {
            self.extension_only(kind, &value)?;
            expression.position += 1;
            self.top_level_or = true;
            return Some(());
        }
        if self.top_level_or {
            return self.extension_only(kind, &value);
        }
        if self.pattern.is_some() {
            return None;
        }
        if let Some(translated) = name_terms(kind, &value, &mut self.options)? {
            self.pattern = Some(translated);
        }
        self.searching = true;
        Some(())
    }

    fn exec(&mut self, expression: &mut Expression) -> Option<()> {
        let mut words = Vec::new();
        let terminator = loop {
            let word = expression.take()?;
            if word == ";" || word == "+" {
                break word.to_owned();
            }
            words.push(word.to_owned());
        };
        if words.is_empty() || !self.executor.is_empty() {
            return None;
        }
        if terminator == "+" {
            if words.last()? != "{}" {
                return None;
            }
            words.pop();
            self.executor.push("-X".into());
        } else {
            self.executor.push("-x".into());
        }
        self.executor.extend(words);
        Some(())
    }

    fn kinds(&mut self, expression: &mut Expression) -> Option<()> {
        for kind in expression.take()?.split(',') {
            if !matches!(kind, "f" | "d" | "l" | "s" | "p") {
                return None;
            }
            self.options.extend(["-t".into(), kind.into()]);
        }
        self.searching = true;
        Some(())
    }

    fn depth(&mut self, token: &str, expression: &mut Expression) -> Option<()> {
        let value = expression.take()?.to_owned();
        if !digits(&value) {
            return None;
        }
        if token == "-maxdepth" {
            let trimmed = value.trim_start_matches('0');
            self.max_depth = Some(if trimmed.is_empty() {
                "0".into()
            } else {
                trimmed.to_owned()
            });
        } else {
            self.options.extend(["--min-depth".into(), value]);
        }
        Some(())
    }

    fn flag(&mut self, token: &str) -> Option<()> {
        match token {
            "-empty" => {
                self.options.extend(["-t".into(), "e".into()]);
                self.searching = true;
            }
            "-xdev" | "-mount" => self.options.push("--one-file-system".into()),
            "-print0" => self.options.push("-0".into()),
            "-follow" => self.options.push("-L".into()),
            "-print" | "-a" | "-and" => {}
            _ => return None,
        }
        Some(())
    }

    fn token(&mut self, token: &str, expression: &mut Expression) -> Option<()> {
        match token {
            "(" => self.group(expression),
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" => self.name(token, expression),
            "-type" => self.kinds(expression),
            "-maxdepth" | "-mindepth" => self.depth(token, expression),
            "-exec" => self.exec(expression),
            _ => self.flag(token),
        }
    }
}

pub(super) fn to_fd(arguments: &[String]) -> Option<Vec<String>> {
    let mut index = 0;
    let mut plan = Plan::default();
    while index < arguments.len() && GLOBAL.contains(&arguments[index].as_str()) {
        if arguments[index] == "-L" {
            plan.options.push("-L".into());
        }
        index += 1;
    }
    let mut roots = Vec::new();
    while index < arguments.len()
        && !(arguments[index].starts_with('-')
            || matches!(arguments[index].as_str(), "(" | "!" | ")"))
    {
        roots.push(arguments[index].clone());
        index += 1;
    }
    let mut expression = Expression {
        tokens: &arguments[index..],
        position: 0,
    };
    while let Some(token) = expression.take().map(str::to_owned) {
        plan.token(&token, &mut expression)?;
    }
    if plan
        .max_depth
        .as_deref()
        .is_some_and(|depth| matches!(depth, "0" | "1"))
    {
        return None;
    }
    if plan.top_level_or
        && (plan.pattern.is_some()
            || !plan.executor.is_empty()
            || plan.options.iter().any(|o| o == "-t" || o == "-g"))
    {
        return None;
    }
    if !plan.searching {
        return None;
    }
    Some(plan.words(roots))
}

impl Plan {
    fn words(mut self, roots: Vec<String>) -> Vec<String> {
        if let Some(depth) = self.max_depth {
            self.options.extend(["-d".into(), depth]);
        }
        for ext in &self.extensions {
            self.options.extend(["-e".into(), ext.clone()]);
        }
        let mut words = vec!["fd".to_owned()];
        let mut seen: Vec<&str> = Vec::new();
        for option in &self.options {
            if FLAG_OPTIONS.contains(&option.as_str()) {
                if seen.contains(&option.as_str()) {
                    continue;
                }
                seen.push(option);
            }
            words.push(option.clone());
        }
        let glob = words.iter().position(|w| w == "-g");
        if let Some(at) = glob {
            words.remove(at);
        }
        let paths: Vec<String> = if roots == ["."] { Vec::new() } else { roots };
        match (glob, &self.pattern) {
            (Some(_), pattern) => {
                words.push("-g".into());
                words.push(pattern.clone().unwrap_or_default());
            }
            (None, Some(pattern)) => words.push(pattern.clone()),
            (None, None) if !paths.is_empty() => words.push(".".into()),
            _ => {}
        }
        words.extend(paths);
        words.extend(self.executor);
        words
    }
}
