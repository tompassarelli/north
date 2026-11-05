use super::{Breaches, Issue, STAFFED_ALARM, Tracker, alarm, declares_category};

#[derive(Default)]
struct Posted {
    created: Vec<(String, String)>,
}

impl Tracker for Posted {
    fn open(&self, _: &str) -> Result<Option<Issue>, String> {
        Ok(None)
    }

    fn body(&self, _: u64) -> Result<String, String> {
        Ok(String::new())
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        self.created.push((title.to_owned(), body.to_owned()));
        Ok(())
    }

    fn edit(&mut self, _: u64, _: &str) -> Result<(), String> {
        Ok(())
    }

    fn close(&mut self, _: u64, _: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn an_alarm_body_declaring_a_category_is_refused_on_every_tick_and_posts_nothing_while_one_without_opens_on_the_second() {
    for header in ["Category: tooling\nSpec: measured\n\nCost rose.", "Cost rose.\n- **category**: `tooling`"] {
        let (mut tracker, mut breaches) = (Posted::default(), Breaches::default());
        for _ in 0..3 {
            assert_eq!(alarm(&mut tracker, &mut breaches, "Cost rose", Some(header), "calm"), Err(STAFFED_ALARM.to_owned()));
        }
        assert!(tracker.created.is_empty() && breaches == Breaches::default(), "{header}");
    }

    let (mut tracker, mut breaches) = (Posted::default(), Breaches::default());
    alarm(&mut tracker, &mut breaches, "Cost rose", Some("Cost rose in sonnet tooling."), "calm").unwrap();
    assert!(tracker.created.is_empty());
    alarm(&mut tracker, &mut breaches, "Cost rose", Some("Cost rose in sonnet tooling."), "calm").unwrap();
    assert_eq!(tracker.created, [("Cost rose".to_owned(), "Cost rose in sonnet tooling.".to_owned())]);
    assert!(!declares_category("The category of each cell follows."), "prose naming a category is no facet line");
}
