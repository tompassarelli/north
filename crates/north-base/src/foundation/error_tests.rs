use super::*;

#[test]
fn generated_authorization_messages_do_not_disclose_payloads() {
    let mut inputs = crate::test_inputs::Generator::new(0x41555448);
    let public = NorthError::Protocol("401 Unauthorized".into()).user_message();
    for case in 0..crate::test_inputs::cases() {
        let secret = format!("private-{}-{}", inputs.word(), inputs.word());
        let context = inputs.word();
        for payload in [
            format!("{secret}: 401 Unauthorized: {context}"),
            format!("turn ended as failed: {context}; 401 Unauthorized: {secret}"),
            format!("401 Unauthorized\n{secret}\n{context}"),
        ] {
            let message = NorthError::Protocol(payload).user_message();
            assert!(!message.contains(&secret), "case {case}: payload disclosed");
            assert_eq!(message, public, "case {case}: payload changed the public message");
        }
    }
}
