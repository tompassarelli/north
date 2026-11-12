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

static GUARDED: Memo = Mutex::new(None);

/// On 2026-11-12 a launch placed work from a host's answer two minutes old, so a host whose disk or memory ran short since then still took it.
#[test]
fn a_launch_after_an_answer_minutes_old_asks_the_host_again_before_it_places_work_there() {
    let records = tempfile::tempdir().unwrap();
    let reading = Saturation { memory_total_mib: 64.0, memory_available_mib: 32.0, disk_free_gib: 500.0, disk_total_gib: 900.0, backlog: None, token_age_s: None, disk_bound_gib: None };
    let old = Facts { live: Some(2), saturation: Some(reading), saturation_age_s: Some(0.0), ..Facts::default() };
    std::fs::write(records.path().join("gex45.json"), json!({"at": storage::now() - 120.0, "facts": old}).to_string()).unwrap();
    let ask: Ask = Arc::new(move |_, _, _, _| Facts { live: Some(3), saturation: Some(Saturation { disk_free_gib: 1.0, ..reading }), saturation_age_s: Some(0.0), ..Facts::default() });
    let silent = |_: &Machine| Facts::default();
    let facts = survey_via(&GUARDED, records.path(), &[machine("gex45")], Some("/work"), Duration::from_secs(5), ask, &silent);
    assert_eq!((facts[0].live, facts[0].saturation.map(|now| now.disk_free_gib)), (Some(3), Some(1.0)));
}
