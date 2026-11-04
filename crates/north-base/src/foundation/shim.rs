use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn write_executable(path: &Path, content: impl AsRef<[u8]>) {
    let mut installer = Command::new("install")
        .args(["-m", "755", "/dev/stdin"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    installer.stdin.take().unwrap().write_all(content.as_ref()).unwrap();
    let status = installer.wait().unwrap();
    assert!(status.success(), "install {}: {status}", path.display());
}

pub fn write_script(path: &Path, body: &str) {
    write_executable(path, format!("#!/bin/sh\n{body}"));
}

pub fn copy_executable(source: impl AsRef<Path>, destination: impl AsRef<Path>) {
    let status = Command::new("install").args(["-m", "755"]).arg(source.as_ref()).arg(destination.as_ref()).status().unwrap();
    assert!(status.success(), "install {} {}: {status}", source.as_ref().display(), destination.as_ref().display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const WRITERS: usize = 8;
    const ROUNDS: usize = 150;

    fn hammer(write: fn(&Path, &str)) -> Vec<String> {
        let scratch = Arc::new(tempfile::tempdir().unwrap());
        let launched = Arc::new(AtomicUsize::new(0));
        let writers: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let scratch = Arc::clone(&scratch);
                let launched = Arc::clone(&launched);
                std::thread::spawn(move || {
                    let mut failures = Vec::new();
                    for round in 0..ROUNDS {
                        let script = scratch.path().join(format!("script-{writer}-{round}"));
                        write(&script, "exit 0\n");
                        match Command::new(&script).status() {
                            Ok(status) if status.success() => {
                                launched.fetch_add(1, Ordering::SeqCst);
                            }
                            Ok(status) => failures.push(format!("{status}")),
                            Err(error) => failures.push(error.to_string()),
                        }
                    }
                    failures
                })
            })
            .collect();
        let failures: Vec<String> = writers.into_iter().flat_map(|writer| writer.join().unwrap()).collect();
        assert_eq!(launched.load(Ordering::SeqCst) + failures.len(), WRITERS * ROUNDS);
        failures
    }

    #[test]
    fn scripts_written_and_run_by_concurrent_forking_threads_never_hit_a_busy_text_file() {
        let failures = hammer(write_script);
        assert!(failures.is_empty(), "{} of {} launches failed: {:?}", failures.len(), WRITERS * ROUNDS, failures.first());
    }

    #[test]
    fn a_copied_executable_runs_straight_after_the_copy() {
        let scratch = tempfile::tempdir().unwrap();
        let source = scratch.path().join("source");
        write_script(&source, "echo copied\n");
        let copy = scratch.path().join("copy");
        copy_executable(&source, &copy);
        let output = Command::new(&copy).output().unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "copied\n");
    }
}
