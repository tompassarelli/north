use super::{reservation, storage};
use serde_json::json;

#[test]
fn concurrent_reservations_cannot_spend_the_same_remaining_grant_use() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    let at = 1800000000.0;
    let nodes =
        vec![json!({"name":"lead","conversation":"parent","provider":"claude","status":"working"})];
    let grant = json!({"account":"claude:a","lead":"lead","session":"parent","until":at+3600.0,"at":at,"uses":1,"task":"review","max_workers":8,"authority":"proxy"});
    storage::save(&path.join("pace-control.json"), &json!({"grants":[grant]})).unwrap();
    let admitted = std::thread::scope(|scope| {
        let jobs:Vec<_>=(0..8).map(|id|{let nodes=&nodes;scope.spawn(move || {
            let request=json!({"provider":"claude","account":"claude:a","session":"parent","task":"review","tool_use_id":format!("tool-{id}")});
            reservation::reserve(&path.join("usage-gate.json"),&request,nodes,||at).unwrap().is_none()
        })}).collect();
        jobs.into_iter()
            .map(|job| usize::from(job.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(admitted, 1);
    assert_eq!(
        storage::load(&path.join("pace-control.json"))["grants"][0]["pending"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
