use super::*;
use crate::common::test_helpers::{MockStorage, new_test_engine, test_now, set_test_time};
use super::super::Tag;

#[test]
fn test_engine_req_storage() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let req = r#"["REQ", "sub1", {"kinds": [23194], "authors": ["pk1"]}]"#;
        let msg = ClientMessage::from_json(req).unwrap();
        let responses = engine.handle_typed(1, msg).await;

        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { message, .. } = r {
                matches!(message, RelayMessage::Eose(_))
            } else {
                false
            }
        }));
    });
}

#[test]
fn test_engine_info_event_routing() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };

        engine.process_info_event(event).await;

        assert!(engine.get_wallet_info("pk1").await.is_some());
    });
}

#[test]
fn test_get_wallet_info_none() {
    let mut engine = new_test_engine();
    futures::executor::block_on(async {
        assert!(engine.get_wallet_info("pk1").await.is_none());
    });
}

#[test]
fn test_engine_get_wallet_info_with_encryption_tag() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![super::super::Tag::encryption("nip44_v2 nip04")],
            content: "".into(),
            sig: "sig1".into(),
        };
        engine.process_info_event(event).await;

        let info = engine.get_wallet_info("pk1").await.unwrap();
        assert!(info.encryption_algorithms.contains(&super::super::nip_47::EncryptionMethod::Nip44));
        assert!(info.encryption_algorithms.contains(&super::super::nip_47::EncryptionMethod::Nip04));
    });
}

#[test]
fn test_engine_get_wallet_info_default_nip04() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };
        engine.process_info_event(event).await;

        let info = engine.get_wallet_info("pk1").await.unwrap();
        assert_eq!(info.encryption_algorithms, vec![super::super::nip_47::EncryptionMethod::Nip04]);
    });
}

#[test]
fn test_bridge_signaling() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        let _ = engine.process_req(1, "sub1".into(), vec![Filter {
            kinds: Some(vec![23194]),
            p_tags: Some(vec![wallet_pk.into()]),
            ..Default::default()
        }]).await;

        let bridge_id = 100;
        let bridge_sk = "0202020202020202020202020202020202020202020202020202020202020202";

        // Create a valid signed event for the bridge
        let uri = crate::nostr::nip_47::NwcUri {
            wallet_pubkey: wallet_pk.to_string(),
            secret: bridge_sk.to_string(),
        };
        let client = crate::nostr::nip_47::NwcClient::new(uri).unwrap();

        let bridge_req = serde_json::json!(["REQ", "sub_bridge", {"kinds": [23194], "#p": [client.my_pubkey]}]).to_string();
        let msg = ClientMessage::from_json(&bridge_req).unwrap();
        let responses = engine.handle_typed(bridge_id, msg).await;

        // REQ should return EOSE Send
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 100, message: RelayMessage::Eose(_) })));

        // Create a valid signed event for the bridge
        let uri = crate::nostr::nip_47::NwcUri {
            wallet_pubkey: wallet_pk.to_string(),
            secret: bridge_sk.to_string(),
        };
        let client = crate::nostr::nip_47::NwcClient::new(uri).unwrap();
        let (bridge_event, _) = client.create_request_event(crate::nostr::nip_47::NwcMethod::MakeInvoice, serde_json::json!({}), vec![]).unwrap();

        let bridge_event_json = serde_json::json!([
            "EVENT",
            bridge_event
        ]).to_string();

        let msg = ClientMessage::from_json(&bridge_event_json).unwrap();
        let responses = engine.handle_typed(bridge_id, msg).await;

        // Event should be routed to connection 1 as Send
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 1, .. })));
        // EVENT should return OK Send
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 100, message: RelayMessage::Ok(_, true, _) })));

        let wallet_response_event: Event = serde_json::from_value(serde_json::json!({
            "id": "resp1",
            "pubkey": "wallet_pk",
            "created_at": test_now(),
            "kind": 23194,
            "tags": [["p", "bridge"], ["e", "event1"]],
            "content": "",
            "sig": "dummy_sig"
        })).unwrap();
        
        let mut wallet_response_event = wallet_response_event;
        wallet_response_event.tags = vec![super::super::Tag::p(&client.my_pubkey), super::super::Tag::E(bridge_event.id.clone(), vec![])];

        // Wallet response comes from connection 1
        let responses = engine.process_event(1, wallet_response_event).await;

        // Routed EVENT SHOULD go to connection 100 as Send
        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { recipient_id, message: RelayMessage::Event(_, event) } = r {
                *recipient_id == 100 && event.id == "resp1"
            } else {
                false
            }
        }));
    });
}

#[test]
fn test_virtual_connection_lifecycle() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let id = 100;
        let _ = engine.process_req(id, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            kinds: Some(vec![23194]),
            ..Default::default()
        }]).await;

        let event = Event {
            id: "event1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 23194,
            tags: vec![],
            content: "test".into(),
            sig: "sig".into(),
        };

        let responses = engine.process_event(2, event).await;

        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { recipient_id, message: RelayMessage::Event(_, event) } = r {
                *recipient_id == 100 && event.id == "event1"
            } else {
                false
            }
        }));

        engine.process_close(id, "sub1".into()).await;
        let responses_after = engine.process_event(2, Event {
            id: "event2".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 23194,
            tags: vec![],
            content: "test".into(),
            sig: "sig".into(),
        }).await;
        assert!(!responses_after.iter().any(|r| {
            matches!(r, EngineResponse::Send { recipient_id, .. } if *recipient_id == id)
        }), "closed subscription should not receive events");
    });
}

#[test]
fn test_engine_wakeup_logic() {
    futures::executor::block_on(async {
        // 1. Seed the storage with a "hibernated" connection
        let id = 42;
        let pk = "hibernated_pk";
        let storage = MockStorage::new();
        crate::common::test_helpers::seed_subscription(&storage, id, "sub1", pk, vec![
            Filter {
                kinds: Some(vec![23194]),
                authors: Some(vec![pk.into()]),
                ..Default::default()
            }
        ]).await;
        
        let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);

        // 2. Handle an event that targets the hibernated pubkey
        let event = Event {
            id: "event1".into(),
            pubkey: pk.into(),
            created_at: test_now(),
            kind: 23194,
            tags: vec![super::super::Tag::p(pk)],
            content: "wake up!".into(),
            sig: "sig".into(),
        };

        let responses = engine.process_event(99, event).await;

        // 3. Verify that a WakeUp response was returned
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::WakeUp { connection_id: 42 })));
        
        // 4. Verify that a Data response was ALSO returned (since matching happens after loading)
        assert!(responses.iter().any(|r| matches!(r, EngineResponse::Send { recipient_id: 42, .. })));
    });
}

#[test]
fn test_engine_uses_clock_for_event_verification() {
    futures::executor::block_on(async {
        let now = 1700000000u64;
        set_test_time(now);
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let event_json = serde_json::json!(["EVENT", {
            "id": "fake_id",
            "pubkey": "pk1",
            "created_at": now + 901,
            "kind": 23194,
            "tags": [],
            "content": "test",
            "sig": "badsig"
        }]).to_string();

        let msg = ClientMessage::from_json(&event_json).unwrap();
        let responses = engine.handle_typed(1, msg).await;

        let ok_response = responses.iter().find_map(|r| {
            if let EngineResponse::Send { message: RelayMessage::Ok(_, success, msg), .. } = r {
                Some((success, msg.clone()))
            } else { None }
        });

        assert!(ok_response.is_some(), "should get an OK response");
        let (success, msg) = ok_response.unwrap();
        assert!(!success, "event too far in future should be rejected");
        assert!(msg.contains("too far"), "expected timestamp rejection, got: {}", msg);

        set_test_time(now + 901);
        let event_json_recent = serde_json::json!(["EVENT", {
            "id": "fake_id2",
            "pubkey": "pk1",
            "created_at": now + 901,
            "kind": 23194,
            "tags": [],
            "content": "test",
            "sig": "badsig"
        }]).to_string();

        let msg = ClientMessage::from_json(&event_json_recent).unwrap();
        let responses = engine.handle_typed(1, msg).await;

        let ok_response = responses.iter().find_map(|r| {
            if let EngineResponse::Send { message: RelayMessage::Ok(_, success, msg), .. } = r {
                Some((success, msg.clone()))
            } else { None }
        });

        assert!(ok_response.is_some(), "should get an OK response");
        let (success, msg) = ok_response.unwrap();
        assert!(msg.contains("invalid"), "event within tolerance should fail for id/sig, not timestamp, got: {}", msg);
        assert!(!success);
    });
}

#[test]
fn test_kind_5_deletion_with_e_tag() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        // Publish info event for alice
        let info_event = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };
        engine.process_info_event(info_event.clone()).await;
        assert!(engine.get_wallet_info("alice").await.is_some());

        // Delete it via kind 5 with e-tag referencing the info event
        let deletion_event = Event {
            id: "del1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![Tag::E("info1".into(), vec![])],
            content: "deleted".into(),
            sig: "sig2".into(),
        };
        engine.process_deletion_event(&deletion_event).await;

        // Info should be gone
        assert!(engine.get_wallet_info("alice").await.is_none());
    });
}

#[test]
fn test_kind_5_deletion_unauthorized() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        // Publish info event for alice
        let info_event = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };
        engine.process_info_event(info_event.clone()).await;
        assert!(engine.get_wallet_info("alice").await.is_some());

        // Try to delete with wrong pubkey (bob)
        let deletion_event = Event {
            id: "del1".into(),
            pubkey: "bob".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![Tag::E("info1".into(), vec![])],
            content: "deleted".into(),
            sig: "sig2".into(),
        };
        engine.process_deletion_event(&deletion_event).await;

        // Info should still be there (bob can't delete alice's info)
        assert!(engine.get_wallet_info("alice").await.is_some());
    });
}

#[test]
fn test_kind_5_deletion_with_k_tag() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let info_event = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };
        engine.process_info_event(info_event.clone()).await;
        assert!(engine.get_wallet_info("alice").await.is_some());

        // Delete via k=13194 tag without e-tag
        let deletion_event = Event {
            id: "del1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![Tag::Other("k".into(), vec![serde_json::json!("13194")])],
            content: "deleted".into(),
            sig: "sig2".into(),
        };
        engine.process_deletion_event(&deletion_event).await;

        assert!(engine.get_wallet_info("alice").await.is_none());
    });
}

#[test]
fn test_validate_event_rejects_invalid_kind() {
    let engine = new_test_engine();
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: test_now(),
        kind: 1,
        tags: vec![],
        content: "".into(),
        sig: "sig".into(),
    };
    let result = engine.validate_event(&event);
    assert!(result.is_err());
    assert!(result.unwrap_err().1.contains("kind not allowed"));
}

#[test]
fn test_validate_event_accepts_valid_kind() {
    let engine = new_test_engine();
    let event = Event {
        id: "id1".into(),
        pubkey: "pk1".into(),
        created_at: test_now(),
        kind: 13194,
        tags: vec![],
        content: "".into(),
        sig: "badsig".into(),
    };
    let result = engine.validate_event(&event);
    assert!(result.is_err()); // fails at signature, not kind
    assert!(!result.unwrap_err().1.contains("kind not allowed"));
}

#[test]
fn test_route_verified_event_with_info_kind() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        // Subscribe to track info events
        let info_event = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: "wallet info".into(),
            sig: "sig1".into(),
        };

        let _ = engine.route_verified_event(1, info_event.clone()).await;
        // route_verified_event does NOT send OK (caller's job) — just caches + routes
        // Verify info was cached
        assert!(engine.get_wallet_info("alice").await.is_some());
    });
}

#[test]
fn test_handle_close_removes_subscription() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let msg = ClientMessage::Close("sub1".into());
        let responses = engine.handle_typed(1, msg).await;
        // Close should not produce any responses
        assert!(responses.is_empty());
    });
}

#[test]
fn test_on_terminate_removes_state() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let _ = engine.process_req(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await;

        engine.on_terminate(1).await;
        let responses_after = engine.process_event(2, Event {
            id: "event2".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 23194,
            tags: vec![],
            content: "test".into(),
            sig: "sig".into(),
        }).await;
        assert!(!responses_after.iter().any(|r| {
            matches!(r, EngineResponse::Send { recipient_id, .. } if *recipient_id == 1)
        }), "terminated connection should not receive events");
    });
}

#[test]
fn test_req_filter_too_broad() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let msg = ClientMessage::Req("sub1".into(), vec![Filter::default()]);
        let responses = engine.handle_typed(1, msg).await;
        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send { message, .. } = r {
                matches!(message, RelayMessage::Closed(_, _))
            } else {
                false
            }
        }));
    });
}

#[test]
fn test_handle_req_returns_closed_on_storage_failure() {
    futures::executor::block_on(async {
        use std::sync::{Arc, Mutex};
        use std::collections::HashMap;
        let storage = crate::common::test_helpers::MockStorage {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: true,
        };
        let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);
        engine.on_connect(1).await;

        let responses = engine.handle_req(1, "sub1".into(), vec![Filter {
            kinds: Some(vec![23194]),
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await;

        assert_eq!(responses.len(), 1, "should only have CLOSED response");
        match &responses[0] {
            EngineResponse::Send { recipient_id, message } => {
                assert_eq!(*recipient_id, 1);
                match message {
                    RelayMessage::Closed(sub_id, reason) => {
                        assert_eq!(sub_id, "sub1");
                        assert!(reason.contains("persist failed"), "reason should mention persist failure, got: {}", reason);
                    }
                    other => panic!("expected CLOSED, got: {:?}", other),
                }
            }
            other => panic!("expected EngineResponse::Send, got: {:?}", other),
        }
    });
}

#[test]
fn test_handle_req_internal_returns_closed_on_storage_failure() {
    futures::executor::block_on(async {
        use std::sync::{Arc, Mutex};
        use std::collections::HashMap;
        let storage = crate::common::test_helpers::MockStorage {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: true,
        };
        let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);
        engine.on_connect(1).await;

        let responses = engine.handle_req_internal(1, "sub1".into(), vec![Filter {
            kinds: Some(vec![23194]),
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await;

        assert_eq!(responses.len(), 1, "should only have CLOSED response");
        match &responses[0] {
            EngineResponse::Send { recipient_id, message } => {
                assert_eq!(*recipient_id, 1);
                match message {
                    RelayMessage::Closed(sub_id, reason) => {
                        assert_eq!(sub_id, "sub1");
                        assert!(reason.contains("persist failed"), "reason should mention persist failure, got: {}", reason);
                    }
                    other => panic!("expected CLOSED, got: {:?}", other),
                }
            }
            other => panic!("expected EngineResponse::Send, got: {:?}", other),
        }
    });
}

#[test]
fn test_kind_5_deletion_no_tags() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let info_event = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
        };
        engine.process_info_event(info_event.clone()).await;
        assert!(engine.get_wallet_info("alice").await.is_some());

        // Delete with no e/k tags (delete all events by this author)
        let deletion_event = Event {
            id: "del1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![],
            content: "deleted".into(),
            sig: "sig2".into(),
        };
        engine.process_deletion_event(&deletion_event).await;

        assert!(engine.get_wallet_info("alice").await.is_none());
    });
}
