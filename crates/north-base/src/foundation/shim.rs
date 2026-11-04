use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn opens_for_writing(info: &str) -> bool {
    info.lines()
        .find_map(|line| u32::from_str_radix(line.strip_prefix("flags:")?.trim(), 8).ok())
        .is_some_and(|flags| flags & 0o3 != 0)
}

fn held_for_writing(target: &Path) -> bool {
    let Ok(processes) = std::fs::read_dir("/proc") else { return false };
    processes.flatten().any(|process| {
        let Ok(descriptors) = std::fs::read_dir(process.path().join("fd")) else { return false };
        descriptors.flatten().any(|descriptor| {
            std::fs::read_link(descriptor.path()).is_ok_and(|opened| opened == target)
                && std::fs::read_to_string(process.path().join("fdinfo").join(descriptor.file_name())).is_ok_and(|info| opens_for_writing(&info))
        })
    })
}

pub fn wait_until_executable(script: &Path) {
    let target = std::fs::canonicalize(script).unwrap();
    while held_for_writing(&target) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn write_executable(path: &Path, content: impl AsRef<[u8]>) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    wait_until_executable(path);
}

pub fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n[ -z \"$NORTH_SHIM_PROBE\" ] || exit 0\n{body}")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    while Command::new(path).env("NORTH_SHIM_PROBE", "1").status().is_err_and(|error| error.kind() == ErrorKind::ExecutableFileBusy) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::sync::mpsc;
    use std::time::Instant;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("north-shim-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("script")
    }

    #[test]
    fn returns_when_no_writer_holds_the_file() {
        let path = scratch("idle");
        write_script(&path, "exit 0\n");
        let started = Instant::now();
        wait_until_executable(&path);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn waits_while_a_writer_holds_the_file_and_returns_after_close() {
        let path = scratch("held");
        write_script(&path, "exit 0\n");
        let (opened_tx, opened_rx) = mpsc::channel();
        let holder_path = path.clone();
        let holder = std::thread::spawn(move || {
            let file = OpenOptions::new().write(true).open(&holder_path).unwrap();
            opened_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            drop(file);
        });
        opened_rx.recv().unwrap();
        let started = Instant::now();
        wait_until_executable(&path);
        assert!(started.elapsed() >= Duration::from_millis(100));
        holder.join().unwrap();
    }
}
