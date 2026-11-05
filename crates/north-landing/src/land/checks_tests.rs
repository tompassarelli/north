use super::unfit_host;

#[test]
fn a_crate_the_host_lacks_offline_is_an_unfit_host() {
    let log = "Compiling x\nerror: failed to download `ahash v0.8.12`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\nsafe-push: verdict refused\n";
    assert_eq!(unfit_host(log).as_deref(), Some("error: failed to download `ahash v0.8.12`"));
    assert_eq!(unfit_host("error: failed to download `ahash v0.8.12`\nCaused by: HTTP 502\n"), None, "a download that failed online is not this signature");
}

