pub(super) const COMMANDS: [&str; 7] = ["search", "show", "session", "status", "index", "compress", "restore"];
const CLOSED_AFTER_HOURS: f64 = 48.0;

const VALUE_OPTIONS: [&str; 16] = [
    "-n", "--limit", "-p", "--project", "-r", "--role", "-t", "--tool", "--since", "--until", "-s", "--session", "--provider", "--color", "-C", "--older-than",
];

pub(super) const HELP: &str = "usage: north convo [-h] [-x] [-a] [-r ROLE] [-t NAME] [-p NAME] [-s ID] [--provider {claude,codex}]
             [--since WHEN] [--until WHEN] [-m] [-n LIMIT] [--full] [-C N] [--json] [-u] [-q] [-v]
             [--color {auto,always,never}] [--dry-run] [--older-than HOURS] [--max-files N]
             [query ...]

find past agent work across Claude Code and Codex transcripts

positional arguments:
  query                 search terms, or the id for show/session

options:
  -x, --exact           literal string
  -a, --all-terms       no partial-match fallback
  -r, --role ROLE       user|assistant|thinking|tool|system, comma-separated
  -t, --tool NAME       tool calls of one tool (Bash, Skill…)
  -p, --project NAME    project name substring
  -s, --session ID      session or subagent id prefix
  --provider {claude,codex}
  --since WHEN          12h, 3d, 2w, 6m, 1y or YYYY-MM-DD
  --until WHEN          same forms as --since
  -m, --messages        list messages, not sessions
  -n, --limit LIMIT     results (10 sessions or 20 messages)
  --full                whole message text; with index: rebuild from scratch
  -C, --context N       show: neighbouring messages (default 2)
  --json
  -u, --no-update       skip the refresh; a miss is then inconclusive
  -q, --quiet
  -v, --verbose
  --color {auto,always,never}
  --dry-run             compress: report only
  --older-than HOURS    compress: only transcripts untouched this long
  --max-files N         compress: only the N largest eligible files

commands:
  north convo [search] TERMS   sessions ranked by relevance (default command)
  north convo show ID…        one message in full, with -C neighbours
  north convo session ID      a session's transcripts, its first ask, and mentions
  north convo status | index [--full] | compress [--dry-run] | restore FILE…

query:
  words must all appear in one message; a token with punctuation
  (c289-ko-bottom, tools/x.ts) is matched as a phrase; \"quoted phrase\",
  AND/OR/NOT, ( ) and prefix* work. Stemmed: test matches tests.
  A message holding the words as typed, adjacent and in order, ranks higher.
  When fewer than three sessions have a message with every word, sessions
  covering the most words follow, marked partial (-a disables this).
  -x matches the literal string.

output:
  one entry per session: id prefix, best-hit time, project, title, hit count,
  then the best message's role, snippet and [id]. -m lists messages instead.
  --json keys: session agent title project provider id ts role tool snippet
  path line hits partial (text with --full; show --json always has text).
  Times are UTC.

exit: 0 found · 1 conclusive miss · 2 inconclusive (index busy, root
missing, -u) or bad usage. Calls to convo itself are not results; the
current session (CLAUDE_CODE_SESSION_ID, CODEX_THREAD_ID) ranks last.

examples:
  north convo stage backdrop contrast         where was this discussed?
  north convo -r user --since 3d tests        what did the human say?
  north convo -t Bash -x 'wisp pad --helper'  which exact command ran?
  north convo -r user --since 1d              newest human messages, no query
  north convo show 4276825 -C 3               read a hit with context
";

#[derive(Default)]
pub(super) struct Args {
    pub command: String,
    pub query: Vec<String>,
    pub exact: bool,
    pub all_terms: bool,
    pub role: Option<String>,
    pub tool: Option<String>,
    pub project: Option<String>,
    pub session: Option<String>,
    pub provider: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub messages: bool,
    pub limit: i64,
    pub full: bool,
    pub context: i64,
    pub json: bool,
    pub no_update: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub color: String,
    pub dry_run: bool,
    pub older_than: f64,
    pub max_files: usize,
    pub help: bool,
}

impl Args {
    pub(super) fn has_filter(&self) -> bool {
        [&self.role, &self.tool, &self.project, &self.session, &self.since, &self.until, &self.provider].iter().any(|filter| filter.is_some())
    }
}

fn command_position(arguments: &[String]) -> Option<usize> {
    let mut index = 0;
    while index < arguments.len() {
        let argument = &arguments[index];
        if VALUE_OPTIONS.contains(&argument.as_str()) {
            index += 2;
        } else if argument.starts_with('-') {
            index += 1;
        } else {
            return COMMANDS.contains(&argument.as_str()).then_some(index);
        }
    }
    None
}

fn number<T: std::str::FromStr>(flag: &str, value: &str) -> Result<T, String> {
    value.trim().parse().map_err(|_| format!("argument {flag}: invalid value: {value:?}"))
}

fn choice(flag: &str, value: String, allowed: &[&str]) -> Result<String, String> {
    if allowed.contains(&value.as_str()) {
        return Ok(value);
    }
    let listed = allowed.iter().map(|name| format!("'{name}'")).collect::<Vec<_>>().join(", ");
    Err(format!("argument {flag}: invalid choice: {value:?} (choose from {listed})"))
}

fn set(args: &mut Args, flag: &str, value: String) -> Result<(), String> {
    match flag {
        "-r" | "--role" => args.role = Some(value),
        "-t" | "--tool" => args.tool = Some(value),
        "-p" | "--project" => args.project = Some(value),
        "-s" | "--session" => args.session = Some(value),
        "--provider" => args.provider = Some(choice(flag, value, &["claude", "codex"])?),
        "--since" => args.since = Some(value),
        "--until" => args.until = Some(value),
        "-n" | "--limit" => args.limit = number(flag, &value)?,
        "-C" | "--context" => args.context = number(flag, &value)?,
        "--older-than" => args.older_than = number(flag, &value)?,
        "--max-files" => args.max_files = number(flag, &value)?,
        "--color" => args.color = choice(flag, value, &["auto", "always", "never"])?,
        _ => return Err(format!("unrecognized arguments: {flag}")),
    }
    Ok(())
}

fn switch(args: &mut Args, flag: &str) -> bool {
    match flag {
        "-x" | "--exact" => args.exact = true,
        "-a" | "--all-terms" => args.all_terms = true,
        "-m" | "--messages" => args.messages = true,
        "--full" => args.full = true,
        "--json" => args.json = true,
        "-u" | "--no-update" => args.no_update = true,
        "-q" | "--quiet" => args.quiet = true,
        "-v" | "--verbose" => args.verbose = true,
        "--dry-run" => args.dry_run = true,
        "-h" | "--help" => args.help = true,
        _ => return false,
    }
    true
}

pub(super) fn parse(arguments: &[String]) -> Result<Args, String> {
    let mut rest = arguments.to_vec();
    let command = command_position(&rest).map(|at| rest.remove(at));
    let mut args = Args { command: command.unwrap_or_else(|| "search".to_owned()), context: 2, color: "auto".to_owned(), older_than: CLOSED_AFTER_HOURS, ..Args::default() };
    let mut iter = rest.into_iter();
    let mut only_positional = false;
    while let Some(argument) = iter.next() {
        if only_positional || argument == "-" || !argument.starts_with('-') {
            args.query.push(argument);
        } else if argument == "--" {
            only_positional = true;
        } else if let Some((flag, value)) = argument.split_once('=').filter(|_| argument.starts_with("--")) {
            set(&mut args, flag, value.to_owned())?;
        } else if VALUE_OPTIONS.contains(&argument.as_str()) {
            let value = iter.next().ok_or_else(|| format!("argument {argument}: expected one argument"))?;
            set(&mut args, &argument, value)?;
        } else if switch(&mut args, &argument) {
        } else if !argument.starts_with("--") && argument.len() > 2 {
            cluster(&mut args, &argument, &mut iter)?;
        } else {
            return Err(format!("unrecognized arguments: {argument}"));
        }
    }
    if matches!(args.command.as_str(), "show" | "session" | "restore") && args.query.is_empty() && !args.help {
        return Err(format!("{} needs an argument", args.command));
    }
    Ok(args)
}

fn cluster(args: &mut Args, argument: &str, iter: &mut impl Iterator<Item = String>) -> Result<(), String> {
    let chars: Vec<char> = argument.chars().skip(1).collect();
    for (index, flag) in chars.iter().enumerate() {
        let name = format!("-{flag}");
        if VALUE_OPTIONS.contains(&name.as_str()) {
            let attached: String = chars[index + 1..].iter().collect();
            let value = if attached.is_empty() { iter.next().ok_or_else(|| format!("argument {name}: expected one argument"))? } else { attached };
            return set(args, &name, value);
        }
        if !switch(args, &name) {
            return Err(format!("unrecognized arguments: {argument}"));
        }
    }
    Ok(())
}
