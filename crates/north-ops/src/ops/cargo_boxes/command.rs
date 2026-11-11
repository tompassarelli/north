use super::workspace::{Package, TestTarget, Workspace};

struct Word {
    start: usize,
    end: usize,
    text: String,
}

fn segments(command: &str) -> Vec<Vec<Word>> {
    let bytes = command.as_bytes();
    let mut segments = vec![Vec::new()];
    let mut word: Option<Word> = None;
    let mut index = 0;
    while index < bytes.len() {
        let separator = separator(bytes, index);
        if bytes[index].is_ascii_whitespace() || separator.is_some() {
            segments.last_mut().into_iter().for_each(|segment| segment.extend(word.take()));
            if separator.is_some() {
                segments.push(Vec::new());
            }
            index += separator.unwrap_or(1);
            continue;
        }
        let current = word.get_or_insert_with(|| Word { start: index, end: index, text: String::new() });
        let (consumed, text) = token(command, index);
        current.text.push_str(&text);
        index += consumed;
        current.end = index;
    }
    segments.last_mut().into_iter().for_each(|segment| segment.extend(word.take()));
    segments.into_iter().filter(|segment| !segment.is_empty()).collect()
}

fn separator(bytes: &[u8], index: usize) -> Option<usize> {
    let doubled = bytes.get(index + 1) == Some(&bytes[index]);
    match bytes[index] {
        b'&' | b'|' if doubled => Some(2),
        b';' | b'|' | b'\n' => Some(1),
        _ => None,
    }
}

fn token(command: &str, start: usize) -> (usize, String) {
    let rest = &command[start..];
    let closing = |open: usize, close: char| rest[open..].find(close).map_or(rest.len(), |at| open + at + close.len_utf8());
    match rest.chars().next() {
        Some('\'') => {
            let end = closing(1, '\'');
            (end, rest[1..end.saturating_sub(1).max(1)].to_owned())
        }
        Some('"') => {
            let end = closing(1, '"');
            (end, rest[1..end.saturating_sub(1).max(1)].to_owned())
        }
        Some('$') if rest[1..].starts_with('(') => {
            let mut depth = 0usize;
            let end = rest
                .char_indices()
                .find_map(|(at, character)| {
                    match character {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => return None,
                    }
                    (depth == 0).then_some(at + 1)
                })
                .unwrap_or(rest.len());
            (end, rest[..end].to_owned())
        }
        Some('\\') => {
            let next = rest[1..].chars().next().map_or(0, char::len_utf8);
            (1 + next, rest[1..1 + next].to_owned())
        }
        Some(character) => (character.len_utf8(), character.to_string()),
        None => (0, String::new()),
    }
}

const SCOPE: [&str; 6] = ["build", "test", "nextest", "clippy", "run", "fmt"];

fn cargo_word(words: &[Word]) -> Option<usize> {
    let texts: Vec<&str> = words.iter().map(|word| word.text.as_str()).collect();
    let mut at = 0;
    while *texts.get(at)? != "cargo" {
        at += wrapper(&texts[at..])?;
    }
    SCOPE.contains(texts.get(at + 1)?).then_some(at)
}

fn wrapper(words: &[&str]) -> Option<usize> {
    let assignment = words[0].split_once('=').is_some_and(|(name, _)| !name.is_empty() && name.chars().all(|character| character.is_ascii_alphanumeric() || character == '_'));
    match words {
        _ if assignment => Some(1),
        ["env" | "time", ..] => Some(1),
        ["timeout", _, ..] => Some(2),
        ["nix", "develop", "-c" | "--command", ..] | ["depbox", "run", "--", ..] => Some(3),
        _ => None,
    }
}

pub fn calls_cargo(command: &str) -> bool {
    segments(command).iter().any(|words| cargo_word(words).is_some())
}

pub fn rewrite(command: &str, workspace: &Workspace) -> Result<String, String> {
    let mut rewritten = String::with_capacity(command.len());
    let mut copied = 0;
    for words in segments(command) {
        let Some(at) = cargo_word(&words) else { continue };
        let arguments: Vec<&str> = words[at + 1..].iter().map(|word| word.text.as_str()).collect();
        let invocation = Invocation::parse(&arguments)?;
        rewritten.push_str(&command[copied..words[at].start]);
        rewritten.push_str(&invocation.buck2(workspace)?);
        copied = words.last().map_or(words[at].end, |word| word.end);
    }
    rewritten.push_str(&command[copied..]);
    Ok(rewritten)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Verb {
    Build,
    Test,
    Nextest,
}

#[derive(Clone, Copy)]
enum Ignored {
    Only,
    All,
}

struct Invocation<'a> {
    verb: Verb,
    packages: Vec<&'a str>,
    workspace: bool,
    test: Option<&'a str>,
    filters: Vec<&'a str>,
    no_run: bool,
    timings: bool,
    release: bool,
    ignored: Option<Ignored>,
    threads: Option<&'a str>,
    stress: Option<&'a str>,
}

fn plain_filter(filter: &str) -> Result<&str, String> {
    let plain = !filter.is_empty() && filter.chars().all(|character| character.is_ascii_alphanumeric() || character == '_' || character == ':');
    if plain { Ok(filter) } else { Err(format!("the test filter {filter:?} is not a plain name or :: path, and libtest matches substrings only")) }
}

fn expression(text: &str) -> Result<Vec<&str>, String> {
    text.split(['+', '|'])
        .map(|term| {
            let inner = term.trim().strip_prefix("test(").and_then(|rest| rest.strip_suffix(')')).ok_or_else(|| format!("the filter term {:?} is not test(NAME) or test(/NAME/)", term.trim()))?;
            let name = inner.strip_prefix('/').and_then(|rest| rest.strip_suffix('/')).unwrap_or(inner);
            plain_filter(name)
        })
        .collect()
}

fn verb<'w, 'a>(arguments: &'w [&'a str]) -> Result<(Verb, &'w [&'a str]), String> {
    match arguments {
        ["build", rest @ ..] => Ok((Verb::Build, rest)),
        ["test", rest @ ..] => Ok((Verb::Test, rest)),
        ["nextest", "run", rest @ ..] => Ok((Verb::Nextest, rest)),
        [verb, ..] => Err(format!("cargo {verb} has no rule for an issue box: north#1115 maps it only for skill and doc lines, or after box 10 names the clippy and rustfmt targets")),
        [] => Err("a bare cargo has no rule".to_owned()),
    }
}

fn value<'a>(flag: &str, words: &mut impl Iterator<Item = &'a str>) -> Result<&'a str, String> {
    words.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn ignored(value: &str) -> Result<Ignored, String> {
    match value {
        "only" => Ok(Ignored::Only),
        "all" => Ok(Ignored::All),
        other => Err(format!("--run-ignored {other} has no rule")),
    }
}

fn number(value: &str, flag: &str) -> Result<String, String> {
    Some(value).filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())).map(str::to_owned).ok_or_else(|| format!("{flag} needs a number"))
}

fn named_test<'w>(packages: &[&'w Package], test: &str) -> Result<Vec<&'w TestTarget>, String> {
    let suffix = format!("-test-{test}");
    let found: Vec<&TestTarget> = packages.iter().flat_map(|package| &package.tests).filter(|target| target.target.ends_with(&suffix)).collect();
    if found.is_empty() {
        return Err(format!("no selected package has the test target {test}"));
    }
    Ok(found)
}

impl<'a> Invocation<'a> {
    fn parse(arguments: &[&'a str]) -> Result<Invocation<'a>, String> {
        let (verb, rest) = verb(arguments)?;
        let mut invocation = Invocation { verb, packages: Vec::new(), workspace: false, test: None, filters: Vec::new(), no_run: false, timings: false, release: false, ignored: None, threads: None, stress: None };
        let mut words = rest.iter().copied();
        while let Some(word) = words.next() {
            invocation.take(word, &mut words)?;
        }
        Ok(invocation)
    }

    fn take(&mut self, word: &'a str, words: &mut impl Iterator<Item = &'a str>) -> Result<(), String> {
        if self.take_common(word, words)? || self.verb == Verb::Nextest && self.take_nextest(word, words)? {
            return Ok(());
        }
        match word {
            flag if flag.starts_with('-') => Err(format!("the flag {flag} has no rule")),
            filter if self.verb != Verb::Build => {
                self.filters.push(plain_filter(filter)?);
                Ok(())
            }
            other => Err(format!("the argument {other} has no rule")),
        }
    }

    fn take_common(&mut self, word: &str, words: &mut impl Iterator<Item = &'a str>) -> Result<bool, String> {
        match (word, self.verb) {
            ("-p" | "--package", _) => self.packages.push(value(word, words)?),
            ("--workspace" | "--all", _) => self.workspace = true,
            ("--locked" | "--offline" | "--frozen", _) => {}
            ("--release", Verb::Build) => self.release = true,
            ("--timings", Verb::Build | Verb::Test) => self.timings = true,
            ("--test", Verb::Test | Verb::Nextest) => self.test = Some(value(word, words)?),
            ("--no-run", Verb::Test) => self.no_run = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn take_nextest(&mut self, word: &str, words: &mut impl Iterator<Item = &'a str>) -> Result<bool, String> {
        match word {
            "-E" | "--filterset" => self.filters.extend(expression(value(word, words)?)?),
            "--run-ignored" => self.ignored = Some(ignored(value(word, words)?)?),
            "--stress-count" => self.stress = Some(value(word, words)?),
            "--test-threads" => self.threads = Some(value(word, words)?),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn selected<'w>(&self, workspace: &'w Workspace) -> Result<Vec<&'w Package>, String> {
        if self.workspace || self.packages.is_empty() {
            return Ok(workspace.packages.values().collect());
        }
        self.packages.iter().map(|name| workspace.package(name)).collect()
    }

    fn whole_workspace(&self) -> bool {
        self.workspace || self.packages.is_empty()
    }


    fn patterns(&self, workspace: &Workspace) -> Result<Vec<String>, String> {
        let packages = self.selected(workspace)?;
        if let Some(test) = self.test {
            return Ok(named_test(&packages, test)?.into_iter().map(|target| target.target.clone()).collect());
        }
        if self.whole_workspace() {
            return Ok(vec!["//...".to_owned()]);
        }
        Ok(packages
            .iter()
            .map(|package| match self.verb {
                Verb::Build => format!("{}:{}", package.label, package.name),
                _ if package.label == "//" => "//:".to_owned(),
                _ => format!("{}/...", package.label),
            })
            .collect())
    }

    fn runs(&self, workspace: &Workspace) -> Result<String, String> {
        let packages = self.selected(workspace)?;
        let candidates: Vec<&TestTarget> = match self.test {
            Some(test) => named_test(&packages, test)?,
            None => packages.iter().flat_map(|package| &package.tests).collect(),
        };
        if candidates.is_empty() {
            return Err("the selected packages have no test targets".to_owned());
        }
        let probes: Vec<&str> = self.filters.iter().filter_map(|filter| filter.split("::").find(|part| !part.is_empty())).collect();
        let mentioning: Vec<&TestTarget> = candidates.iter().copied().filter(|target| probes.iter().any(|probe| target.mentions(probe))).collect();
        let tail = self.libtest_arguments()?;
        let run = |target: &TestTarget| if tail.is_empty() { format!("buck2 run {}", target.target) } else { format!("buck2 run {}{tail}", target.binary()) };
        let chain = if probes.is_empty() || !mentioning.is_empty() {
            let targets = if probes.is_empty() { candidates } else { mentioning };
            targets.into_iter().map(run).collect::<Vec<_>>().join(" && ")
        } else {
            let scope = self.patterns(workspace)?.join(" + ");
            format!("for t in $(buck2 uquery 'filter(\"-(unittest|test-.+)-bin$\", kind(rust_binary, {scope}))'); do buck2 run $t{tail} || exit 1; done")
        };
        Ok(match self.stress {
            Some(count) => format!("for _ in $(seq {}); do {chain} || exit 1; done", number(count, "--stress-count")?),
            None => chain,
        })
    }

    fn libtest_arguments(&self) -> Result<String, String> {
        let mut arguments: Vec<String> = self.filters.iter().map(|filter| (*filter).to_owned()).collect();
        match self.ignored {
            Some(Ignored::Only) => arguments.push("--ignored".to_owned()),
            Some(Ignored::All) => arguments.push("--include-ignored".to_owned()),
            None => {}
        }
        if let Some(threads) = self.threads {
            arguments.extend(["--test-threads".to_owned(), number(threads, "--test-threads")?]);
        }
        Ok(if arguments.is_empty() { String::new() } else { format!(" -- {}", arguments.join(" ")) })
    }

    fn buck2(&self, workspace: &Workspace) -> Result<String, String> {
        let timings = if self.timings { " && buck2 log critical-path" } else { "" };
        if self.verb == Verb::Build || self.no_run {
            let mode = if self.release { " @mode/release" } else { "" };
            return Ok(format!("buck2 build{mode} {}{timings}", self.patterns(workspace)?.join(" ")));
        }
        if self.filters.is_empty() && self.ignored.is_none() && self.threads.is_none() && self.stress.is_none() {
            return Ok(format!("buck2 test {}{timings}", self.patterns(workspace)?.join(" ")));
        }
        if self.timings {
            return Err("--timings with a filtered run has no rule".to_owned());
        }
        self.runs(workspace)
    }
}
