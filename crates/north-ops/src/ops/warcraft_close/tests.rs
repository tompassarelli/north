use std::process::{Child, Command, Stdio};
use std::time::Duration;

use super::{Launch, Timing, close};

const PAD: usize = 60_000;

fn standin(program: &str, arguments: &[&str], environment: &[(&str, String)]) -> Child {
    let mut command = Command::new(program);
    command.args(arguments).env_clear().stdout(Stdio::null());
    environment.iter().for_each(|(key, value)| {
        command.env(key, value);
    });
    command.spawn().unwrap()
}

fn launch_in(root: &std::path::Path, appid: &str) -> Launch {
    Launch { proc: "/proc".into(), compat: root.join("compat").to_string_lossy().into_owned(), appid: appid.to_owned() }
}

fn sleep_program() -> String {
    std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join("sleep")).find(|path| path.is_file()).unwrap().to_string_lossy().into_owned()
}

fn quick() -> Timing {
    Timing { poll: Duration::from_millis(50), polls: 100, settle: Duration::from_millis(200) }
}

fn alive(child: &mut Child) -> bool {
    child.try_wait().unwrap().is_none()
}

#[test]
fn closing_ends_every_process_of_the_launch_and_spares_another_prefix() {
    let root = tempfile::tempdir().unwrap();
    let compat = root.path().join("compat").to_string_lossy().into_owned();
    let appid = format!("999999{}", std::process::id());
    let pad = "x".repeat(PAD);
    let sleep = sleep_program();
    let background = format!("{sleep} 300 & wait");
    let big = [("STEAM_COMPAT_DATA_PATH", compat.clone()), ("PAD1", pad.clone()), ("PAD2", pad.clone())];
    let mut standins = [
        standin(&sleep, &["300"], &big),
        standin("/bin/sh", &["-c", &background], &[("STEAM_COMPAT_DATA_PATH", compat), ("PAD1", pad)]),
        standin("/bin/sh", &["-c", &background, "reaper", "SteamLaunch", &format!("AppId={appid}"), "--"], &[]),
    ];
    let mut bystander = standin(&sleep, &["300"], &[("STEAM_COMPAT_DATA_PATH", root.path().join("other-compat").to_string_lossy().into_owned())]);
    let launch = launch_in(root.path(), &appid);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(launch.members().len(), 5, "the prefix in a large environment, the reaper's AppId and their descendants");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = close(&launch, &quick(), &mut |line| out.push(line.to_owned()), &mut |line| err.push(line.to_owned()));
    assert_eq!((code, out.as_slice(), err.len()), (0, ["warcraft-close: closed 5 processes of Tom's Steam launch; a process check finds none left".to_owned()].as_slice(), 0));
    assert!(launch.members().is_empty());
    assert!(standins.iter_mut().all(|child| !alive(child)));
    assert!(alive(&mut bystander));
    bystander.kill().unwrap();
    bystander.wait().unwrap();
}

#[test]
fn closing_fails_and_names_a_process_that_survives_kill() {
    let root = tempfile::tempdir().unwrap();
    let proc = root.path().join("proc");
    let pid = 4_194_399_u32;
    let dir = proc.join(pid.to_string());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stat"), format!("{pid} (Battle.net.exe) S 1 1 1 0 -1 0\n")).unwrap();
    let compat = root.path().join("compat").to_string_lossy().into_owned();
    std::fs::write(dir.join("environ"), format!("STEAM_COMPAT_DATA_PATH={compat}\0")).unwrap();
    std::fs::write(dir.join("cmdline"), "C:\\Program Files (x86)\\Battle.net\\Battle.net.exe\0").unwrap();
    let launch = Launch { proc, compat, appid: "1".to_owned() };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = close(&launch, &Timing { poll: Duration::from_millis(1), polls: 2, settle: Duration::from_millis(1) }, &mut |line| out.push(line.to_owned()), &mut |line| err.push(line.to_owned()));
    assert_eq!(code, 1);
    assert!(out.is_empty());
    assert_eq!(err[1], format!("  {pid} C:\\Program Files (x86)\\Battle.net\\Battle.net.exe "));
}
