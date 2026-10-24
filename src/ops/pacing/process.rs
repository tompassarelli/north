use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(super) struct Output {
    pub code: i32,
    pub out: String,
    pub err: String,
}
pub(super) fn run(
    program: &str,
    args: &[&str],
    input: Option<&str>,
    timeout: f64,
) -> Result<Output, String> {
    run_env(program, args, input, timeout, &[])
}
pub(super) fn run_env(
    program: &str,
    args: &[&str],
    input: Option<&str>,
    timeout: f64,
    env: &[(&str, &str)],
) -> Result<Output, String> {
    let mut child = Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .process_group(0)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let read = |mut pipe: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let out = read(Box::new(child.stdout.take().ok_or("missing stdout")?));
    let err = read(Box::new(child.stderr.take().ok_or("missing stderr")?));
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or("missing stdin")?
            .write_all(input.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status.code().unwrap_or(1);
        }
        if start.elapsed().as_secs_f64() >= timeout {
            if let Some(group) = rustix::process::Pid::from_raw(child.id() as i32) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Kill);
            }
            let _ = child.wait();
            break 124;
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ok(Output {
        code,
        out: out.join().unwrap_or_default(),
        err: err.join().unwrap_or_default(),
    })
}
pub(super) fn fleet() -> Vec<Value> {
    run(&binary(), &["fleet", "list", "--json"], None, 5.0)
        .ok()
        .filter(|r| r.code == 0)
        .and_then(|r| serde_json::from_str::<Vec<Value>>(&r.out).ok())
        .unwrap_or_default()
}
pub(super) fn binary() -> String {
    std::env::var("NORTH_BIN").unwrap_or_else(|_| "north".into())
}
pub(super) fn send(name: &str, line: &str) -> Result<(), String> {
    let output = run(&binary(), &["fleet", "send", name, line], None, 10.0)?;
    if output.code == 0 {
        Ok(())
    } else {
        Err(format!("Fleet notification failed for {name}"))
    }
}
pub(super) fn adapter(program: &str, args: &[&str]) -> (Vec<Value>, Value) {
    let start = Instant::now();
    let output = run(program, args, None, 45.0).unwrap_or_else(|e| Output {
        code: 127,
        out: String::new(),
        err: e,
    });
    let err = output.err.to_lowercase();
    let rows = output
        .out
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .collect();
    (
        rows,
        json!({"elapsed_s":start.elapsed().as_secs_f64(),"code":output.code,
        "throttled":err.contains("429") || err.contains("rate limit") || err.contains("throttled=true")}),
    )
}
