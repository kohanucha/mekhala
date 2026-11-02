use super::*;

#[test]
fn test_tag_p_roundtrip() {
    let tag = Tag::p("abc123");
    let json = serde_json::to_value(&tag).unwrap();
    assert_eq!(json, serde_json::json!(["p", "abc123"]));

    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, Tag::P("abc123".into(), vec![]));
}

#[test]
fn test_tag_p_with_extras_roundtrip() {
    let tag = Tag::P(
        "abc123".into(),
        vec![
            Value::String("wss://relay.example.com".into()),
            Value::String("petname".into()),
        ],
    );
    let json = serde_json::to_value(&tag).unwrap();
    assert_eq!(
        json,
        serde_json::json!(["p", "abc123", "wss://relay.example.com", "petname"])
    );

    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(
        deserialized,
        Tag::P(
            "abc123".into(),
            vec![
                Value::String("wss://relay.example.com".into()),
                Value::String("petname".into())
            ]
        )
    );
}

#[test]
fn test_tag_e_roundtrip() {
    let tag = Tag::E("event_id_1".into(), vec![]);
    let json = serde_json::to_value(&tag).unwrap();
    assert_eq!(json, serde_json::json!(["e", "event_id_1"]));

    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, Tag::E("event_id_1".into(), vec![]));
}

#[test]
fn test_tag_encryption_roundtrip() {
    let tag = Tag::encryption("nip44_v2 nip04");
    let json = serde_json::to_value(&tag).unwrap();
    assert_eq!(json, serde_json::json!(["encryption", "nip44_v2 nip04"]));

    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, Tag::Encryption("nip44_v2 nip04".into()));
}

#[test]
fn test_tag_expiration_roundtrip() {
    let tag = Tag::expiration(1234567890u64);
    let json = serde_json::to_value(&tag).unwrap();
    assert_eq!(json, serde_json::json!(["expiration", "1234567890"]));

    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, Tag::Expiration("1234567890".into()));
}

#[test]
fn test_tag_expiration_numeric_json() {
    let json = serde_json::json!(["expiration", 1234567890u64]);
    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized, Tag::Expiration("1234567890".into()));
    let serialized = serde_json::to_value(&deserialized).unwrap();
    assert_eq!(serialized, serde_json::json!(["expiration", "1234567890"]));
}

#[test]
fn test_tag_other_roundtrip() {
    let tag = Tag::Other(
        "custom".into(),
        vec![
            Value::String("value1".into()),
            Value::String("value2".into()),
        ],
    );
    let json = serde_json::to_value(&tag).unwrap();
    assert_eq!(json, serde_json::json!(["custom", "value1", "value2"]));

    let deserialized: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(
        deserialized,
        Tag::Other(
            "custom".into(),
            vec![
                Value::String("value1".into()),
                Value::String("value2".into())
            ]
        )
    );
}

#[test]
fn test_tag_accessors() {
    let p = Tag::p("pk1");
    assert!(p.is_p());
    assert!(!p.is_e());
    assert_eq!(p.pubkey(), Some("pk1"));
    assert_eq!(p.event_id(), None);
    assert_eq!(p.encryption_scheme(), None);

    let e = Tag::E("eid1".into(), vec![]);
    assert!(!e.is_p());
    assert!(e.is_e());
    assert_eq!(e.event_id(), Some("eid1"));
    assert_eq!(e.pubkey(), None);

    let enc = Tag::encryption("nip04");
    assert_eq!(enc.encryption_scheme(), Some("nip04"));
    assert_eq!(enc.pubkey(), None);

    let exp = Tag::expiration(999u64);
    assert_eq!(exp.pubkey(), None);
    assert_eq!(exp.event_id(), None);
}

#[test]
fn test_tags_in_event_json() {
    let event_json = r#"{
            "id": "abc",
            "pubkey": "pk1",
            "created_at": 1000,
            "kind": 23194,
            "tags": [["p", "wallet_pk"], ["e", "event1"], ["encryption", "nip44_v2 nip04"]],
            "content": "hello",
            "sig": "sig1"
        }"#;

    let event: crate::nostr::Event = serde_json::from_str(event_json).unwrap();
    assert_eq!(event.tags.len(), 3);
    assert_eq!(event.tags[0], Tag::P("wallet_pk".into(), vec![]));
    assert_eq!(event.tags[1], Tag::E("event1".into(), vec![]));
    assert_eq!(event.tags[2], Tag::Encryption("nip44_v2 nip04".into()));
}

#[test]
fn test_tag_preserves_non_string_values() {
    let json = serde_json::json!(["p", "abc123", "wss://relay.example.com", "petname"]);
    let tag: Tag = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(
        tag,
        Tag::P(
            "abc123".into(),
            vec![
                Value::String("wss://relay.example.com".into()),
                Value::String("petname".into())
            ]
        )
    );
    let serialized = serde_json::to_value(&tag).unwrap();
    assert_eq!(serialized, json);
}

#[test]
fn test_expiration_tag_preserves_string_form() {
    let json_in = serde_json::json!(["expiration", "1700000000"]);
    let tag: Tag = serde_json::from_value(json_in.clone()).unwrap();
    assert_eq!(tag, Tag::Expiration("1700000000".into()));
    let json_out = serde_json::to_value(&tag).unwrap();
    assert_eq!(json_out, json_in);
}

#[test]
fn test_tag_kind_value_numeric() {
    let json = serde_json::json!(["k", 13194]);
    let tag: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(tag.kind_value(), Some(13194));
}

#[test]
fn test_tag_kind_value_string() {
    let json = serde_json::json!(["k", "13194"]);
    let tag: Tag = serde_json::from_value(json).unwrap();
    assert_eq!(tag.kind_value(), Some(13194));
}

#[test]
fn test_tag_kind_value_non_k_tag() {
    let tag = Tag::p("pk1");
    assert_eq!(tag.kind_value(), None);

    let tag = Tag::E("event1".into(), vec![]);
    assert_eq!(tag.kind_value(), None);
}

#[test]
fn test_tag_deserialize_empty_array() {
    let result: Result<Tag, _> = serde_json::from_value(serde_json::json!([]));
    assert!(result.is_err());
}

#[test]
fn test_tag_deserialize_non_string_name() {
    let result: Result<Tag, _> = serde_json::from_value(serde_json::json!([42]));
    assert!(result.is_err());
}

#[test]
fn test_tag_deserialize_non_array() {
    let result: Result<Tag, _> = serde_json::from_value(serde_json::json!({"k": "v"}));
    let err = result.expect_err("object is not a tag");
    assert!(err.to_string().contains("sequence"), "{err}");
}

#[test]
fn test_tag_deserialize_p_without_value() {
    for json in [serde_json::json!(["p"]), serde_json::json!(["p", 42])] {
        let result: Result<Tag, _> = serde_json::from_value(json);
        let err = result.expect_err("p tag needs a string pubkey");
        assert!(err.to_string().contains("p-tag requires a pubkey value"), "{err}");
    }
}

#[test]
fn test_tag_deserialize_e_without_value() {
    for json in [serde_json::json!(["e"]), serde_json::json!(["e", true])] {
        let result: Result<Tag, _> = serde_json::from_value(json);
        let err = result.expect_err("e tag needs a string event id");
        assert!(
            err.to_string().contains("e-tag requires an event id value"),
            "{err}"
        );
    }
}

#[test]
fn test_tag_deserialize_encryption_without_scheme() {
    for json in [
        serde_json::json!(["encryption"]),
        serde_json::json!(["encryption", 44]),
    ] {
        let result: Result<Tag, _> = serde_json::from_value(json);
        let err = result.expect_err("encryption tag needs a scheme");
        assert!(
            err.to_string()
                .contains("encryption-tag requires a scheme value"),
            "{err}"
        );
    }
}

#[test]
fn test_tag_deserialize_expiration_without_value() {
    let result: Result<Tag, _> = serde_json::from_value(serde_json::json!(["expiration"]));
    let err = result.expect_err("expiration tag needs a timestamp");
    assert!(
        err.to_string()
            .contains("expiration-tag requires a timestamp value"),
        "{err}"
    );
}

#[test]
fn test_tag_deserialize_expiration_non_numeric_value() {
    for json in [
        serde_json::json!(["expiration", true]),
        serde_json::json!(["expiration", 1.5]),
        serde_json::json!(["expiration", null]),
    ] {
        let result: Result<Tag, _> = serde_json::from_value(json);
        let err = result.expect_err("expiration must be a number or numeric string");
        assert!(
            err.to_string()
                .contains("must be a number or numeric string"),
            "{err}"
        );
    }
}

#[test]
fn test_tag_deserialize_expiration_non_numeric_string() {
    let result: Result<Tag, _> = serde_json::from_value(serde_json::json!(["expiration", "abc"]));
    let err = result.expect_err("expiration string must parse as u64");
    assert!(err.to_string().contains("must be a number"), "{err}");
}
