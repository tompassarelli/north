use toml_edit::DocumentMut;

use super::fallbacks;

const HOSTS: &str = "[hosts.gex45]\nssh = \"tom@gex45\"\n\n[hosts.cpx62]\nssh = \"tom@cpx62\"\n\n[hosts.whiterabbit]\nreaches = [\"gex45\", \"cpx62\"]\n";

// KEEP(bug): on 2026-11-08 between 5:30 and 6:00 AM, 17 land checks batched onto cpx62 ran their mutant kill against .safe-push's landing.remote-hosts gex45, which cpx62 cannot reach ("ssh: Could not resolve hostname gex45"), so each reached no verdict 0.3 s in (north#706)
#[test]
fn a_mutant_kill_inside_an_offload_slot_runs_on_the_slot_host_and_tries_only_hosts_it_reaches() {
    let cfg: DocumentMut = HOSTS.parse().unwrap();
    assert_eq!(fallbacks(true, || Some("gex45".to_owned()), "gex45", Some(&cfg), "cpx62"), ["cpx62"]);
    assert!(fallbacks(false, || None, "gex45", Some(&cfg), "cpx62").is_empty());
    assert_eq!(fallbacks(false, || None, "gex45", Some(&cfg), "whiterabbit"), ["gex45"]);
}
