#[test]
fn planted_hung_case_waits_forever() {
    let (_sender, receiver) = std::sync::mpsc::channel::<()>();
    let _ = receiver.recv();
}
