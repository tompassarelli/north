use crate::capacity_host::{Host, SCOPE_SYSTEMCTL, scope_systemd_run};
use north_core::foundation::shim;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use sha1::{Digest, Sha1};
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

const FLAKE: (&str, &str) = ("flake.nix", "{ outputs = _: { devShells = { }; }; }\n");
const IDENTITY: &str = "Carry matrix <carry@example.invalid> 1700000000 +0000";

fn hex(id: &[u8; 20]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn stored_zlib(data: &[u8]) -> Vec<u8> {
    // Git reads loose objects as zlib streams, and stored deflate blocks need no compressor.
    let mut stream = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = data.chunks(0xffff).collect();
    for (index, block) in blocks.iter().enumerate() {
        let length = block.len() as u16;
        stream.push(u8::from(index + 1 == blocks.len()));
        stream.extend(length.to_le_bytes());
        stream.extend((!length).to_le_bytes());
        stream.extend_from_slice(block);
    }
    let (low, high) = data.iter().fold((1u32, 0u32), |(low, high), &byte| {
        let low = (low + u32::from(byte)) % 65521;
        (low, (high + low) % 65521)
    });
    stream.extend(((high << 16) | low).to_be_bytes());
    stream
}

fn object(git_dir: &Path, kind: &str, body: &[u8]) -> [u8; 20] {
    let mut raw = format!("{kind} {}\0", body.len()).into_bytes();
    raw.extend_from_slice(body);
    let id: [u8; 20] = Sha1::digest(&raw).into();
    let name = hex(&id);
    let path = git_dir.join("objects").join(&name[..2]).join(&name[2..]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, stored_zlib(&raw)).unwrap();
    id
}

fn index(git_dir: &Path, entries: &[(&str, [u8; 20], usize)]) {
    let mut bytes = b"DIRC".to_vec();
    bytes.extend(2u32.to_be_bytes());
    bytes.extend((entries.len() as u32).to_be_bytes());
    for (name, id, size) in entries {
        let start = bytes.len();
        bytes.extend([0u8; 24]);
        bytes.extend(0o100644u32.to_be_bytes());
        bytes.extend([0u8; 8]);
        bytes.extend((*size as u32).to_be_bytes());
        bytes.extend_from_slice(id);
        bytes.extend((name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.resize(start + (bytes.len() - start + 8) / 8 * 8, 0);
    }
    let checksum: [u8; 20] = Sha1::digest(&bytes).into();
    bytes.extend(checksum);
    fs::write(git_dir.join("index"), bytes).unwrap();
}

fn repository(worktree: &Path, history: &[(&str, &[(&str, &str)])], origin: Option<&Path>) -> Vec<String> {
    let git_dir = worktree.join(".git");
    for directory in ["objects/info", "objects/pack", "refs/heads", "refs/tags", "refs/remotes/origin"] {
        fs::create_dir_all(git_dir.join(directory)).unwrap();
    }
    let (mut commits, mut entries) = (Vec::new(), Vec::new());
    for (message, files) in history {
        entries = files.iter().map(|(name, content)| (*name, object(&git_dir, "blob", content.as_bytes()), content.len())).collect();
        let tree: Vec<u8> = entries.iter().flat_map(|(name, id, _)| [format!("100644 {name}\0").as_bytes(), id.as_slice()].concat()).collect();
        let tree = hex(&object(&git_dir, "tree", &tree));
        let parent = commits.last().map(|parent| format!("parent {parent}\n")).unwrap_or_default();
        commits.push(hex(&object(&git_dir, "commit", format!("tree {tree}\n{parent}author {IDENTITY}\ncommitter {IDENTITY}\n\n{message}\n").as_bytes())));
    }
    let tip = commits.last().unwrap();
    for (name, content) in history.last().unwrap().1 {
        fs::write(worktree.join(name), content).unwrap();
    }
    index(&git_dir, &entries);
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(git_dir.join("refs/heads/main"), format!("{tip}\n")).unwrap();
    let mut config = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n".to_owned();
    if let Some(origin) = origin {
        fs::write(git_dir.join("refs/remotes/origin/main"), format!("{tip}\n")).unwrap();
        config.push_str(&format!("[remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n", origin.display()));
    }
    fs::write(git_dir.join("config"), config).unwrap();
    commits
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
        symlink(crate::support::bin_exe!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
        shim::copy_executable(crate::support::manifest_dir().join("bin/cargo"), bin.join("cargo"));
        shim::write_script(&bin.join("nix"), &FAKE_NIX.replace("@TOOLCHAIN@", &toolchain.display().to_string()));
        shim::write_script(&bin.join("ssh"), FAKE_SSH);
        shim::write_script(&toolchain.join("cargo"), "echo \"$0\"\n");
        fs::write(host.path("orchestration.toml"), "[hosts.lane-host]\nreaches = [\"gex45\"]\n[hosts.gex45]\nssh = \"tom@gex45\"\n").unwrap();
        let [base, head] = <[String; 2]>::try_from(repository(&project, &[("Base", &[FLAKE]), ("Lane", &[("change", "lane\n"), FLAKE])], None)).unwrap();
        let remote = host.path("remotes/gex45/code/proj/main");
        fs::create_dir_all(&remote).unwrap();
        assert_eq!(repository(&remote, &[("Base", &[FLAKE])], Some(&project)), std::slice::from_ref(&base));
        Matrix { host, home, project, toolchain, base, head }
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = self.host.command(program);
        let path = format!("{}:{}", self.host.path("bin").display(), self.home.join(".local/bin").display());
        command
            .current_dir(&self.project)
            .env("PATH", path)
            .env("RESIDENT_CAP_HOST", "lane-host")
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("SAFE_PUSH_LAND_BASE", &self.base)
            .env("FAKE_REMOTE_HOMES", self.host.path("remotes"))
            .env("NORTH_OFFLOAD_TARGET_FLOOR_MIB", "0")
            .env("AGENT_CAPACITY_SETTLE_SECONDS", "0")
            .env("AGENT_CAPACITY_CORES", "96")
            .env_remove("NORTH_DEV_SHELL")
            .env_remove("IN_NIX_SHELL")
            .env_remove("DEPBOX")
            .env_remove("AGENT_CAPACITY_LEASE")
            .env_remove("CARGO_TARGET_DIR");
        command
    }

    fn north(&self, arguments: &[&str], probe: &[&str]) -> Output {
        self.command(crate::support::bin_exe!("CARGO_BIN_EXE_north")).args(arguments).args(probe).output().unwrap()
    }

    fn sandboxed(&self, script: &str, probe: &[&str]) -> Output {
        let mut command = self.command(crate::support::manifest_dir().join("bin").join(script));
        command.args(if script == "land-shell" { ["--dev", "--"] } else { ["run", "--"] }).args(probe).output().unwrap()
    }

    fn run(&self, entry: &str, probe: &[&str]) -> Output {
        match entry {
            "gate shim" if probe[0] == "cargo" => self.command(self.home.join(".local/bin/cargo")).args(&probe[1..]).output().unwrap(),
            "gate shim" => self.north(&["capacity", "gate"], probe),
            "capacity host run" => self.north(&["capacity", "host", "run", "--class", "heavy", "--owner", "carry-matrix", "--timeout-seconds", "30", "--"], probe),
            "capacity exec --dev-shell" => self.north(&["capacity", "exec", "--dev-shell", self.project.to_str().unwrap(), "--"], probe),
            "offload run" => {
                let line = probe.iter().map(|word| format!("'{}'", word.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ");
                self.command(crate::support::bin_exe!("CARGO_BIN_EXE_north")).args(["capacity", "offload", "run", "--host", "gex45", "--base", &self.base, "--head", &self.head, "--timeout", "30", "--", &line]).env("SAFE_PUSH_LANDING", "1").output().unwrap()
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
    let probes: [(&[&str], Option<&String>); 4] = [(&["cargo"], Some(&cargo)), (&["sh", "-c", "exec \"$(command -v cargo)\""], Some(&cargo)), (&["sh", "-c", "printenv SAFE_PUSH_LAND_BASE"], Some(&matrix.base)), (&[MISSING, "--version"], None)];
    let check = |entry: &str, probe: &[&str], expected: Option<&String>| -> Option<String> {
        let output = matrix.run(entry, probe);
        match expected {
            Some(expected) => {
                let seen = stdout_lines(&output);
                (!output.status.success() || seen != [expected.clone()]).then(|| format!("{entry} {probe:?}: exit {:?}, stdout {seen:?}, stderr {}", output.status.code(), String::from_utf8_lossy(&output.stderr).trim()))
            }
            None => (output.status.code() != Some(127)).then(|| format!("{entry} [{MISSING}]: exit {:?}, not 127; stderr {}", output.status.code(), String::from_utf8_lossy(&output.stderr).trim())),
        }
    };
    let wrong: Vec<String> = std::thread::scope(|scope| {
        let check = &check;
        let spawn = |entry: &'static str| probes.iter().map(move |&(probe, expected)| scope.spawn(move || check(entry, probe, expected))).collect::<Vec<_>>();
        let mut runs = spawn("offload run");
        let prepared = matrix.north(&["capacity", "dev-shell", "run", "--prepare"], &[]);
        assert!(prepared.status.success(), "dev-shell run --prepare: {}", String::from_utf8_lossy(&prepared.stderr));
        runs.extend(ENTRIES.into_iter().filter(|entry| *entry != "offload run").flat_map(spawn));
        runs.into_iter().filter_map(|run| run.join().unwrap()).collect()
    });
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
