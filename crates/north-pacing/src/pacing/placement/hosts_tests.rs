use super::*;

fn machine(name: &str) -> Machine {
    Machine { name: name.into(), ssh: name.into(), serves: Vec::new(), pushes: None, reaches: None, launches: true, cap: None, live: None, accounts: Vec::new(), claude_ai: Vec::new(), saturation: None, stale_age_s: None }
}

static MEMO: Memo = Mutex::new(None);

#[test]
fn one_hung_host_is_placed_from_its_last_reading_when_the_deadline_passes() {
    *MEMO.lock().unwrap() = Some(HashMap::new());
    let (release, hung) = mpsc::channel::<()>();
    let hung = Mutex::new(hung);
    let ask: Ask = Arc::new(move |name, _, _| {
        if name == "hung" {
            let _ = hung.lock().unwrap().recv();
        }
        Facts { live: Some(1), ..Facts::default() }
    });
    let silent = |_: &Machine, _: Option<&str>| Facts { live: Some(4), stale_age_s: Some(45.0), ..Facts::default() };
    let facts = survey_via(&MEMO, &[machine("hung"), machine("fine")], None, Duration::from_millis(100), ask, &silent);
    assert_eq!(facts[0].stale_age_s, Some(45.0));
    assert_eq!(facts[1].stale_age_s, None);
    assert_eq!(facts[1].live, Some(1));
    drop(release);
}
