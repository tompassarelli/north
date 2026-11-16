//! The proof run's pacer (north#1632): writes stand-in boxes onto one issue per stage of the scratch repository at each
//! stage's rate, so dispatch staffs them with stand-in workers, counts the stand-in landings the lander records, and
//! stops by itself when a box written over five minutes ago has not landed for five minutes running.
use std::process::Command;
use std::time::{Duration, Instant};

const USAGE: &str = "usage: north proof-run --repo OWNER/REPO [--profile-repo NAME] [--stages RATE:MINUTES,...] [--dies-at RATE --dies N]";
const STAGES: &str = "30:10,60:10,120:10,210:30";
const LATE: Duration = Duration::from_mins(5);
const TICK: Duration = Duration::from_secs(5);
const HEADER: &str = "Category: tooling\nSpec: exact\nScope: cross-module\nSurface: shell\nVerify: local-test\n\nStand-in boxes of the north#1632 proof run: each lands one line under lanes/standin/ through a stand-in worker.\n\n## Done when\n";
const LANE_PREFIX: &str = "si";

struct Plan {
    repo: String,
    profile_repo: String,
    stages: Vec<(u32, u32)>,
    dies_at: Option<u32>,
    dies: u32,
}

struct Stage {
    number: u64,
    body: String,
}

struct Watch {
    profile_repo: String,
    since: u64,
    started: Instant,
    written: Vec<Instant>,
    behind_since: Option<Instant>,
    last_read: Option<Instant>,
}

impl Watch {
    fn due(&self) -> bool {
        self.last_read.is_none_or(|at| at.elapsed() >= Duration::from_mins(1))
    }

    fn read(&mut self, rate: u32) -> Result<(), String> {
        self.last_read = Some(Instant::now());
        let landed = landed(&self.profile_repo, self.since);
        let due = self.written.iter().filter(|at| at.elapsed() >= LATE).count();
        println!("t+{}s stage {rate}/h: written {} landed {} due {due}", self.started.elapsed().as_secs(), self.written.len(), landed.as_ref().map_or_else(|why| format!("unread ({why})"), usize::to_string));
        let late = landed.is_ok_and(|count| count < due);
        self.behind_since = if late { self.behind_since.or(Some(Instant::now())) } else { None };
        match self.behind_since {
            Some(since) if since.elapsed() >= LATE => Err(format!("stage {rate}/h fell behind: boxes written over 5 min ago stayed unlanded for 5 min")),
            _ => Ok(()),
        }
    }
}

pub(super) fn run(arguments: &[String]) -> u8 {
    let plan = match parse(arguments) {
        Ok(plan) => plan,
        Err(why) => {
            eprintln!("north proof-run: {why}\n{USAGE}");
            return 64;
        }
    };
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |at| at.as_secs());
    let mut watch = Watch { profile_repo: plan.profile_repo.clone(), since, started: Instant::now(), written: Vec::new(), behind_since: None, last_read: None };
    for (rate, minutes) in plan.stages.iter().copied() {
        if let Err(why) = pace(&plan, rate, minutes, &mut watch) {
            println!("stop: {why}");
            return 2;
        }
    }
    println!("done: {} boxes written in {} s", watch.written.len(), watch.started.elapsed().as_secs());
    0
}

fn pace(plan: &Plan, rate: u32, minutes: u32, watch: &mut Watch) -> Result<(), String> {
    let start = Instant::now();
    let mut stage = open(&plan.repo, rate).map_err(|why| format!("stage {rate}/h opened no issue: {why}"))?;
    println!("stage {rate}/h: issue {}#{} for {minutes} min", plan.repo, stage.number);
    let gap = Duration::from_secs_f64(3600.0 / f64::from(rate));
    let mut boxes = 0u32;
    while start.elapsed() < Duration::from_mins(u64::from(minutes)) {
        if start.elapsed() >= gap * boxes {
            boxes += 1;
            let dies = plan.dies_at == Some(rate) && boxes <= plan.dies;
            stage.body.push_str(&format!("- [ ] standin {rate}-{boxes:04}{}\n", if dies { " dies once mid-lane" } else { "" }));
            match edit(&plan.repo, &stage) {
                Ok(()) => watch.written.push(Instant::now()),
                Err(why) => println!("box {rate}-{boxes:04} not written: {why}"),
            }
        }
        if watch.due() {
            watch.read(rate)?;
        }
        std::thread::sleep(TICK);
    }
    Ok(())
}

fn parse(arguments: &[String]) -> Result<Plan, String> {
    let mut plan = Plan { repo: String::new(), profile_repo: "autoland-scratch".into(), stages: Vec::new(), dies_at: None, dies: 0 };
    let mut stages = STAGES.to_owned();
    let mut words = arguments.iter();
    while let Some(flag) = words.next() {
        let value = words.next().ok_or_else(|| format!("{flag} takes a value"))?.clone();
        match flag.as_str() {
            "--repo" => plan.repo = value,
            "--profile-repo" => plan.profile_repo = value,
            "--stages" => stages = value,
            "--dies-at" => plan.dies_at = Some(value.parse().map_err(|_| format!("--dies-at {value} is no rate"))?),
            "--dies" => plan.dies = value.parse().map_err(|_| format!("--dies {value} is no count"))?,
            _ => return Err(format!("unknown flag {flag}")),
        }
    }
    if plan.repo.is_empty() {
        return Err("--repo is required".into());
    }
    for stage in stages.split(',') {
        let (rate, minutes) = stage.split_once(':').ok_or_else(|| format!("stage {stage} is not RATE:MINUTES"))?;
        let number = |text: &str| text.parse::<u32>().ok().filter(|value| *value > 0).ok_or_else(|| format!("stage {stage} is not RATE:MINUTES"));
        plan.stages.push((number(rate)?, number(minutes)?));
    }
    Ok(plan)
}

fn gh(arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("gh").args(arguments).output().map_err(|why| format!("gh: {why}"))?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}

fn open(repo: &str, rate: u32) -> Result<Stage, String> {
    let url = gh(&["issue", "create", "-R", repo, "--title", &format!("Stand-in stage {rate} an hour (north#1632 proof run)"), "--body", HEADER])?;
    let number = url.trim().rsplit('/').next().and_then(|tail| tail.parse().ok()).ok_or_else(|| format!("gh printed no issue URL: {}", url.trim()))?;
    Ok(Stage { number, body: HEADER.to_owned() })
}

fn edit(repo: &str, stage: &Stage) -> Result<(), String> {
    gh(&["issue", "edit", &stage.number.to_string(), "-R", repo, "--body", &stage.body]).map(drop)
}

fn landed(profile_repo: &str, since: u64) -> Result<usize, String> {
    let window = format!("{}m", (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |at| at.as_secs()).saturating_sub(since) + 60).div_ceil(60));
    let output = Command::new(north_base::platform::north_binary::north_binary()).args(["land", "profile", "--since", &window, "--repo", profile_repo]).output().map_err(|why| format!("north land profile: {why}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).lines().last().unwrap_or_default().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.contains(" landed ") && line.trim_start_matches("- ").starts_with(LANE_PREFIX)).count())
}
