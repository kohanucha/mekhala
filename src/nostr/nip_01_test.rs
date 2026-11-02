use super::*;

#[test]
fn test_client_message_parsing() {
    let event_json = r#"["EVENT",{"id":"id","pubkey":"pk","created_at":1000,"kind":1,"tags":[],"content":"hi","sig":"sig"}]"#;
    let msg = ClientMessage::from_json(event_json).unwrap();
    match msg {
        ClientMessage::Event(e) => assert_eq!(e.content, "hi"),
        _ => panic!("Expected Event"),
    }

    let req_json = r#"["REQ","sub1",{"authors":["pk1"]}]"#;
    let msg = ClientMessage::from_json(req_json).unwrap();
    match msg {
        ClientMessage::Req(id, filters) => {
            assert_eq!(id, "sub1");
            assert_eq!(filters.len(), 1);
        }
        _ => panic!("Expected Req"),
    }

    let close_json = r#"["CLOSE","sub1"]"#;
    let msg = ClientMessage::from_json(close_json).unwrap();
    match msg {
        ClientMessage::Close(id) => assert_eq!(id, "sub1"),
        _ => panic!("Expected Close"),
    }
}

#[test]
fn test_relay_message_serialization() {
    assert_eq!(
        RelayMessage::Notice("hi".into()).to_json(),
        r#"["NOTICE","hi"]"#
    );
    assert_eq!(
        RelayMessage::Eose("sub1".into()).to_json(),
        r#"["EOSE","sub1"]"#
    );
    assert_eq!(
        RelayMessage::Ok("id1".into(), true, String::new()).to_json(),
        r#"["OK","id1",true,""]"#
    );
}

#[test]
fn test_relay_message_event_serialization() {
    let event = crate::nostr::Event {
        id: "test_id".into(),
        pubkey: "test_pubkey".into(),
        created_at: 1234567890,
        kind: 1,
        tags: vec![],
        content: "test content".into(),
        sig: "test_sig".into(),
    };
    let msg = RelayMessage::Event("sub1".into(), event);
    let json = msg.to_json();
    assert!(json.starts_with(r#"["EVENT","sub1","#));
}

#[test]
fn test_relay_message_closed_serialization() {
    assert_eq!(
        RelayMessage::Closed("sub1".into(), "reason".into()).to_json(),
        r#"["CLOSED","sub1","reason"]"#
    );
}

#[test]
fn test_nip_11_info_structure() {
    let info = serde_json::json!({"supported_nips": [1, 11, 47]});
    assert!(info.is_object());
    let nips = info
        .get("supported_nips")
        .and_then(|v| v.as_array())
        .unwrap();
    assert!(nips.contains(&serde_json::json!(1)));
    assert!(nips.contains(&serde_json::json!(11)));
    assert!(nips.contains(&serde_json::json!(47)));
}

#[test]
fn test_partial_client_message_event() {
    let json = r#"["EVENT",{"id":"abc123"}]"#;
    let msg = PartialClientMessage::from_json(json);
    assert!(matches!(msg, Some(PartialClientMessage::Event(id)) if id == "abc123"));
}

#[test]
fn test_partial_client_message_non_event() {
    let json = r#"["REQ","sub1",{}]"#;
    let msg = PartialClientMessage::from_json(json);
    assert!(msg.is_none());
}

#[test]
fn test_partial_client_message_malformed() {
    assert!(PartialClientMessage::from_json("not json").is_none());
    assert!(PartialClientMessage::from_json(r#"["ONLY_ELEMENT"]"#).is_none());
}

#[test]
fn test_client_message_empty() {
    let result = ClientMessage::from_json("[]");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("empty"));
}

#[test]
fn test_client_message_unknown_type() {
    let result = ClientMessage::from_json(r#"["UNKNOWN","data"]"#);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown message type"));
}

#[test]
fn test_client_message_malformed_json() {
    let result = ClientMessage::from_json("{{{");
    assert!(result.is_err());
}

#[test]
fn test_relay_message_unknown_type() {
    let result = RelayMessage::from_json(r#"["UNKNOWN"]"#);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown message type"));
}

#[test]
fn test_relay_message_empty() {
    let result = RelayMessage::from_json("[]");
    assert!(result.is_err());
}

#[test]
fn test_relay_message_ok_parsing() {
    let msg = RelayMessage::from_json(r#"["OK","id1",true,""]"#).unwrap();
    assert_eq!(msg, RelayMessage::Ok("id1".into(), true, String::new()));
}

#[test]
fn test_relay_message_closed_parsing() {
    let msg = RelayMessage::from_json(r#"["CLOSED","sub1","reason"]"#).unwrap();
    assert_eq!(msg, RelayMessage::Closed("sub1".into(), "reason".into()));
}

#[test]
fn test_relay_message_eose_parsing() {
    let msg = RelayMessage::from_json(r#"["EOSE","sub1"]"#).unwrap();
    assert_eq!(msg, RelayMessage::Eose("sub1".into()));
}

#[test]
fn test_relay_message_notice_parsing() {
    let msg = RelayMessage::from_json(r#"["NOTICE","hello"]"#).unwrap();
    assert_eq!(msg, RelayMessage::Notice("hello".into()));
}

#[test]
fn test_relay_message_event_parsing() {
    let json = r#"["EVENT","sub1",{"id":"test_id","pubkey":"pk","created_at":1000,"kind":1,"tags":[],"content":"hi","sig":"sig"}]"#;
    let msg = RelayMessage::from_json(json).unwrap();
    match msg {
        RelayMessage::Event(sub_id, event) => {
            assert_eq!(sub_id, "sub1");
            assert_eq!(event.id, "test_id");
        }
        _ => panic!("Expected EVENT message"),
    }
}

// Every malformed payload below is reachable from a WebSocket frame, so each
// one must be rejected with an error rather than panicking.

#[test]
fn test_partial_client_message_non_string_type() {
    assert!(PartialClientMessage::from_json(r#"[42,{"id":"abc"}]"#).is_none());
}

#[test]
fn test_partial_client_message_event_without_string_id() {
    assert!(PartialClientMessage::from_json(r#"["EVENT",{"id":42}]"#).is_none());
}

#[test]
fn test_client_message_non_string_type() {
    assert!(ClientMessage::from_json(r#"[42]"#).is_err());
}

#[test]
fn test_client_message_event_payload_invalid() {
    assert!(ClientMessage::from_json(r#"["EVENT",{"kind":1}]"#).is_err());
}

#[test]
fn test_client_message_req_non_string_sub_id() {
    assert!(ClientMessage::from_json(r#"["REQ",42,{}]"#).is_err());
}

#[test]
fn test_client_message_req_invalid_filter() {
    assert!(ClientMessage::from_json(r#"["REQ","sub1",[]]"#).is_err());
}

#[test]
fn test_client_message_close_non_string_sub_id() {
    assert!(ClientMessage::from_json(r#"["CLOSE",42]"#).is_err());
}

#[test]
fn test_client_message_below_variant_arity_is_unknown() {
    for json in [r#"["EVENT"]"#, r#"["REQ","sub1"]"#, r#"["CLOSE"]"#] {
        let err = ClientMessage::from_json(json).expect_err("short message must be rejected");
        assert!(err.contains("unknown message type"), "{json}: {err}");
    }
}

#[test]
fn test_relay_message_non_array_payload() {
    assert!(RelayMessage::from_json("{}").is_err());
    assert!(RelayMessage::from_json("not json").is_err());
}

#[test]
fn test_relay_message_non_string_type() {
    assert!(RelayMessage::from_json(r#"[42]"#).is_err());
}

#[test]
fn test_relay_message_ok_field_types() {
    assert!(RelayMessage::from_json(r#"["OK",1,true,""]"#).is_err());
    assert!(RelayMessage::from_json(r#"["OK","id1","yes",""]"#).is_err());
    assert!(RelayMessage::from_json(r#"["OK","id1",true,42]"#).is_err());
}

#[test]
fn test_relay_message_event_field_types() {
    assert!(RelayMessage::from_json(r#"["EVENT",42,{}]"#).is_err());
    assert!(RelayMessage::from_json(r#"["EVENT","sub1",{"kind":1}]"#).is_err());
}

#[test]
fn test_relay_message_eose_and_notice_field_types() {
    assert!(RelayMessage::from_json(r#"["EOSE",42]"#).is_err());
    assert!(RelayMessage::from_json(r#"["NOTICE",42]"#).is_err());
}

#[test]
fn test_relay_message_closed_field_types() {
    assert!(RelayMessage::from_json(r#"["CLOSED",42,"reason"]"#).is_err());
    assert!(RelayMessage::from_json(r#"["CLOSED","sub1",42]"#).is_err());
}

#[test]
fn test_relay_message_below_variant_arity_is_unknown() {
    for json in [
        r#"["OK","id1",true]"#,
        r#"["EVENT","sub1"]"#,
        r#"["EOSE"]"#,
        r#"["NOTICE"]"#,
        r#"["CLOSED","sub1"]"#,
    ] {
        let err = RelayMessage::from_json(json).expect_err("short message must be rejected");
        assert!(err.contains("unknown message type"), "{json}: {err}");
    }
}

#[test]
fn test_partial_client_message_single_element_array() {
    // One element cannot be a valid EVENT frame; it must be rejected instead
    // of indexing past the end of the array.
    assert!(PartialClientMessage::from_json(r#"["EVENT"]"#).is_none());
}

#[test]
fn test_partial_client_message_two_element_event() {
    match PartialClientMessage::from_json(r#"["EVENT",{"id":"abc"}]"#) {
        Some(PartialClientMessage::Event(id)) => assert_eq!(id, "abc"),
        None => panic!("a two-element EVENT frame must parse"),
    }
}
