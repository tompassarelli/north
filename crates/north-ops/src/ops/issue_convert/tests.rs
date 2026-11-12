use std::collections::BTreeSet;

use super::boxes;
use super::closed::{self, Leftover, OpenCopy};
use super::manifest::{self, Manifest, Parent, Row, Scan, Tracker, Verdict, WAITS};
use crate::threads::claims;

const BODY: &str = "Class: prose above the boxes.

## Done when

- [ ] Grounding reaches every worker at launch: the brief carries the pack. Done when: `north fleet briefs --share` exits 0. Needs: none
- [x] 0. Already built. Done when: `true` exits 0.
- [ ] 3a. The split is run on five tasks (see the comment).
  - [ ] The coordinator runs the split outside a sandbox. Done when: `north graph split --check` exits 0.
  - [ ] A second pass covers the two change sets. Category: planning
- [ ] The eval prints seconds before the first edit. Done when: `north eval summary` exits 0. Needs: native
  - [ ] the table is pasted here
  one more line of the eval box
- [ ] Decision for the owner: whether leaves are created before the showing.
- [ ] The claimed box stays a line in the body. Done when: `true` exits 0.
* [ ] x
";

const LABELS: [&str; 4] = ["area:fleet", "category:feature", "category:planning", WAITS];

fn checked(manifest: &mut Manifest, parent: &Parent, open_titles: &[&str], scan: &Scan) {
    let titles: BTreeSet<String> = open_titles.iter().map(|title| manifest::title_key(title)).collect();
    let labels: BTreeSet<String> = LABELS.iter().map(|label| (*label).to_owned()).collect();
    manifest.check(parent, &Tracker { titles: &titles, labels: Some(&labels), scan });
}

fn flags(manifest: &Manifest) -> Vec<(usize, String)> {
    manifest.flagged().map(|row| (row.row, row.flags.join("; "))).collect()
}

#[test]
fn a_dry_run_names_every_open_box_exactly_once() {
    let labels = ["area:fleet".to_owned(), "category:feature".to_owned(), "priority:now".to_owned()];
    let parent = Parent { reference: "tompassarelli/north#7", body: BODY, labels: &labels, children: 0 };
    let claimed = [Some("the-claimed-box-stays".to_owned())];
    let clean = |_: &str| -> Result<BTreeSet<String>, String> { Ok(BTreeSet::new()) };
    let mut manifest = manifest::draft(&parent, &claimed, &[]);
    checked(&mut manifest, &parent, &["An open issue's title"], &clean);

    let open: Vec<String> = boxes::checkboxes(BODY).into_iter().filter(|checkbox| !checkbox.ticked).map(|checkbox| checkbox.first).collect();
    assert_eq!(manifest.rows.iter().map(|row| row.source.clone()).collect::<Vec<_>>(), open);
    assert_eq!((manifest.rows.len(), manifest.open_boxes), (9, boxes::unticked(BODY)));
    assert_eq!(manifest.refused, Vec::<String>::new());
    let shape: Vec<(Verdict, Option<usize>, &str, &str)> = manifest.rows.iter().map(|row| (row.verdict, row.under, row.title.as_str(), row.kind.as_str())).collect();
    assert_eq!(
        shape,
        [
            (Verdict::Leaf, None, "Grounding reaches every worker at launch", "Task"),
            (Verdict::ThirdLevel, None, "The split is run on five tasks", "Feature"),
            (Verdict::Leaf, Some(2), "The coordinator runs the split outside a sandbox", "Task"),
            (Verdict::Leaf, Some(2), "A second pass covers the two change sets", "Task"),
            (Verdict::Leaf, None, "The eval prints seconds before the first edit", "Task"),
            (Verdict::Prose, Some(5), "", ""),
            (Verdict::Gate, None, "", ""),
            (Verdict::Claimed, None, "", ""),
            (Verdict::Leaf, None, "", "Task"),
        ]
    );
    assert_eq!(manifest.rows[0].labels, ["area:fleet", "category:feature"]);
    assert_eq!(manifest.rows[3].labels, ["area:fleet", "category:planning"]);
    assert_eq!(manifest.rows[4].labels, ["area:fleet", "category:feature", WAITS]);
    let slug = claims::box_slug("The eval prints seconds before the first edit. Done when: `north eval summary` exits 0. Needs: native").unwrap_or_default();
    assert_eq!(
        manifest.rows[4].body,
        format!("The eval prints seconds before the first edit.\n\nDone when: `north eval summary` exits 0.\nNeeds: native\none more line of the eval box\n- the table is pasted here\n\nConverted from tompassarelli/north#7, box `{slug}`.")
    );
    let found = flags(&manifest);
    assert_eq!(found.iter().map(|(row, _)| *row).collect::<Vec<_>>(), [9]);
    assert!(found[0].1.contains("no title") && found[0].1.contains("no runnable Done when"), "{found:?}");
    assert!(manifest.count_line().starts_with("tompassarelli/north#7: 9 open boxes, 9 rows: 5 leaf, 1 third-level issue, 1 prose, 1 gate, 1 held by a live claim; 1 rows flagged"), "{}", manifest.count_line());

    let mut edited: Vec<Row> = manifest.rows.clone();
    edited[8].verdict = Verdict::Prose;
    edited[0].title = "Every worker's brief carries the grounding pack".to_owned();
    let mut again = manifest::draft(&parent, &claimed, &edited);
    checked(&mut again, &parent, &[], &clean);
    assert!(again.passes(), "{:?} {:?}", again.refused, flags(&again));
    assert_eq!((again.rows.len(), again.rows[0].title.as_str(), again.rows[8].verdict), (9, "Every worker's brief carries the grounding pack", Verdict::Prose));

    let mut short = again.clone();
    short.rows.remove(2);
    checked(&mut short, &parent, &[], &clean);
    assert!(short.refused.iter().any(|refusal| refusal.starts_with("8 rows for the 9 open boxes")), "{:?}", short.refused);
    assert!(!short.passes());

    let mut long = again.clone();
    long.rows[2].title = "t".repeat(81);
    long.rows[3].title = "an OPEN   issue's title".to_owned();
    long.rows[4].title = "3a. A title that still carries its letter code".to_owned();
    checked(&mut long, &parent, &["An open issue's title"], &clean);
    assert_eq!(
        flags(&long),
        [(3, "its title is 81 characters and the limit is 80".to_owned()), (4, "its title equals an open issue's".to_owned()), (5, "its title starts with a letter code".to_owned())]
    );

    let private = |text: &str| -> Result<BTreeSet<String>, String> { Ok(if text.contains("grounding pack") { BTreeSet::from(["host-name".to_owned()]) } else { BTreeSet::new() }) };
    let mut scanned = again.clone();
    checked(&mut scanned, &parent, &[], &private);
    assert_eq!(flags(&scanned), [(1, "the privacy scan refuses it (deny-list rule host-name)".to_owned())]);
}

#[test]
fn a_closed_issue_s_unticked_box_is_listed_with_the_open_issue_that_holds_its_text() {
    let closed = [(634, "- [ ] The lander records each span of a landing in its ledger. Done when: `north land spans` exits 0.\n- [x] A ticked box is no leftover.\n- [ ] A box nobody copied"), (640, "No box here.")];
    let open = [
        (900, "- [ ] The lander records nothing of the kind"),
        (902, "- [x] The lander records each span of a landing in its ledger. Done when: `north land spans` exits 0. Evidence: `north land spans` passed on main 0123456789ab on 2026-11-10, exit 0."),
    ];
    assert_eq!(
        closed::leftovers(&closed, &open),
        [
            Leftover { issue: 634, text: "The lander records each span of a landing in its ledger. Done when: `north land spans` exits 0.".to_owned(), copy: Some(OpenCopy { issue: 902, ticked: true }) },
            Leftover { issue: 634, text: "A box nobody copied".to_owned(), copy: None },
        ]
    );
}
