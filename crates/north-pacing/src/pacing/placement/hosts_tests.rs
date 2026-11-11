use super::*;

fn machine(name: &str) -> Machine {
    Machine { name: name.into(), ssh: name.into(), serves: Vec::new(), pushes: None, reaches: None, launches: true, cap: None, live: None, accounts: Vec::new(), claude_ai: Vec::new(), saturation: None, stale_age_s: None, placed_recently: 0 }
}

static MEMO: Memo = Mutex::new(None);

#[test]
fn one_hung_host_is_placed_from_its_last_reading_when_the_deadline_passes() {
    *MEMO.lock().unwrap() = Some(HashMap::new());
    let (release, hung) = mpsc::channel::<()>();
    let hung = Mutex::new(hung);
    let ask: Ask = Arc::new(move |name, _, _, _| {
        if name == "hung" {
            let _ = hung.lock().unwrap().recv();
        }
        Facts { live: Some(1), ..Facts::default() }
    });
    let silent = |_: &Machine| Facts { live: Some(4), stale_age_s: Some(45.0), ..Facts::default() };
    let records = tempfile::tempdir().unwrap();
    let facts = survey_via(&MEMO, records.path(), &[machine("hung"), machine("fine")], None, Duration::from_millis(100), ask, &silent);
    assert_eq!(facts[0].stale_age_s, Some(45.0));
    assert_eq!(facts[1].stale_age_s, None);
    assert_eq!(facts[1].live, Some(1));
    drop(release);
}

static UNREMEMBERED: Memo = Mutex::new(None);

#[test]
fn five_launches_started_together_share_one_answer_from_a_host_that_answers_one_at_a_time() {
    let records = tempfile::tempdir().unwrap();
    let dir = records.path().to_path_buf();
    let asked = Arc::new(Mutex::new(0));
    let host = Arc::new(Mutex::new(()));
    let ask: Ask = {
        let (asked, dir) = (Arc::clone(&asked), dir.clone());
        Arc::new(move |name, _, _, question| {
            if question == Asked::Directory {
                return Facts { has_cwd: Some(true), ..Facts::default() };
            }
            let _one_at_a_time = host.lock().unwrap();
            *asked.lock().unwrap() += 1;
            let reading = Saturation { memory_total_mib: 64.0, memory_available_mib: 32.0, disk_free_gib: 500.0, disk_total_gib: 900.0, backlog: None, token_age_s: None, disk_bound_gib: None };
            let facts = Facts { live: Some(0), saturation: Some(reading), saturation_age_s: Some(0.0), has_cwd: Some(true), ..Facts::default() };
            record(&dir, name, &Facts { has_cwd: None, ..facts.clone() });
            facts
        })
    };
    let launches: Vec<_> = (0..5)
        .map(|_| {
            let (ask, dir) = (Arc::clone(&ask), dir.clone());
            std::thread::spawn(move || {
                let silent = |_: &Machine| Facts { stale_age_s: Some(1.0), ..Facts::default() };
                survey_via(&UNREMEMBERED, &dir, &[machine("gex45")], Some("/work"), Duration::from_secs(5), ask, &silent)
            })
        })
        .collect();
    let finished: Vec<_> = launches.into_iter().map(|launch| launch.join().unwrap()).collect();
    assert_eq!(*asked.lock().unwrap(), 1, "one answer serves every launch that starts while it is fresh");
    for facts in finished {
        assert_eq!((facts[0].live, facts[0].has_cwd, facts[0].stale_age_s), (Some(0), Some(true), None));
    }
}

#[test]
fn a_launch_left_past_the_deadline_behind_a_burst_does_not_read_the_host_as_lacking_its_directory() {
    let records = tempfile::tempdir().unwrap();
    let reading = Facts { live: Some(0), accounts: vec!["d".into()], ..Facts::default() };
    record(records.path(), "gex45", &reading);
    let (release, busy) = mpsc::channel::<()>();
    let busy = Mutex::new(busy);
    let ask: Ask = Arc::new(move |_, _, _, _| {
        let _ = busy.lock().unwrap().recv();
        Facts::default()
    });
    let silent = |machine: &Machine| last_recorded(records.path(), &machine.name);
    let facts = survey_via(&UNREMEMBERED, records.path(), &[machine("gex45")], Some("/tmp/eval-b/worktrees/t01-l21"), Duration::from_millis(100), ask, &silent);
    drop(release);
    assert!(facts[0].stale_age_s.is_some(), "the host is placed from its last reading");
    assert_eq!(facts[0].has_cwd, None, "a reading that never asked about the directory says nothing of it, so the launch is not kept on the control host");
}
