use super::*;

pub(crate) fn payload(uuid: u128, source: &str, decision: &str, replacement: Option<&str>) -> Vec<u8> {
    let uuid = HyphenatedUUID(uuid).to_string();
    let mut data = Vec::new();
    for field in [Some(uuid.as_str()), Some(source), Some(decision), replacement].into_iter().flatten() {
        assert!(field.is_ascii());
        data.extend_from_slice(&(field.len() as u16).to_be_bytes());
        data.extend_from_slice(field.as_bytes());
    }
    data
}

#[test]
fn chat_and_command_decisions_are_fifo_and_independent_in_both_arrival_orders() {
    for input_first in [false, true] {
        for (decision, replacement, expected) in [
            ("ALLOWED", None, Some("original")),
            ("CANCEL", None, None),
            ("MODIFY", Some("approved"), Some("approved")),
        ] {
            let mut session = Session::default();
            let data = payload(1, "CHAT_RESULT", decision, replacement);
            let result = if input_first {
                assert_eq!(session.submit("original".into()).unwrap(), None);
                session.receive(&data, 1).unwrap()
            } else {
                assert_eq!(session.receive(&data, 1).unwrap(), None);
                session.submit("original".into()).unwrap()
            };
            assert_eq!(result.as_deref(), expected);

            session.receive(&payload(1, "CHAT_RESULT", "CANCEL", None), 1).unwrap();
            session
                .receive(&payload(1, "COMMAND_RESULT", "MODIFY", Some("/load build.schem")), 1)
                .unwrap();
            assert_eq!(session.submit("/help".into()).unwrap().as_deref(), Some("//load build.schem"));
            assert_eq!(session.submit("blocked".into()).unwrap(), None);
            session.receive(&payload(1, "CHAT_RESULT", "ALLOWED", None), 1).unwrap();
            assert_eq!(session.submit("next".into()).unwrap().as_deref(), Some("next"));
        }
    }
}

#[test]
fn java_modified_utf8_handles_polish_text_emoji_and_nul() {
    // DataOutputStream.writeUTF("żółw 🐈\0"), including its byte length.
    let mut bytes = &b"\x00\x10\xc5\xbc\xc3\xb3\xc5\x82w \xed\xa0\xbd\xed\xb0\x88\xc0\x80"[..];
    assert_eq!(read_utf(&mut bytes).unwrap(), "żółw 🐈\0");
    assert!(bytes.is_empty());

    let mut data = payload(1, "CHAT_RESULT", "MODIFY", None);
    data.extend_from_slice(b"\x00\x0e\xc5\xbc\xc3\xb3\xc5\x82w \xed\xa0\xbd\xed\xb0\x88");
    let mut session = Session::default();
    session.receive(&data, 1).unwrap();
    assert_eq!(session.submit("original".into()).unwrap().as_deref(), Some("żółw 🐈"));
}

#[test]
fn invalid_payloads_cannot_change_another_player_or_execute_chat_as_a_command() {
    let valid = payload(1, "CHAT_RESULT", "ALLOWED", None);
    for length in 0..valid.len() {
        assert!(Session::default().receive(&valid[..length], 1).is_err());
    }
    for data in [
        payload(2, "CHAT_RESULT", "ALLOWED", None),
        payload(1, "UNKNOWN", "ALLOWED", None),
        payload(1, "CHAT_RESULT", "UNKNOWN", None),
        payload(1, "CHAT_RESULT", "MODIFY", Some("/op player")),
        payload(1, "CHAT_RESULT", "MODIFY", Some("line\nfeed")),
        payload(1, "CHAT_RESULT", "MODIFY", Some(&"x".repeat(257))),
        [valid.clone(), vec![0]].concat(),
    ] {
        assert!(Session::default().receive(&data, 1).is_err());
    }
    for bytes in [
        &b"\x00\x01\x00"[..],
        &b"\x00\x02\xc0\xaf"[..],
        &b"\x00\x03\xe0\x80\x80"[..],
        &b"\x00\x02\xc3w"[..],
        &b"\x00\x03\xed\xa0\xbd"[..],
        &b"\x00\x04\xf0\x9f\x90\x88"[..],
    ] {
        assert!(read_utf(&mut &bytes[..]).is_err());
    }
}

#[test]
fn missing_decisions_never_allow_input_and_both_queues_are_bounded() {
    for source in ["CHAT_RESULT", "COMMAND_RESULT"] {
        let input = if source == "CHAT_RESULT" { "hello" } else { "/help" };
        let mut waiting = Session::default();
        let mut ready = Session::default();
        let data = payload(1, source, "ALLOWED", None);
        for _ in 0..QUEUE_LIMIT {
            assert_eq!(waiting.submit(input.into()).unwrap(), None);
            assert_eq!(ready.receive(&data, 1).unwrap(), None);
        }
        assert!(waiting.submit(input.into()).is_err());
        assert!(ready.receive(&data, 1).is_err());
        for _ in 0..QUEUE_LIMIT {
            assert_eq!(waiting.receive(&data, 1).unwrap().as_deref(), Some(input));
            assert_eq!(ready.submit(input.into()).unwrap().as_deref(), Some(input));
        }
    }
}
