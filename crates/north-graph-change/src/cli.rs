use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use north_coverage::hunks::Hunks;
use north_coverage::run_selected::{git, repository};
use north_graph::{Graph, default_path};
use serde_json::{Value, json};

use crate::blame::{self, Failure, Suspect};
use crate::carry::{self, Pass};
use crate::cone::{self, Cone};
use crate::facts::{Facts, TestId};
use crate::lane::{self, Change, Lane};
use crate::overlap;
use crate::shadow;

pub const VERBS: [&str; 3] = ["affected", "overlap", "blame"];
const UNTRACED: u8 = 3;
const PATIENCE: Duration = Duration::from_secs(5);
const USAGE: &str = "usage: north graph ask affected [--db PATH] [--repo DIR] [--json | --nextest] [--lane NAME=BASE..HEAD]... [--ran NAME=BASE..HEAD]... BASE HEAD
       north graph ask overlap [--db PATH] [--repo DIR] [--json] NAME=BASE..HEAD...
       north graph ask blame [--db PATH] [--repo DIR] [--json] [--failed \"BINARY TEST\"]... [--compile FILE]... [--log FILE]... NAME=BASE..HEAD...";

#[derive(Default)]
struct Options {
    db: Option<PathBuf>,
    repo: Option<PathBuf>,
    json: bool,
    nextest: bool,
    failed: Vec<String>,
    compile: Vec<String>,
    logs: Vec<PathBuf>,
    lanes: Vec<String>,
    ran: Vec<String>,
    positional: Vec<String>,
}

fn options(arguments: &[String]) -> Result<Options, String> {
    let mut parsed = Options::default();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        let mut value = |flag: &str| rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match argument.as_str() {
            "--db" => parsed.db = Some(PathBuf::from(value("--db")?)),
            "--repo" => parsed.repo = Some(PathBuf::from(value("--repo")?)),
            "--json" => parsed.json = true,
            "--nextest" => parsed.nextest = true,
            "--failed" => parsed.failed.push(value("--failed")?),
            "--compile" => parsed.compile.push(value("--compile")?),
            "--log" => parsed.logs.push(PathBuf::from(value("--log")?)),
            "--lane" => parsed.lanes.push(value("--lane")?),
            "--ran" => parsed.ran.push(value("--ran")?),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => parsed.positional.push(argument.clone()),
        }
    }
    Ok(parsed)
}

struct Context {
    root: PathBuf,
    graph: Graph,
}

impl Context {
    fn open(options: &Options) -> Result<Self, String> {
        let repo = options.repo.clone().unwrap_or_else(|| PathBuf::from("."));
        let root = PathBuf::from(git(&repo, &["rev-parse", "--show-toplevel"])?);
        let db = options.db.clone().unwrap_or_else(default_path);
        let graph = Graph::open_read_only(&db, PATIENCE).map_err(|why| format!("cannot open the graph {}: {why}", db.display()))?;
        Ok(Self { root, graph })
    }

    fn facts(&self) -> Result<Facts<'_>, String> {
        Facts::read(&self.graph, &repository(&self.root))
    }
}

fn change_between(root: &Path, facts: &Facts, base: &str, head: &str) -> Result<(Change, Option<Hunks>), String> {
    let change = Change::read(root, base, head)?;
    let stale = facts.coverage_commit.as_deref().and_then(|commit| lane::carried_since(root, commit, base, &change));
    Ok((change, stale))
}

fn cone_between(root: &Path, facts: &Facts, base: &str, head: &str) -> Result<Cone, String> {
    let (change, stale) = change_between(root, facts, base, head)?;
    cone::of(facts, &change, stale.as_ref())
}

fn cones(root: &Path, facts: &Facts, words: &[String]) -> Result<Vec<(String, Cone)>, String> {
    if words.is_empty() {
        return Err("name at least one lane as NAME=BASE..HEAD".to_owned());
    }
    words.iter().map(|word| Lane::parse(word).and_then(|lane| Ok((lane.name, cone_between(root, facts, &lane.base, &lane.head)?)))).collect()
}

fn carried(root: &Path, facts: &Facts, base: &str, options: &Options) -> BTreeSet<TestId> {
    if options.ran.is_empty() {
        return BTreeSet::new();
    }
    let lanes = match cones(root, facts, &options.lanes) {
        Ok(lanes) => lanes,
        Err(why) => {
            eprintln!("north graph ask affected: no alone pass carries a test, as the landed lanes have no cone: {why}");
            return BTreeSet::new();
        }
    };
    let pass = |word: &String| -> Result<Pass, String> {
        let lane = Lane::parse(word)?;
        Ok(Pass { ran: cone_between(root, facts, &lane.base, &lane.head)?, unseen: cone_between(root, facts, &lane.base, base)?, lane: lane.name })
    };
    let passes: Vec<Pass> = options.ran.iter().filter_map(|word| pass(word).map_err(|why| eprintln!("north graph ask affected: the alone pass {word} carries no test: {why}")).ok()).collect();
    carry::carried(&lanes, &passes)
}

fn ids(tests: &BTreeSet<TestId>) -> Vec<String> {
    tests.iter().map(ToString::to_string).collect()
}

pub fn run(arguments: &[String]) -> u8 {
    let Some((verb, rest)) = arguments.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let parsed = match options(rest) {
        Ok(parsed) => parsed,
        Err(why) => {
            eprintln!("north graph ask {verb}: {why}\n{USAGE}");
            return 2;
        }
    };
    let outcome = Context::open(&parsed).and_then(|context| match verb.as_str() {
        "affected" => affected(&context, &parsed),
        "overlap" => overlap(&context, &parsed),
        "blame" => blame(&context, &parsed),
        _ => Err(USAGE.to_owned()),
    });
    match outcome {
        Ok(code) => code,
        Err(why) => {
            eprintln!("north graph ask {verb}: {why}");
            1
        }
    }
}

fn affected(context: &Context, options: &Options) -> Result<u8, String> {
    let [base, head] = options.positional.as_slice() else { return Err(format!("affected takes BASE HEAD\n{USAGE}")) };
    let facts = context.facts()?;
    let (change, stale) = change_between(&context.root, &facts, base, head)?;
    let mut cone = cone::of(&facts, &change, stale.as_ref())?;
    if shadow::shadowed(&context.root) && let Err(why) = shadow::record(base, head, &cone, &cone::proof(&facts, &change, stale.as_ref())) {
        eprintln!("north graph ask affected: gate {} logs nothing: {why}", shadow::GATE);
    }
    let carried = if cone.untraced.is_none() { carried(&context.root, &facts, base, options) } else { BTreeSet::new() };
    cone.tests.retain(|test| !carried.contains(test));
    cone.always.retain(|test| !carried.contains(test));
    if options.json {
        let since = cone::since_base(&context.root, &facts, base, &cone)?;
        let added: Vec<Value> = cone.added.iter().map(|test| json!({ "package": test.package, "name": test.name })).collect();
        println!("{}", json!({ "tests": ids(&cone.tests), "always": ids(&cone.always), "added": added, "files": cone.files, "untraced": cone.untraced, "known": facts.all_tests().len(), "carried": ids(&carried), "since": ids(&since) }));
    } else if options.nextest {
        let arguments = cone.nextest_arguments();
        if cone.untraced.is_none() && !arguments.is_empty() {
            println!("{}", arguments.join(" "));
        }
    } else if cone.untraced.is_none() {
        for test in cone.tests.iter().chain(&cone.always) {
            println!("{test}");
        }
    }
    match &cone.untraced {
        Some(reason) => {
            eprintln!("north graph ask affected: untraced: {reason}");
            Ok(UNTRACED)
        }
        None => Ok(0),
    }
}

fn overlap(context: &Context, options: &Options) -> Result<u8, String> {
    let facts = context.facts()?;
    let lanes = cones(&context.root, &facts, &options.positional)?;
    let found = overlap::of(&lanes, || facts.all_tests());
    if options.json {
        let pairs: Vec<Value> = found.pairs.iter().map(|pair| json!({ "a": pair.a, "b": pair.b, "tests": ids(&pair.tests) })).collect();
        println!("{}", json!({ "groups": found.groups, "pairs": pairs, "always": ids(&found.always) }));
    } else {
        for group in &found.groups {
            println!("{}", group.join(" "));
        }
    }
    Ok(0)
}

fn blame(context: &Context, options: &Options) -> Result<u8, String> {
    let facts = context.facts()?;
    let lanes = cones(&context.root, &facts, &options.positional)?;
    let mut failures: Vec<Failure> = options.failed.iter().map(|text| Failure::test(text)).collect();
    failures.extend(options.compile.iter().map(|file| Failure::compile(file)));
    let root = context.root.to_string_lossy();
    for log in &options.logs {
        let text = std::fs::read_to_string(log).map_err(|why| format!("cannot read {}: {why}", log.display()))?;
        failures.extend(blame::from_log(&text, &root));
    }
    if failures.is_empty() {
        return Err("no failure to blame: pass --failed, --compile or a --log naming one".to_owned());
    }
    let suspects: Vec<Suspect> = lanes.iter().map(|(name, cone)| Suspect { name, cone }).collect();
    let found = blame::of(&failures, &suspects, &facts);
    if options.json {
        println!("{}", json!({ "lanes": found.lanes, "by_failure": found.by_failure, "untraced": found.untraced }));
    } else {
        for lane in &found.lanes {
            println!("{lane}");
        }
    }
    if found.lanes.is_empty() {
        eprintln!("north graph ask blame: no lane's cone holds any of the {} failures", failures.len());
        return Ok(UNTRACED);
    }
    Ok(0)
}

pub fn ask(arguments: &[String]) -> u8 {
    match arguments.first() {
        Some(verb) if VERBS.contains(&verb.as_str()) => run(arguments),
        _ => north_graph::run(&[&["ask".to_owned()], arguments].concat()),
    }
}
