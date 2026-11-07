use super::verdict::{Facts, Verdict, retirement_verdict};

fn next(seed: i64) -> i64 {
    (1_103_515_245 * seed + 12_345) & 2_147_483_647
}

#[test]
fn retirement_gates_protection_monotonicity_and_force_delete_proof_hold() {
    let mut seed = 317;
    for trial in 0..64_i64 {
        seed = next(seed);
        let minimum = [0, 2, 24, 72][usize::try_from(seed % 4).unwrap()];
        let archive = [-1, 24, 72][usize::try_from((seed >> 3) % 3).unwrap()];
        let ages = [-1, 0, minimum - 1, minimum, minimum + 1, archive - 1, archive, archive + 1, 100];
        let age = ages[usize::try_from(trial % 9).unwrap()];
        let (merged, equivalent, evil) = ((seed >> 7) & 1 == 1, (seed >> 8) & 1 == 1, (seed >> 9) & 1 == 1);
        let protections = u32::try_from((seed >> 10) & 3).unwrap();
        let facts = Facts { age, minimum_age: minimum, merged, equivalent, evil_merge: evil, archive_age: (archive >= 0).then_some(archive), protections };
        let actual = retirement_verdict(&facts);
        let admissible = protections == 0 && age >= minimum && (merged || (equivalent && !evil) || (archive >= 0 && age >= archive));
        assert_eq!(actual != Verdict::Keep, admissible, "completeness seed={seed} trial={trial}");
        match actual {
            Verdict::Keep => {}
            Verdict::Retire => assert!(merged),
            Verdict::RetireEquivalent => assert!(equivalent && !evil && !merged, "force-delete seed={seed} trial={trial}"),
            Verdict::ArchiveThenDelete => assert!(!merged && (evil || !equivalent) && archive >= 0 && age >= archive),
        }
        for protection in 0..17 {
            let protected = retirement_verdict(&Facts { protections: protections | (1 << protection), ..facts });
            assert_eq!(protected, Verdict::Keep, "protection={protection} seed={seed} trial={trial}");
        }
    }
}
