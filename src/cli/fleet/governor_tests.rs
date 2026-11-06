use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::json;

use super::*;

const NOW: f64 = 1_793_847_600.0;
const BOUNDS: Bounds = Bounds { drain_minutes: 30.0, launch_to_queued_minutes: 66.0, lanes_per_session: 1.18, pressure_basis_points: 2500.0 };

fn decided(done: &Path, name: &str, verdict: &str, ago_minutes: f64) {
    let path = done.join(format!("{name}.json"));
    fs::create_dir_all(done).unwrap();
    fs::write(&path, json!({"branch": name, "queued_at": "2026-11-05T10:00:00", "verdict": verdict}).to_string()).unwrap();
    OpenOptions::new().write(true).open(&path).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs_f64(NOW - ago_minutes * 60.0)).unwrap();
}

fn queued_at(ago_minutes: f64) -> String {
    let at = time::OffsetDateTime::from_unix_timestamp((NOW - ago_minutes * 60.0) as i64).unwrap();
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", at.year(), u8::from(at.month()), at.day(), at.hour(), at.minute(), at.second())
}

fn ticket(queue: &Path, micros: u128, index: usize, ago_minutes: f64) {
    fs::create_dir_all(queue).unwrap();
    fs::write(queue.join(format!("{micros}-lane-{index}.json")), json!({"branch": format!("lane-{index}"), "queued_at": queued_at(ago_minutes)}).to_string()).unwrap();
}

fn waiting(queue: &Path, index: usize, ago_minutes: f64) {
    ticket(queue, ((NOW - ago_minutes * 60.0) * 1e6) as u128, index, ago_minutes);
}

struct Queues {
    root: tempfile::TempDir,
}

impl Queues {
    fn new() -> Self {
        Self { root: tempfile::tempdir().unwrap() }
    }

    fn repo(&self, key: &str, queued: usize, landed_last_hour: usize) -> std::path::PathBuf {
        let queue = self.root.path().join(key);
        fs::create_dir_all(&queue).unwrap();
        for index in 0..landed_last_hour {
            decided(&queue.join("done"), &format!("landed-{index}"), "landed abc", index as f64 * 59.0 / landed_last_hour as f64);
        }
        for index in 0..queued {
            waiting(&queue, index, 5.0);
        }
        queue
    }
}

fn assessed(queue: &Path, load: Load) -> Assessment {
    assess("north", read_lander(queue, NOW), read_landings(&all_queues(queue), NOW), load, BOUNDS)
}

#[test]
fn a_home_with_no_lander_record_reads_as_not_stalled_and_refuses_nothing() {
    let queues = Queues::new();
    let queue = queues.repo("north", 2, 0);
    waiting(&queue, 9, 29_897_794.0);
    let assessment = assessed(&queue, Load::default());
    assert_eq!((assessment.lander.recorded, assessment.lander.queued), (false, 3));
    assert_eq!(assessment.refusal, None);
}

#[test]
fn a_jump_the_queue_ticket_with_a_low_id_queued_minutes_ago_does_not_trip_the_stall() {
    let queues = Queues::new();
    let queue = queues.repo("north", 1, 50);
    ticket(&queue, 1_699_999_999_999_970, 9, 4.0);
    let assessment = assessed(&queue, Load::default());
    assert_eq!((assessment.lander.queued, assessment.lander.oldest_pending_minutes), (2, Some(5.0)));
    assert_eq!(assessment.refusal, None);
}

#[test]
fn a_repository_is_named_by_its_checkout_not_its_origin_remote() {
    let scratch = tempfile::tempdir().unwrap();
    let main = scratch.path().join("code/makgora/main");
    let lane = scratch.path().join("code/makgora/worktrees/box");
    let git = |directory: &Path, args: &[&str]| assert!(Command::new("git").arg("-C").arg(directory).args(args).output().unwrap().status.success(), "{args:?}");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    git(&main, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["worktree", "add", "-q", "-b", "box", lane.to_str().unwrap()]);
    assert_eq!(checkout(&main).as_deref(), Some("makgora"));
    assert_eq!(checkout(&lane).as_deref(), Some("makgora"));
    assert_eq!(checkout(scratch.path()), None);
}

