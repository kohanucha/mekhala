use super::*;

    #[test]
    fn test_target_pubkeys_author_only() {
        let event = Event {
            id: "id".into(),
            pubkey: "pk1".into(),
            created_at: 0,
            kind: 1,
            tags: vec![],
            content: "".into(),
            sig: "".into(),
        };
        let keys = event.target_pubkeys();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("pk1"));
    }

    #[test]
    fn test_target_pubkeys_with_p_tags() {
        let event = Event {
            id: "id".into(),
            pubkey: "author".into(),
            created_at: 0,
            kind: 1,
            tags: vec![
                Tag::p("recipient1"),
                Tag::p("recipient2"),
                Tag::E("event_id".into(), vec![]),
            ],
            content: "".into(),
            sig: "".into(),
        };
        let keys = event.target_pubkeys();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains("author"));
        assert!(keys.contains("recipient1"));
        assert!(keys.contains("recipient2"));
    }

    #[test]
    fn test_target_pubkeys_deduplication() {
        let event = Event {
            id: "id".into(),
            pubkey: "pk1".into(),
            created_at: 0,
            kind: 1,
            tags: vec![
                Tag::P("pk1".into(), vec![]),
                Tag::p("pk2"),
                Tag::P("pk2".into(), vec![]),
            ],
            content: "".into(),
            sig: "".into(),
        };
        let keys = event.target_pubkeys();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains("pk1"));
        assert!(keys.contains("pk2"));
    }

    #[test]
    fn test_kind_5_passes_kind_check() {
        let event = Event {
            id: "id5".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 5,
            tags: vec![Tag::E("event_to_delete".into(), vec![])],
            content: "deleting".into(),
            sig: "badsig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        // Kind 5 should pass the kind check, then fail on id/signature
        match result {
            Err(RelayError::InvalidId) | Err(RelayError::InvalidSignature) => {}
            Err(RelayError::InvalidKind) => panic!("kind 5 should not be rejected as InvalidKind"),
            other => panic!("expected id/sig error, got {:?}", other),
        }
    }

    #[test]
    fn test_verify_invalid_kind() {
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 1,
            tags: vec![],
            content: "".into(),
            sig: "sig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        assert_eq!(result, Err(RelayError::InvalidKind));
    }

    #[test]
    fn test_verify_content_too_large() {
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 13194,
            tags: vec![],
            content: "a".repeat(65537),
            sig: "sig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        match result {
            Err(RelayError::LimitExceeded(msg)) => assert!(msg.contains("content too large")),
            _ => panic!("expected LimitExceeded, got {:?}", result),
        }
    }

    #[test]
    fn test_verify_kind_23196_missing_p_tag() {
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 23196,
            tags: vec![],
            content: "".into(),
            sig: "sig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        assert_eq!(result, Err(RelayError::MissingTag("p".into())));
    }

    #[test]
    fn test_verify_kind_23197_missing_p_tag() {
        let event = Event {
            id: "id2".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 23197,
            tags: vec![Tag::E("eid1".into(), vec![])],
            content: "".into(),
            sig: "sig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        assert_eq!(result, Err(RelayError::MissingTag("p".into())));
    }

    #[test]
    fn test_verify_kind_23195_missing_p_tag() {
        let event = Event {
            id: "id3".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 23195,
            tags: vec![Tag::E("eid1".into(), vec![])],
            content: "".into(),
            sig: "sig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        assert_eq!(result, Err(RelayError::MissingTag("p".into())));
    }

    #[test]
    fn test_verify_kind_23195_missing_e_tag() {
        let event = Event {
            id: "id4".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 23195,
            tags: vec![Tag::p("pk2")],
            content: "".into(),
            sig: "sig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        assert_eq!(result, Err(RelayError::MissingTag("e".into())));
    }

    #[test]
    fn test_verify_kind_23196_with_p_tag_passes_kind_check() {
        let event = Event {
            id: "id5".into(),
            pubkey: "pk1".into(),
            created_at: 1700000000,
            kind: 23196,
            tags: vec![Tag::p("pk2")],
            content: "".into(),
            sig: "badsig".into(),
        };
        let result = event.verify(1700000000, &Limits::default());
        match result {
            Err(RelayError::InvalidId) | Err(RelayError::InvalidSignature) => {}
            Err(e) => panic!("expected id/sig error for kind 23196 with p tag, got {:?}", e),
            Ok(_) => panic!("expected error for kind 23196 with bad sig"),
        }
    }
