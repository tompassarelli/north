use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Ran {
    pub code: i32,
    pub out: Vec<u8>,
    pub err: String,
}

impl Ran {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.out).into_owned()
    }
}

pub fn run(program: &str, args: &[&str], env: &[(&str, &str)], unset: &[&str], stdin: Option<Vec<u8>>) -> Ran {
    let mut command = Command::new(program);
    command.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    unset.iter().for_each(|name| {
        command.env_remove(name);
    });
    command.envs(env.iter().copied());
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let Ok(mut child) = command.spawn() else {
        return Ran { code: 127, out: Vec::new(), err: format!("{program}: could not start") };
    };
    let writer = stdin.zip(child.stdin.take()).map(|(bytes, mut pipe)| std::thread::spawn(move || pipe.write_all(&bytes)));
    let finished = child.wait_with_output();
    if let Some(writer) = writer {
        let _ = writer.join();
    }
    match finished {
        Ok(output) => Ran { code: output.status.code().unwrap_or(-1), out: output.stdout, err: String::from_utf8_lossy(&output.stderr).into_owned() },
        Err(error) => Ran { code: -1, out: Vec::new(), err: error.to_string() },
    }
}

pub fn git(dir: &Path, args: &[&str], stdin: Option<Vec<u8>>) -> Result<Ran, String> {
    let dir = dir.to_string_lossy();
    let mut full = vec!["-C", dir.as_ref()];
    full.extend_from_slice(args);
    let ran = run("git", &full, &[], &[], stdin);
    if ran.code == 0 { Ok(ran) } else { Err(format!("git {}: exit {} {}", args.join(" "), ran.code, ran.err.trim())) }
}
