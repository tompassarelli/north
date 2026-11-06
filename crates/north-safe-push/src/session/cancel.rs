use north_base::foundation::step::request_cancel;

pub fn watch_for_cancel() {
    std::thread::spawn(|| {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
        runtime.block_on(async {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut interrupt), Ok(mut terminate), Ok(mut hangup)) =
                (signal(SignalKind::interrupt()), signal(SignalKind::terminate()), signal(SignalKind::hangup()))
            else {
                return;
            };
            loop {
                tokio::select! {
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => {}
                    _ = hangup.recv() => {}
                }
                request_cancel();
            }
        });
    });
}
