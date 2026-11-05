use crate::capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use north_core::foundation::shim;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TOOLS: [&str; 46] = [
    "sh", "bash", "env", "git", "printenv", "realpath", "readlink", "dirname", "basename", "sha256sum", "cut", "mkdir", "find", "touch", "id", "cat", "xargs", "ln", "rm", "bwrap", "timeout", "setsid", "flock", "du", "df", "awk",
    "date", "tr", "sleep", "grep", "sed", "head", "tail", "wc", "ls", "mv", "cp", "chmod", "mktemp", "stat", "sort", "kill", "true", "false", "uname", "ssh-keygen",
];

const FAKE_NIX: &str = "case \"$1\" in
  eval) echo /nix/store/0000-carry-matrix-shell.drv ;;
  print-dev-env)
    flake=$2
    while [ \"$1\" != --profile ]; do shift; done
    ln -sfn \"$flake\" \"$2\" ;;
  develop)
    while [ \"$1\" != -c ]; do shift; done
    shift
    PATH=@TOOLCHAIN@:$PATH IN_NIX_SHELL=impure exec \"$@\" ;;
  *) exit 64 ;;
esac
";

const FAKE_SSH: &str = r#"while [ $# -gt 0 ]; do
  case "$1" in
    -o) shift 2 ;;
    -T) shift ;;
    --) shift; break ;;
    *) destination=${1#*@}; shift ;;
  esac
done
HOME=$FAKE_REMOTE_HOMES/$destination exec sh -c "$*"
"#;

const MISSING: &str = "no-such-program-d10";

struct Matrix {
    host: Host,
    home: PathBuf,
    project: PathBuf,
    toolchain: PathBuf,
    base: String,
    head: String,
}

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(["-c", "user.name=Carry matrix", "-c", "user.email=carry@example.invalid"]).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

impl Matrix {
    fn new() -> Self {
        let host = Host::new("carry-matrix-", &[("systemd-run", &scope_systemd_run("\"$@\"")), ("systemctl", SCOPE_SYSTEMCTL)]);
        let home = host.path("home");
        let (bin, toolchain, project) = (home.join(".local/bin"), home.join(".local/state/nix/profiles/toolchain/bin"), home.join("code/proj/main"));
        for directory in [&bin, &toolchain, &project, &home.join(".local/state/scopes")] {
            fs::create_dir_all(directory).unwrap();
        }
        let outer = std::env::var_os("PATH").unwrap();
        for tool in TOOLS {
            if let Some(found) = std::env::split_paths(&outer).map(|directory| directory.join(tool)).find(|path| path.is_file()) {
                symlink(found, bin.join(tool)).unwrap();
            }
        }
        symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
        shim::copy_executable(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/cargo"), bin.join("cargo"));
        shim::write_script(&bin.join("nix"), &FAKE_NIX.replace("@TOOLCHAIN@", &toolchain.display().to_string()));
        shim::write_script(&bin.join("ssh"), FAKE_SSH);
        shim::write_script(&toolchain.join("cargo"), "echo \"$0\"\n");
        fs::write(host.path("orchestration.toml"), "[hosts.gex45]\nssh = \"tom@gex45\"\n").unwrap();
        git(&project, &["init", "-q", "-b", "main"]);
        fs::write(project.join("flake.nix"), "{ outputs = _: { devShells = { }; }; }\n").unwrap();
        git(&project, &["add", "."]);
        git(&project, &["commit", "-qm", "Base"]);
        let base = git(&project, &["rev-parse", "HEAD"]);
        let remote = host.path("remotes/gex45/code/proj/main");
        git(&project, &["clone", "-q", project.to_str().unwrap(), remote.to_str().unwrap()]);
        fs::write(project.join("change"), "lane\n").unwrap();
        git(&project, &["add", "."]);
        git(&project, &["commit", "-qm", "Lane"]);
        let head = git(&project, &["rev-parse", "HEAD"]);
        Matrix { host, home, project, toolchain, base, head }
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = self.host.command(program);
        let path = format!("{}:{}", self.host.path("bin").display(), self.home.join(".local/bin").display());
        command
            .current_dir(&self.project)
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("SAFE_PUSH_LAND_BASE", &self.base)
            .env("FAKE_REMOTE_HOMES", self.host.path("remotes"))
            .env("NORTH_OFFLOAD_TARGET_FLOOR_MIB", "0")
            .env_remove("NORTH_DEV_SHELL")
            .env_remove("IN_NIX_SHELL")
            .env_remove("DEPBOX")
            .env_remove("AGENT_CAPACITY_LEASE")
            .env_remove("CARGO_TARGET_DIR");
        command
    }

    fn north(&self, arguments: &[&str], probe: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_north")).args(arguments).args(probe).output().unwrap()
    }

    fn sandboxed(&self, script: &str, probe: &[&str]) -> Output {
        let mut command = self.command(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(script));
        command.args(if script == "land-shell" { ["--dev", "--"] } else { ["run", "--"] }).args(probe).output().unwrap()
    }

    fn sandboxed_landing(&self, probe: &[&str]) -> Output {
        let mut command = self.command(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/depbox"));
        command.args(["run", "--"]).args(probe).env("SAFE_PUSH_LAND_PACKAGES", "-p north-core").env("SAFE_PUSH_LAND_CPUS", "3").env("SAFE_PUSH_LAND_IT", "1").env("SAFE_PUSH_LAND_RUNTIME_HEAD", &self.head).output().unwrap()
    }

    fn run(&self, entry: &str, probe: &[&str]) -> Output {
        match entry {
            "gate shim" if probe[0] == "cargo" => self.command(self.home.join(".local/bin/cargo")).args(&probe[1..]).output().unwrap(),
            "gate shim" => self.north(&["capacity", "gate"], probe),
            "capacity host run" => self.north(&["capacity", "host", "run", "--class", "heavy", "--owner", "carry-matrix", "--timeout-seconds", "30", "--"], probe),
            "capacity exec --dev-shell" => self.north(&["capacity", "exec", "--dev-shell", self.project.to_str().unwrap(), "--"], probe),
            "offload run" => {
                let line = probe.iter().map(|word| format!("'{}'", word.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ");
                self.command(env!("CARGO_BIN_EXE_north")).args(["capacity", "offload", "run", "--host", "gex45", "--base", &self.base, "--head", &self.head, "--timeout", "30", "--", &line]).env("SAFE_PUSH_LANDING", "1").output().unwrap()
            }
            "land-shell --dev" => self.sandboxed("land-shell", probe),
            "depbox run" => self.sandboxed("depbox", probe),
            _ => unreachable!("{entry}"),
        }
    }
}

const ENTRIES: [&str; 6] = ["gate shim", "capacity host run", "capacity exec --dev-shell", "offload run", "land-shell --dev", "depbox run"];

fn stdout_lines(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout).lines().filter(|line| !line.starts_with("offload: ")).map(str::to_owned).collect()
}

#[test]
fn carry_matrix_every_entry_runs_the_dev_shell_cargo_sees_the_landing_base_and_exits_127_for_a_missing_program() {
    let matrix = Matrix::new();
    let cargo = matrix.toolchain.join("cargo").display().to_string();
    let mut wrong = Vec::new();
    for entry in ENTRIES {
        for (probe, expected) in [(&["cargo"][..], &cargo), (&["sh", "-c", "exec \"$(command -v cargo)\""][..], &cargo), (&["sh", "-c", "printenv SAFE_PUSH_LAND_BASE"][..], &matrix.base)] {
            let output = matrix.run(entry, probe);
            let seen = stdout_lines(&output);
            if !output.status.success() || seen != [expected.clone()] {
                wrong.push(format!("{entry} {probe:?}: exit {:?}, stdout {seen:?}, stderr {}", output.status.code(), String::from_utf8_lossy(&output.stderr).trim()));
            }
        }
        let missing = matrix.run(entry, &[MISSING, "--version"]);
        if missing.status.code() != Some(127) {
            wrong.push(format!("{entry} [{MISSING}]: exit {:?}, not 127; stderr {}", missing.status.code(), String::from_utf8_lossy(&missing.stderr).trim()));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_depbox_run_inside_a_landing_sees_the_packages_cpus_integration_flag_and_runtime_head() {
    let matrix = Matrix::new();
    let output = matrix.sandboxed_landing(&["sh", "-c", "printf '%s|%s|%s|%s\\n' \"$SAFE_PUSH_LAND_PACKAGES\" \"$SAFE_PUSH_LAND_CPUS\" \"$SAFE_PUSH_LAND_IT\" \"$SAFE_PUSH_LAND_RUNTIME_HEAD\""]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(stdout_lines(&output), [format!("-p north-core|3|1|{}", matrix.head)]);
}
