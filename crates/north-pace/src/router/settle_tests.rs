use super::settle::{self, Arm, FLOOR, Outcome, Side, SplitMix, family, shares};

fn real_boxes() -> Vec<Outcome> {
    include_str!("fixtures/closed_box_costs.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let (usd, kept) = line.split_once(' ').unwrap();
            Outcome { usd: usd.parse().unwrap(), closed: true, kept: kept == "1" }
        })
        .collect()
}

fn drawn(sample: &[Outcome], count: usize, scale: f64, random: &mut SplitMix) -> Vec<Outcome> {
    (0..count).map(|_| sample[random.below(sample.len())]).map(|outcome| Outcome { usd: outcome.usd * scale, ..outcome }).collect()
}

fn flat(usd: f64, count: usize, kept: usize) -> Vec<Outcome> {
    (0..count).map(|index| Outcome { usd, closed: true, kept: index < kept }).collect()
}

#[test]
fn an_arm_is_settled_only_on_a_bootstrap_over_real_closed_box_costs_and_until_then_its_share_follows_thompson_sampling_with_a_floor() {
    let sample = real_boxes();
    assert_eq!(sample.len(), 177);
    let mut random = SplitMix(0x0636);
    let (mut dearer_settled, mut dearer_by_point, mut cheaper_settled) = (0, 0, 0);
    for _ in 0..1000 {
        let sizes = (15 + random.below(16), 15 + random.below(16));
        let cheap = drawn(&sample, sizes.0, 1.0, &mut random);
        let dear = drawn(&sample, sizes.1, 1.5, &mut random);
        match settle::settled(Arm { runs: sizes.1, boxes: &dear }, Arm { runs: sizes.0, boxes: &cheap }) {
            Some(Side::First) => dearer_settled += 1,
            Some(Side::Second) => cheaper_settled += 1,
            None => {}
        }
        dearer_by_point += usize::from(settle::usd_per_kept(&dear) <= settle::usd_per_kept(&cheap));
    }
    assert!(dearer_settled <= 50, "the bootstrap settled the 1.5 times dearer arm in {dearer_settled} of 1000 draws");
    assert!(dearer_by_point > 50, "a point difference settles the dearer arm in {dearer_by_point} of 1000 draws, so the sample does not tell the rules apart");
    assert!(cheaper_settled > dearer_settled, "the cheaper arm settled in {cheaper_settled} draws");

    let (cheap, dear) = (flat(5.0, 15, 15), flat(9.0, 15, 15));
    assert_eq!(settle::settled(Arm { runs: 15, boxes: &dear }, Arm { runs: 15, boxes: &cheap }), Some(Side::Second));
    assert_eq!(settle::settled(Arm { runs: 14, boxes: &dear }, Arm { runs: 15, boxes: &cheap }), None, "an arm one run short keeps sampling");
    assert_eq!(settle::settled(Arm { runs: 15, boxes: &dear }, Arm { runs: 15, boxes: &cheap[..14] }), None, "an arm one closed box short keeps sampling");
    assert_eq!(settle::settled(Arm { runs: 15, boxes: &dear }, Arm { runs: 15, boxes: &flat(3.0, 15, 13) }), None, "a cheaper arm that keeps fewer of its boxes does not settle");
    assert_eq!(settle::settled(Arm { runs: 15, boxes: &cheap }, Arm { runs: 15, boxes: &cheap }), None, "a tie is no evidence for either arm");

    assert_eq!(shares(&[&[], &[]], "feature tier"), (vec![0.5, 0.5], false), "two arms with no boxes start 50-50");
    let (small_cheap, small_dear) = ([3.0, 4.0, 5.0, 30.0].map(|usd| flat(usd, 1, 1)[0]), [6.0, 8.0, 10.0, 60.0].map(|usd| flat(usd, 1, 1)[0]));
    let (moved, settled) = shares(&[&small_cheap, &small_dear], "feature tier");
    assert!(!settled && moved[0] > 0.6 && moved[1] >= FLOOR && (moved[0] + moved[1] - 1.0).abs() < 1e-9, "{moved:?}");
    assert_eq!(shares(&[&small_cheap, &small_dear], "feature tier").0, moved, "the draw is seeded");
    let (floored, _) = shares(&[&flat(1.0, 4, 4), &flat(100.0, 4, 4)], "feature tier");
    assert!((floored[0] - (1.0 - FLOOR)).abs() < 1e-9 && (floored[1] - FLOOR).abs() < 1e-9, "an arm that loses every draw keeps the floor until the rule settles: {floored:?}");
    assert_eq!(shares(&[&flat(1.0, 15, 15), &flat(100.0, 15, 15)], "feature tier"), (vec![1.0, 0.0], true), "a settled comparison gives the winner every box");
    assert_eq!(
        ["worker", "sonnet-high", "effort-xhigh", "no-handoff", "handoff-150k", "context-300k", "climb-split-tooling"].map(family),
        ["tier", "tier", "effort", "handoff", "handoff", "context", "climb"],
        "each experiment splits its category's boxes on its own"
    );
}
