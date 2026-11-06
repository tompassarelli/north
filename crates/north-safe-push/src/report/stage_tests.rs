use super::{Stage, stage_line, stage_row};

const BASH: &str = "safe-push: stage x y: 2.456 s (+3.206 s since start)\n1762430000\tnorth\tnorth-621-p8\tx y\t2456\t3206\n";

#[test]
fn stage_line_and_row_equal_what_bash_printed_for_the_same_times() {
    let stage = Stage {
        name: "x y",
        origin_url: "git@github.com:tompassarelli/north.git",
        branch: "north-621-p8",
        sha: "",
        started_ms: 1_762_430_000_250,
        previous_ms: 1_762_430_001_000,
        now_ms: 1_762_430_003_456,
    };
    assert_eq!(
        format!("{}\n{}", stage_line(&stage), stage_row(&stage)),
        BASH
    );
}
