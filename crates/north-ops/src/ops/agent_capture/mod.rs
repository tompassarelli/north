

use std::process::Command;

use serde_json::Value;

const USAGE: &str = "agent-capture: screenshot for agents that never touches Tom's clipboard.\n\n  north agent-capture --screen OUT.png            the focused output\n  north agent-capture --window ID OUT.png         one visible floating niri window (ID from `niri msg -j windows`);\n                                                  windows with app-id \"agent-client.*\" or title \"agent-client-*\" open floating\n  north agent-capture --region \"X,Y WxH\" OUT.png  a logical-pixel region\n\nBuilt on grim because niri's screenshot actions always replace the clipboard; niri reports positions only for visible floating windows.";

pub struct Programs {
    pub niri: String,
    pub grim: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Geometry {
    Missing,
    Hidden,
    At(String),
}

struct Failure(String, u8);

impl Programs {
    fn from_env() -> Self {
        let named = |name: &str, default: &str| std::env::var(name).ok().filter(|value| !value.is_empty()).unwrap_or_else(|| default.to_owned());
        Self { niri: named("AGENT_CAPTURE_NIRI", "niri"), grim: named("AGENT_CAPTURE_GRIM", "grim") }
    }

    fn niri_json(&self, what: &str, arguments: &[&str]) -> Result<Value, Failure> {
        let output = Command::new(&self.niri).args(["msg", "-j"]).args(arguments).output().ok().filter(|output| output.status.success());
        output.and_then(|output| serde_json::from_slice(&output.stdout).ok()).ok_or_else(|| Failure(format!("niri msg {what} failed"), 1))
    }

    fn grim(&self, arguments: &[&str]) -> Result<(), Failure> {
        match Command::new(&self.grim).args(arguments).status() {
            Ok(status) if status.success() => Ok(()),
            _ => Err(Failure("grim failed".to_owned(), 1)),
        }
    }
}

pub fn geometry(windows: &Value, workspaces: &Value, outputs: &Value, id: u64) -> Geometry {
    let Some(window) = windows.as_array().and_then(|all| all.iter().find(|window| window["id"] == id)) else { return Geometry::Missing };
    let Some(space) = workspaces.as_array().and_then(|all| all.iter().find(|space| space["id"] == window["workspace_id"])) else { return Geometry::Hidden };
    let layout = &window["layout"];
    let pair = |value: &Value| Some((value[0].as_f64()?, value[1].as_f64()?));
    let (true, Some((tile_x, tile_y))) = (space["is_active"] == true, pair(&layout["tile_pos_in_workspace_view"])) else { return Geometry::Hidden };
    let logical = &space["output"].as_str().map_or(&Value::Null, |name| &outputs[name])["logical"];
    let (Some((offset_x, offset_y)), Some((width, height)), Some(x), Some(y)) = (pair(&layout["window_offset_in_tile"]), pair(&layout["window_size"]), logical["x"].as_f64(), logical["y"].as_f64()) else { return Geometry::Hidden };
    Geometry::At(format!("{},{} {}x{}", (x + tile_x + offset_x).floor(), (y + tile_y + offset_y).floor(), width, height))
}

fn capture(arguments: &[String], programs: &Programs) -> Result<String, Failure> {
    let usage = |text: &str| Failure(format!("usage: {text}"), 2);
    match arguments.first().map(String::as_str) {
        Some("--screen") => {
            let out = arguments.get(1).filter(|out| !out.is_empty()).ok_or_else(|| usage("north agent-capture --screen OUT.png"))?;
            let focused = programs.niri_json("focused-output", &["focused-output"]).ok().and_then(|output| output["name"].as_str().map(str::to_owned)).ok_or_else(|| Failure("no focused output".to_owned(), 1))?;
            programs.grim(&["-o", &focused, out])?;
            Ok(out.clone())
        }
        Some("--window") => {
            let (Some(id), Some(out)) = (arguments.get(1).filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).and_then(|id| id.parse::<u64>().ok()), arguments.get(2).filter(|out| !out.is_empty())) else {
                return Err(usage("north agent-capture --window ID OUT.png"));
            };
            let found = geometry(&programs.niri_json("windows", &["windows"])?, &programs.niri_json("workspaces", &["workspaces"])?, &programs.niri_json("outputs", &["outputs"])?, id);
            match found {
                Geometry::Missing => Err(Failure(format!("no niri window with id {id}"), 1)),
                Geometry::Hidden => Err(Failure(format!("window {id} has no on-screen position (niri reports one only for visible floating windows; open it with app-id \"agent-client.<name>\" so it floats); capture its output with --screen or a --region"), 3)),
                Geometry::At(region) => programs.grim(&["-g", &region, out]).map(|()| out.clone()),
            }
        }
        Some("--region") => {
            let (Some(region), Some(out)) = (arguments.get(1).filter(|region| !region.is_empty()), arguments.get(2).filter(|out| !out.is_empty())) else {
                return Err(usage("north agent-capture --region \"X,Y WxH\" OUT.png"));
            };
            programs.grim(&["-g", region, out]).map(|()| out.clone())
        }
        Some(other) => Err(Failure(format!("unknown option: {other} (try --help)"), 2)),
        None => Err(Failure(USAGE.to_owned(), 2)),
    }
}

pub fn run_with(arguments: &[String], programs: &Programs, out: &mut dyn FnMut(&str), err: &mut dyn FnMut(&str)) -> u8 {
    if matches!(arguments.first().map(String::as_str), Some("-h" | "--help")) {
        out(USAGE);
        return 0;
    }
    match capture(arguments, programs) {
        Ok(path) => {
            out(&path);
            0
        }
        Err(Failure(message, code)) => {
            err(&format!("agent-capture: {message}"));
            code
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    run_with(arguments, &Programs::from_env(), &mut |line| println!("{line}"), &mut |line| eprintln!("{line}"))
}
