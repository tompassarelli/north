use crate::threads::runs::brief::CATEGORIES;

const INFRA: [&str; 1] = ["tooling"];
const BUG: [&str; 2] = ["bug-known-cause", "debugging-unknown-cause"];

pub fn skeleton(category: &str) -> Option<String> {
    CATEGORIES.contains(&category).then(|| {
        let mut lines = vec![
            "Item: <repo#number and title>".to_owned(),
            format!("Category: {category}"),
        ];
        if BUG.contains(&category) {
            lines.push("Class: <the class of failure, so the Done when covers every instance>".into());
        }
        lines.extend(
            [
                "Spec: exact",
                "Scope: module",
                "Surface: shell",
                "Verify: local-test",
                "Done when: `<command that proves it>`",
                "ETA: <minutes> min",
                "Files: <paths>",
            ]
            .map(str::to_owned),
        );
        if INFRA.contains(&category) {
            lines.push("Payback: saves <N> min/change (Tom <a>, agent <b>), <C> changes/day, cost <M> min, payback <D> days".into());
        }
        lines.push(String::new());
        lines.push("<Goal, Sources and the state checked at spawn>".into());
        lines.join("\n") + "\n"
    })
}

pub fn run(arguments: &[String]) -> u8 {
    let category = match arguments {
        [new, flag, category] if new == "new" && flag == "--category" => category,
        _ => {
            eprintln!("usage: north brief new --category <{}>", CATEGORIES.join("|"));
            return 2;
        }
    };
    match skeleton(category) {
        Some(text) => {
            print!("{text}");
            0
        }
        None => {
            eprintln!("north brief: unknown category {category}; use one of {}", CATEGORIES.join(" | "));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_skeleton_filled_with_placeholders_passes_the_brief_guard() {
        for category in CATEGORIES {
            let text = skeleton(category).unwrap();
            let filled = regex::Regex::new("<[^>]*>").unwrap().replace_all(&text, "1").into_owned();
            let kind = if category == "mechanical" { "worker-haiku" } else { "worker" };
            assert_eq!(super::super::worker_tier::brief_refusal(kind, false, false, &filled), None, "{category}:\n{filled}");
            assert!(crate::router::brief::class_refusal(&filled).is_none(), "{category}");
        }
    }

    #[test]
    fn an_unknown_category_has_no_skeleton() {
        assert_eq!(skeleton("invented"), None);
    }
}
