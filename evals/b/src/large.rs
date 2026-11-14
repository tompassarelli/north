use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use serde_json::{Value, json};

use crate::collect;
use crate::ladder::{self, Plan, Run};
use crate::proc::{TOOLPATH, git, local_time, must, now, sh};
use crate::scratch::{self, LARGE};

pub const WORKERS: [usize; 3] = [1, 3, 9];
pub const LIMIT_S: f64 = 14_400.0;
const POLL_S: u64 = 5;
const PARSE_TOML: &str = "console.log(JSON.stringify(Bun.TOML.parse(await Bun.file(process.argv[1]).text())))";

type Placed = (usize, Run);

pub struct Task {
    pub id: String,
    pub base: String,
    pub text: String,
    pub acceptance: String,
    pub dir: PathBuf,
}

pub struct Part {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub text: String,
    pub files: Vec<String>,
}

pub struct Split {
    pub parts: Vec<Part>,
    pub split_s: f64,
    pub source: String,
}

pub struct Large<'a> {
    pub task: Task,
    pub split: Split,
    pub workers: usize,
    pub stamp: String,
    pub plan: Plan,
    pub repo: &'a Path,
}

pub fn toml(path: &Path) -> Result<Value, String> {
    let ran = sh(&format!("PATH={TOOLPATH}:$PATH bun -e '{PARSE_TOML}' '{}'", path.display()));
    serde_json::from_str(&must(ran, &format!("read {}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn task(tasks: &Path, id: &str) -> Result<Task, String> {
    let dir = fs::canonicalize(tasks).map_err(|e| format!("{}: {e}", tasks.display()))?.parent().map(Path::to_path_buf).unwrap_or_default();
    if dir.join("answers.toml").exists() {
        return Err(format!("{} holds answers.toml, which no worker may read; point --tasks at a copy of tasks.toml and spec/ without it", dir.display()));
    }
    let all = toml(tasks)?;
    let found = all["task"].as_array().into_iter().flatten().find(|t| t["id"] == id).ok_or_else(|| format!("{id} is not a task of {}", tasks.display()))?;
    let field = |key: &str| found[key].as_str().map(str::to_owned).ok_or_else(|| format!("task {id} has no {key}"));
    let expand = |text: String| text.replace("$EVAL_LARGE", &dir.to_string_lossy());
    Ok(Task { id: id.to_owned(), base: field("base")?, text: field("text")?.trim().to_owned(), acceptance: expand(field("check")?), dir })
}

fn files(entry: &Value) -> Vec<String> {
    entry["files"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

pub fn split(path: &Path) -> Result<Split, String> {
    let plan = toml(path)?;
    let source = format!("{} (sha256 {})", path.display(), sh(&format!("sha256sum '{}' | cut -c1-64", path.display())).out.trim());
    let split_s = plan["split_s"].as_f64().unwrap_or(0.0);
    let listed = plan["part"].as_array().cloned().unwrap_or_default();
    if listed.is_empty() {
        let whole = Part { id: "whole".to_owned(), kind: "whole", title: "the whole task in one session".to_owned(), text: String::new(), files: Vec::new() };
        return Ok(Split { parts: vec![whole], split_s, source });
    }
    let interface = &plan["interface"];
    let text = |entry: &Value, what: &str| entry["text"].as_str().map(|t| t.trim().to_owned()).ok_or_else(|| format!("{}: {what} has no text", path.display()));
    let mut parts = vec![Part { id: "interface".to_owned(), kind: "interface", title: "the shared interface".to_owned(), text: text(interface, "[interface]")?, files: files(interface) }];
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for (i, entry) in listed.iter().enumerate() {
        let id = format!("part-{}", i + 1);
        for file in files(entry) {
            if let Some(other) = owner.insert(file.clone(), id.clone()) {
                return Err(format!("{}: {file} is in both {other} and {id}; a plan's parts change disjoint files", path.display()));
            }
        }
        let title = entry["name"].as_str().map_or_else(|| id.clone(), str::to_owned);
        parts.push(Part { title, text: text(entry, &id)?, files: files(entry), id, kind: "part" });
    }
    Ok(Split { parts, split_s, source })
}

pub fn box_check(task: &Task, kind: &str) -> String {
    let spec = format!("{}/spec/", task.dir.display());
    let build = task.acceptance.split_once(" && ").filter(|(first, _)| first.starts_with("cp -r ") && first.contains(&spec)).map(|(_, rest)| rest.to_owned());
    if kind == "whole" { task.acceptance.clone() } else { build.unwrap_or_else(|| task.acceptance.clone()) }
}

impl Large<'_> {
    pub fn run_id(&self) -> String {
        format!("{}-{}-w{}", self.stamp, self.task.id, self.workers)
    }

    pub fn branch(&self, part: &Part) -> String {
        format!("{}-w{}-{}", self.task.id, self.workers, part.id)
    }

    pub fn session(&self, part: &Part) -> String {
        format!("evalb-{}-w{}-{}-{}", self.task.id, self.workers, part.id, self.stamp)
    }

    fn brief(&self, part: &Part) -> Value {
        let task = &self.task;
        let boxes = self.split.parts.len();
        let scope = format!("Change only these files: {}.", part.files.join(", "));
        let goal = format!("The whole task's acceptance command, which passes once every box has landed: {}", task.acceptance);
        let text = match part.kind {
            "interface" => format!("{}\n\nThis run splits the task into {boxes} boxes. Yours is the shared interface, which lands alone before the other boxes start: {}\n{scope}\n{goal}", task.text, part.text),
            "part" => format!(
                "{}\n\nThis run splits the task into {boxes} boxes. The shared interface has landed on main: {}\nYours is {} ({}), worked beside the other parts, which change the other files: {}\n{scope}\n{goal}",
                task.text, self.split.parts[0].text, part.id, part.title, part.text
            ),
            _ => task.text.clone(),
        };
        json!({"id": part.id, "text": text, "check": box_check(task, part.kind), "message": format!("Do large task {} {} (Run: {}-{})", task.id, part.id, self.run_id(), part.id)})
    }

    pub fn lay_out(&self) -> Result<(), String> {
        let root = scratch::root(LARGE);
        let base = &self.task.base;
        must(sh(&scratch::reset(&root)), "reset the scratch project")?;
        must(git(&scratch::origin(LARGE), &["fetch", "-q", &self.repo.to_string_lossy(), base]), &format!("fetch base {base} from {}", self.repo.display()))?;
        must(git(&scratch::origin(LARGE), &["update-ref", "refs/heads/main", base]), "point the scratch origin's main at the base")?;
        must(sh(&scratch::lanes(&root, &[])), "make the lander's checkout")?;
        let forget = |dir: &str| format!("git -C {dir} for-each-ref --format='%(refname)' | grep -vx -e refs/heads/main -e refs/remotes/origin/main | xargs -r -n1 git -C {dir} update-ref -d\ngit -C {dir} reflog expire --expire=now --all\ngit -C {dir} gc -q --prune=now\n");
        must(sh(&format!("set -e\nrm -f {root}/origin.git/FETCH_HEAD {root}/main/.git/FETCH_HEAD\n{}{}", forget(&format!("{root}/origin.git")), forget(&format!("{root}/main")))), "drop every earlier run's objects")?;
        let check = self.plan.work.join("landcheck.sh");
        fs::write(&check, format!("#!/bin/sh\nexport EVAL_LARGE='{}' PATH={TOOLPATH}:$PATH\n{}\n", self.task.dir.display(), box_check(&self.task, self.split.parts[0].kind))).map_err(|e| e.to_string())?;
        let main = scratch::checkout(LARGE);
        fs::write(main.join(".safe-push"), format!("[landing]\n\tqueue = lander\n\tlander-host = {}\n\tremote-hosts =\n[land]\n\tcheck = ** :: sh {}\n", self.plan.hosts[0], check.display())).map_err(|e| e.to_string())?;
        must(git(&main, &["add", ".safe-push"]), "stage the scratch land rule")?;
        must(git(&main, &["commit", "-qm", &format!("Land this eval clone through its scratch lander (Run: {})", self.run_id())]), "commit the scratch land rule")?;
        must(git(&main, &["push", "-q", "origin", "main"]), "push the scratch land rule").map(|_| ())
    }

    fn lane(&self, part: &Part) -> Result<(), String> {
        let root = scratch::root(LARGE);
        let branch = self.branch(part);
        must(sh(&format!("set -e\ngit -C {root}/main fetch -q origin\ngit -C {root}/main worktree add -q -b {branch} {} origin/main\n", scratch::worktree(LARGE, &branch))), &format!("make the lane of {}", part.id)).map(|_| ())
    }

    pub fn dry_rows(&self) -> Vec<Value> {
        self.split.parts.iter().enumerate().map(|(order, part)| {
            let wave = if part.kind == "part" { format!("parts, at most {} at once, after the interface lands", self.workers) } else { "first, alone".to_owned() };
            json!({"arm": self.plan.arm, "row": "box", "dry": true, "run_id": self.run_id(), "task": self.task.id, "workers": self.workers, "box": part.id, "kind": part.kind, "title": part.title,
                   "files": part.files, "launch_order": order + 1, "wave": wave, "branch": self.branch(part), "session": self.session(part), "lane": scratch::worktree(LARGE, &self.branch(part)),
                   "check": box_check(&self.task, part.kind), "brief": ladder::prompt(&self.brief(part)), "plan": self.split.source, "host": self.plan.hosts[0], "account": self.plan.accounts[0]})
        }).collect()
    }

    fn start(&self, part: &Part, wave: &'static str) -> Run {
        let started = now();
        let made = self.lane(part);
        let mut run = ladder::launch(&self.plan, &self.brief(part), &self.plan.accounts[0], &self.branch(part), &self.session(part), wave, (started, None, 0.0));
        if let Err(error) = made {
            run.launch_output = format!("{error}\n{}", run.launch_output);
        }
        let _ = fs::create_dir_all(self.plan.work.join(&run.name));
        let _ = fs::write(self.plan.work.join(&run.name).join("launch.log"), &run.launch_output);
        eprintln!("eval-b: launched {} (exit {}, {:.0} s)", run.name, run.launch_exit, run.launch_end - run.launch_start);
        run
    }

    pub fn drive(&self) -> (f64, Vec<Placed>) {
        let began = now();
        let deadline = began - self.split.split_s + LIMIT_S;
        let mut pending: Vec<usize> = (1..self.split.parts.len()).rev().collect();
        let mut active = vec![(0, self.start(&self.split.parts[0], "interface"))];
        let mut done: Vec<Placed> = Vec::new();
        let mut collectors = Vec::new();
        loop {
            let interface = done.iter().find(|(i, _)| *i == 0).map(|(_, r)| landed(r));
            if interface == Some(false) || now() > deadline {
                pending.clear();
            }
            let free = if interface == Some(true) { self.workers.saturating_sub(active.len()) } else { 0 };
            let batch: Vec<usize> = (0..free).filter_map(|_| pending.pop()).collect();
            batch.iter().for_each(|&i| active.push((i, self.start(&self.split.parts[i], "parts"))));
            if active.is_empty() && pending.is_empty() {
                break;
            }
            thread::sleep(std::time::Duration::from_secs(POLL_S));
            let names: Vec<&str> = active.iter().map(|(_, r)| r.name.as_str()).collect();
            let status = ladder::statuses(&names);
            let lands = ladder::landed_branches(scratch::queue_dir(LARGE).as_deref());
            let at = now();
            for (_, run) in active.iter_mut().filter(|(_, r)| r.resolved.is_none()) {
                let name = run.name.clone();
                ladder::step(run, deadline - run.launch_start, (&name, status.get(&name)), lands.get(&run.branch).copied().filter(|end| *end >= run.launch_start), at);
            }
            let (resolved, still): (Vec<Placed>, Vec<Placed>) = active.into_iter().partition(|(_, r)| r.resolved.is_some());
            active = still;
            for (i, run) in resolved {
                eprintln!("eval-b: {} resolved: {}", run.name, run.resolved.as_ref().map_or("", |r| r.1.as_str()));
                let (work, name) = (self.plan.work.clone(), run.name.clone());
                collectors.push(thread::spawn(move || collect::session(&work, &name)));
                done.push((i, run));
            }
        }
        for collector in collectors {
            let _ = collector.join();
        }
        (began, done)
    }

    fn check_at(&self, verify: &Path, commit: &str, command: &str) -> bool {
        let dir = verify.display();
        let env = format!("EVAL_LARGE='{}' PATH={TOOLPATH}:$PATH", self.task.dir.display());
        sh(&format!("cd '{dir}' && git checkout -q -f {commit} && git clean -fdq && {env} bash -c '{}' >/dev/null 2>&1", command.replace('\'', "'\\''"))).ok()
    }

    pub fn replay(&self, commits: &[(String, String)]) -> Result<(Vec<Value>, bool), String> {
        let work = &self.plan.work;
        must(git(work, &["clone", "-q", &scratch::origin(LARGE).to_string_lossy(), "verify"]), "clone the landed main")?;
        let verify = work.join("verify");
        let tip = git(&verify, &["rev-parse", "main"]).out.trim().to_owned();
        let replays: Vec<Value> = commits.iter().map(|(part, commit)| {
            let kind = self.split.parts.iter().find(|p| &p.id == part).map_or("part", |p| p.kind);
            let began = now();
            let pass = self.check_at(&verify, commit, &box_check(&self.task, kind));
            json!({"box": part, "commit": commit, "check": box_check(&self.task, kind), "pass": pass, "seconds": (now() - began).round()})
        }).collect();
        let correct = !tip.is_empty() && self.check_at(&verify, &tip, &self.task.acceptance);
        Ok((replays, correct))
    }

    pub fn rows(&self, began: f64, done: &[Placed], conditions: &Value, command: &str) -> Result<Vec<Value>, String> {
        let queue = scratch::queue_dir(LARGE);
        let mut rows: Vec<Value> = self.split.parts.iter().enumerate().map(|(i, part)| self.box_row(i, part, done.iter().find(|(j, _)| *j == i).map(|(_, r)| r), queue.as_deref(), began, conditions)).collect();
        let mut commits: Vec<(f64, String, String)> = rows.iter().filter_map(|r| Some((r["landing_span"]["end"].as_f64()?, r["box"].as_str()?.to_owned(), r["landed_commit"].as_str()?.to_owned()))).collect();
        commits.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (replays, correct) = self.replay(&commits.iter().map(|(_, b, c)| (b.clone(), c.clone())).collect::<Vec<_>>())?;
        let all_landed = rows.iter().all(|r| r["landed"] == true);
        let last = commits.last().map(|c| c.0);
        let elapsed = last.filter(|_| all_landed).unwrap_or_else(|| done.iter().filter_map(|(_, r)| r.resolved.as_ref().map(|x| x.0)).fold(began, f64::max)) - began + self.split.split_s;
        let sum = |key: &str| rows.iter().filter_map(|r| r[key].as_u64()).sum::<u64>();
        let tokens = rows.iter().filter_map(|r| r["tokens_total_spend"].as_u64()).sum::<u64>();
        let cost: Option<f64> = rows.iter().filter(|r| r["session"].is_string()).map(|r| r["cost_usd"].as_f64()).sum();
        let red = replays.iter().filter(|r| r["pass"] == false).count();
        let stopped = done.iter().any(|(_, r)| r.resolved.as_ref().is_some_and(|x| x.1 == "time-limit"));
        let failure = match (all_landed, correct) {
            (true, true) => String::new(),
            (true, false) => "acceptance-failed".to_owned(),
            _ => rows.iter().find(|r| r["landed"] != true).and_then(|r| r["failure"].as_str()).map_or_else(String::new, |f| format!("box-unlanded: {f}")),
        };
        let run = json!({
            "arm": self.plan.arm, "row": "run", "run_id": self.run_id(), "task": self.task.id, "workers": self.workers, "boxes": self.split.parts.len(), "parts": self.split.parts.len() - 1,
            "landed_boxes": rows.iter().filter(|r| r["landed"] == true).count(), "plan": self.split.source,
            "split_s": self.split.split_s, "clock": "from the split's start (split_s before the interface launch) to the last box's landing span end, counted only when the final main passes the acceptance command (run after, off the clock)",
            "started_at": local_time(began - self.split.split_s), "elapsed_s": elapsed.round(), "time_to_all_landed_s": if all_landed { json!(elapsed.round()) } else { Value::Null },
            "acceptance": correct, "time_to_all_landed_and_correct_s": if all_landed && correct { json!(elapsed.round()) } else { Value::Null },
            "failed": !(all_landed && correct), "failure": failure, "stopped_at_limit": stopped,
            "conflicts": sum("conflict_refusals"), "reworks": sum("reworks"), "handins": sum("handins"), "lander_refusals": sum("lander_refusals"), "interventions": sum("interventions"),
            "tokens_total_spend": tokens, "cost_usd": cost.map(|c| (c * 1e6).round() / 1e6),
            "main_red": red > 0, "main_red_count": red, "main_replays": replays,
            "main_red_basis": "after the run, off the clock: each landing commit replayed through the land check it landed on, and the final main through the acceptance command",
            "conditions": conditions, "command": command, "raw": self.plan.work,
        });
        rows.insert(0, run);
        Ok(rows)
    }

    fn box_row(&self, order: usize, part: &Part, run: Option<&Run>, queue: Option<&Path>, began: f64, conditions: &Value) -> Value {
        let base = json!({"arm": self.plan.arm, "row": "box", "run_id": self.run_id(), "task": self.task.id, "workers": self.workers, "box": part.id, "kind": part.kind, "title": part.title,
                          "files": part.files, "launch_order": order + 1, "branch": self.branch(part), "host": conditions["host"], "account": self.plan.accounts[0]});
        let Some(run) = run else {
            let mut row = base;
            row["landed"] = json!(false);
            row["failure"] = json!("not-launched");
            return row;
        };
        let dir = self.plan.work.join(&run.name);
        let transcript = collect::transcript(&dir);
        let (cost, total) = collect::cost(&dir, &transcript);
        let attempts = collect::spans(queue, &run.branch, run.launch_start);
        let landing = attempts.as_array().and_then(|a| a.iter().find(|x| x["attributes"]["landed"].is_string())).cloned().unwrap_or(Value::Null);
        let (end, reason) = run.resolved.clone().unwrap_or((run.launch_end, "unresolved".to_owned()));
        let landed = reason == "landed";
        let mut row = base;
        let fields = json!({
            "session": run.name, "launched_at": local_time(run.launch_start), "launch_epoch": run.launch_start, "launch_after_start_s": (run.launch_start - began).round(),
            "launch_s": (run.launch_end - run.launch_start).round(), "launch_exit": run.launch_exit, "landed": landed, "failure": if landed { String::new() } else { reason },
            "landed_commit": landing["attributes"]["landed"], "landing_span": {"start": landing["queued_at"], "end": landing["decided_at"]},
            "landing_span_s": landing["decided_at"].as_f64().zip(landing["queued_at"].as_f64()).map(|(e, s)| (e - s).round()), "queue_wait_s": landing["queue_wait_s"],
            "t_land_s": if landed { json!((end - run.launch_start).round()) } else { Value::Null },
            "lander_attempts": attempts.as_array().map_or(0, Vec::len), "lander_refusals": attempts.as_array().map_or(0, |a| a.iter().filter(|x| !x["attributes"]["landed"].is_string()).count()),
            "interventions": run.nudges.len(), "intervention_stalls": run.nudges, "model_requests": transcript["model_requests"], "tokens": transcript["tokens"], "tokens_total_spend": total, "cost_usd": cost,
            "last_words": if landed { String::new() } else { collect::last_words(&dir) }, "raw": dir, "transcript": dir.join("transcript.jsonl"),
        });
        fields.as_object().into_iter().flatten().for_each(|(k, v)| row[k] = v.clone());
        crate::rows::counted(&mut row, &transcript["handins"]);
        row
    }
}

fn landed(run: &Run) -> bool {
    run.resolved.as_ref().is_some_and(|r| r.1 == "landed")
}

pub fn conditions(large: &Large) -> Value {
    json!({
        "host": large.plan.hosts[0], "host_load_at_start": sh("cut -d' ' -f1-3 /proc/loadavg").out.trim(), "harness_north_built_from": sh("cut -c1-12 ~/.local/state/north/current/built-from").out.trim(),
        "other_fleet_sessions_working": crate::rows::working_sessions(), "provider": large.plan.provider, "model": large.plan.model, "effort_requested": large.plan.effort, "account": large.plan.accounts[0],
        "context_arm": "context-500k, pinned in every brief as the ladder's", "repo": "north", "base": large.task.base, "plan": large.split.source, "split_s": large.split.split_s, "limit_s": LIMIT_S,
        "caches": "cold lanes: a fresh worktree per box from the origin's main at its launch", "local_time_at_start": local_time(now()),
    })
}
