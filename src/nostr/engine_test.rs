use super::super::Tag;
use super::*;
use crate::common::test_helpers::{new_test_engine, set_test_time, test_now, MockStorage};

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
            content: String::new(),
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
            content: String::new(),
            sig: "sig1".into(),
        };
        engine.process_info_event(event).await;

        let info = engine.get_wallet_info("pk1").await.unwrap();
        assert!(info
            .encryption_algorithms
            .contains(&super::super::nip_47::EncryptionMethod::Nip44));
        assert!(info
            .encryption_algorithms
            .contains(&super::super::nip_47::EncryptionMethod::Nip04));
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
            content: String::new(),
            sig: "sig1".into(),
        };
        engine.process_info_event(event).await;

        let info = engine.get_wallet_info("pk1").await.unwrap();
        assert_eq!(
            info.encryption_algorithms,
            vec![super::super::nip_47::EncryptionMethod::Nip04]
        );
    });
}

#[test]
fn test_bridge_signaling() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let wallet_pk = "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        let _ = engine
            .process_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    p_tags: Some(vec![wallet_pk.into()]),
                    ..Default::default()
                }],
            )
            .await;

        let bridge_id = 100;
        let bridge_sk = "0202020202020202020202020202020202020202020202020202020202020202";

        // Create a valid signed event for the bridge
        let uri = crate::nostr::nip_47::NwcUri {
            wallet_pubkey: wallet_pk.to_string(),
            secret: bridge_sk.to_string(),
        };
        let client = crate::nostr::nip_47::NwcClient::new(uri).unwrap();

        let bridge_req =
            serde_json::json!(["REQ", "sub_bridge", {"kinds": [23194], "#p": [client.my_pubkey]}])
                .to_string();
        let msg = ClientMessage::from_json(&bridge_req).unwrap();
        let responses = engine.handle_typed(bridge_id, msg).await;

        // REQ should return EOSE Send
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                recipient_id: 100,
                message: RelayMessage::Eose(_)
            }
        )));

        // Create a valid signed event for the bridge
        let uri = crate::nostr::nip_47::NwcUri {
            wallet_pubkey: wallet_pk.to_string(),
            secret: bridge_sk.to_string(),
        };
        let client = crate::nostr::nip_47::NwcClient::new(uri).unwrap();
        let (bridge_event, _) = client
            .create_request_event(
                crate::nostr::nip_47::NwcMethod::MakeInvoice,
                serde_json::json!({}),
                vec![],
            )
            .unwrap();

        let bridge_event_json = serde_json::json!(["EVENT", bridge_event]).to_string();

        let msg = ClientMessage::from_json(&bridge_event_json).unwrap();
        let responses = engine.handle_typed(bridge_id, msg).await;

        // Event should be routed to connection 1 as Send
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                recipient_id: 1,
                ..
            }
        )));
        // EVENT should return OK Send
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                recipient_id: 100,
                message: RelayMessage::Ok(_, true, _)
            }
        )));

        let wallet_response_event: Event = serde_json::from_value(serde_json::json!({
            "id": "resp1",
            "pubkey": "wallet_pk",
            "created_at": test_now(),
            "kind": 23194,
            "tags": [["p", "bridge"], ["e", "event1"]],
            "content": "",
            "sig": "dummy_sig"
        }))
        .unwrap();

        let mut wallet_response_event = wallet_response_event;
        wallet_response_event.tags = vec![
            super::super::Tag::p(&client.my_pubkey),
            super::super::Tag::E(bridge_event.id.clone(), vec![]),
        ];

        // Wallet response comes from connection 1
        let responses = engine.process_event(1, wallet_response_event).await;

        // Routed EVENT SHOULD go to connection 100 as Send
        assert!(responses.iter().any(|r| {
            if let EngineResponse::Send {
                recipient_id,
                message: RelayMessage::Event(_, event),
            } = r
            {
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
        let _ = engine
            .process_req(
                id,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    kinds: Some(vec![23194]),
                    ..Default::default()
                }],
            )
            .await;

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
            if let EngineResponse::Send {
                recipient_id,
                message: RelayMessage::Event(_, event),
            } = r
            {
                *recipient_id == 100 && event.id == "event1"
            } else {
                false
            }
        }));

        engine.process_close(id, "sub1".into()).await;
        let responses_after = engine
            .process_event(
                2,
                Event {
                    id: "event2".into(),
                    pubkey: "alice".into(),
                    created_at: test_now(),
                    kind: 23194,
                    tags: vec![],
                    content: "test".into(),
                    sig: "sig".into(),
                },
            )
            .await;
        assert!(
            !responses_after.iter().any(|r| {
                matches!(r, EngineResponse::Send { recipient_id, .. } if *recipient_id == id)
            }),
            "closed subscription should not receive events"
        );
    });
}

#[test]
fn test_engine_wakeup_logic() {
    futures::executor::block_on(async {
        // 1. Seed the storage with a "hibernated" connection
        let id = 42;
        let pk = "hibernated_pk";
        let storage = MockStorage::new();
        crate::common::test_helpers::seed_subscription(
            &storage,
            id,
            "sub1",
            pk,
            vec![Filter {
                kinds: Some(vec![23194]),
                authors: Some(vec![pk.into()]),
                ..Default::default()
            }],
        )
        .await;

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
        assert!(responses
            .iter()
            .any(|r| matches!(r, EngineResponse::WakeUp { connection_id: 42 })));

        // 4. Verify that a Data response was ALSO returned (since matching happens after loading)
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                recipient_id: 42,
                ..
            }
        )));
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
        }])
        .to_string();

        let msg = ClientMessage::from_json(&event_json).unwrap();
        let responses = engine.handle_typed(1, msg).await;

        let ok_response = responses.iter().find_map(|r| {
            if let EngineResponse::Send {
                message: RelayMessage::Ok(_, success, msg),
                ..
            } = r
            {
                Some((success, msg.clone()))
            } else {
                None
            }
        });

        assert!(ok_response.is_some(), "should get an OK response");
        let (success, msg) = ok_response.unwrap();
        assert!(!success, "event too far in future should be rejected");
        assert!(
            msg.contains("too far"),
            "expected timestamp rejection, got: {msg}"
        );

        set_test_time(now + 901);
        let event_json_recent = serde_json::json!(["EVENT", {
            "id": "fake_id2",
            "pubkey": "pk1",
            "created_at": now + 901,
            "kind": 23194,
            "tags": [],
            "content": "test",
            "sig": "badsig"
        }])
        .to_string();

        let msg = ClientMessage::from_json(&event_json_recent).unwrap();
        let responses = engine.handle_typed(1, msg).await;

        let ok_response = responses.iter().find_map(|r| {
            if let EngineResponse::Send {
                message: RelayMessage::Ok(_, success, msg),
                ..
            } = r
            {
                Some((success, msg.clone()))
            } else {
                None
            }
        });

        assert!(ok_response.is_some(), "should get an OK response");
        let (success, msg) = ok_response.unwrap();
        assert!(
            msg.contains("invalid"),
            "event within tolerance should fail for id/sig, not timestamp, got: {msg}"
        );
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
            content: String::new(),
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
            content: String::new(),
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
            content: String::new(),
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
        content: String::new(),
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
        content: String::new(),
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

        let _ = engine
            .process_req(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;

        engine.on_terminate(1).await;
        let responses_after = engine
            .process_event(
                2,
                Event {
                    id: "event2".into(),
                    pubkey: "alice".into(),
                    created_at: test_now(),
                    kind: 23194,
                    tags: vec![],
                    content: "test".into(),
                    sig: "sig".into(),
                },
            )
            .await;
        assert!(
            !responses_after.iter().any(|r| {
                matches!(r, EngineResponse::Send { recipient_id, .. } if *recipient_id == 1)
            }),
            "terminated connection should not receive events"
        );
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
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        let storage = crate::common::test_helpers::MockStorage {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: true,
        };
        let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);
        engine.on_connect(1).await;

        let responses = engine
            .handle_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;

        assert_eq!(responses.len(), 1, "should only have CLOSED response");
        match &responses[0] {
            EngineResponse::Send {
                recipient_id,
                message,
            } => {
                assert_eq!(*recipient_id, 1);
                match message {
                    RelayMessage::Closed(sub_id, reason) => {
                        assert_eq!(sub_id, "sub1");
                        assert!(
                            reason.contains("persist failed"),
                            "reason should mention persist failure, got: {reason}"
                        );
                    }
                    other => panic!("expected CLOSED, got: {other:?}"),
                }
            }
            other => panic!("expected EngineResponse::Send, got: {other:?}"),
        }
    });
}

#[test]
fn test_handle_req_internal_returns_closed_on_storage_failure() {
    futures::executor::block_on(async {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        let storage = crate::common::test_helpers::MockStorage {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: true,
        };
        let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);
        engine.on_connect(1).await;

        let responses = engine
            .handle_req_internal(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;

        assert_eq!(responses.len(), 1, "should only have CLOSED response");
        match &responses[0] {
            EngineResponse::Send {
                recipient_id,
                message,
            } => {
                assert_eq!(*recipient_id, 1);
                match message {
                    RelayMessage::Closed(sub_id, reason) => {
                        assert_eq!(sub_id, "sub1");
                        assert!(
                            reason.contains("persist failed"),
                            "reason should mention persist failure, got: {reason}"
                        );
                    }
                    other => panic!("expected CLOSED, got: {other:?}"),
                }
            }
            other => panic!("expected EngineResponse::Send, got: {other:?}"),
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
            content: String::new(),
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

use k256::schnorr::{signature::hazmat::PrehashSigner, SigningKey};

fn wallet_sk() -> SigningKey {
    let bytes = hex::decode(crate::common::test_helpers::TEST_WALLET_SK).unwrap();
    let arr: [u8; 32] = bytes.try_into().unwrap();
    SigningKey::from_slice(&arr).unwrap()
}

fn signed_event(sk: &SigningKey, kind: u64, tags: Vec<Tag>, content: &str) -> Event {
    let pk = crate::common::test_helpers::TEST_WALLET_PK;
    let created_at = test_now();
    let (id, id_bytes) = Event::compute_id(pk, created_at, kind, &tags, content).unwrap();
    let sig = hex::encode(sk.sign_prehash(&id_bytes).unwrap().to_bytes());
    Event {
        id,
        pubkey: pk.into(),
        created_at,
        kind,
        tags,
        content: content.into(),
        sig,
    }
}

fn ok_response(responses: &[EngineResponse]) -> (bool, String) {
    responses
        .iter()
        .find_map(|r| match r {
            EngineResponse::Send {
                message: RelayMessage::Ok(_, success, msg),
                ..
            } => Some((*success, msg.clone())),
            _ => None,
        })
        .expect("expected an OK response")
}

#[test]
fn test_handle_typed_caches_info_event() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let event = signed_event(&wallet_sk(), 13194, vec![Tag::encryption("nip44_v2")], "{}");
        let responses = engine.handle_typed(1, ClientMessage::Event(event)).await;

        assert!(ok_response(&responses).0);
        let info = engine
            .get_wallet_info(crate::common::test_helpers::TEST_WALLET_PK)
            .await
            .expect("info event must be cached");
        assert_eq!(
            info.encryption_algorithms,
            vec![super::super::nip_47::EncryptionMethod::Nip44]
        );
    });
}

#[test]
fn test_handle_typed_deletion_removes_cached_info() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let sk = wallet_sk();
        let pk = crate::common::test_helpers::TEST_WALLET_PK;

        let info = signed_event(&sk, 13194, vec![], "{}");
        engine.process_info_event(info.clone()).await;
        assert!(engine.get_wallet_info(pk).await.is_some());

        let deletion = signed_event(&sk, 5, vec![Tag::E(info.id, vec![])], "");
        let responses = engine.handle_typed(1, ClientMessage::Event(deletion)).await;

        assert!(ok_response(&responses).0);
        assert!(engine.get_wallet_info(pk).await.is_none());
    });
}

#[test]
fn test_handle_typed_blocks_disallowed_kind() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        // A correctly signed kind 1 event is still rejected by the allow list.
        let event = signed_event(&wallet_sk(), 1, vec![], "text");
        let responses = engine.handle_typed(1, ClientMessage::Event(event)).await;
        let (success, msg) = ok_response(&responses);
        assert!(!success);
        assert!(msg.contains("kind not allowed"), "{msg}");

        // A disallowed kind with a broken signature still reports the kind
        // rejection, because the kind check runs before verification.
        let mut broken = signed_event(&wallet_sk(), 1, vec![], "text");
        broken.id = "tampered".into();
        let responses = engine.handle_typed(1, ClientMessage::Event(broken)).await;
        let (success, msg) = ok_response(&responses);
        assert!(!success);
        assert!(msg.contains("kind not allowed"), "{msg}");
    });
}

#[test]
fn test_route_verified_event_deletion_removes_cached_info() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let sk = wallet_sk();
        let pk = crate::common::test_helpers::TEST_WALLET_PK;

        let info = signed_event(&sk, 13194, vec![], "{}");
        engine.process_info_event(info.clone()).await;

        let deletion = signed_event(&sk, 5, vec![Tag::E(info.id, vec![])], "");
        let responses = engine.route_verified_event(1, deletion).await;

        assert!(!responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                message: RelayMessage::Ok(..),
                ..
            }
        )));
        assert!(engine.get_wallet_info(pk).await.is_none());
    });
}

#[test]
fn test_validate_event_accepts_nwc_kinds() {
    set_test_time(crate::util::now());
    let engine = new_test_engine();
    let sk = wallet_sk();

    for kind in [23194u64, 23195, 23196, 23197] {
        let tags = match kind {
            23195 => vec![Tag::p("recipient"), Tag::E("event".into(), vec![])],
            23196 | 23197 => vec![Tag::p("recipient")],
            _ => vec![],
        };
        let event = signed_event(&sk, kind, tags, "");
        assert_eq!(engine.validate_event(&event), Ok(()), "kind {kind}");
    }
}

#[test]
fn test_route_verified_event_delivers_to_subscriber() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        engine.on_connect(2).await;
        let pk = crate::common::test_helpers::TEST_WALLET_PK;

        let responses = engine
            .handle_typed(
                2,
                ClientMessage::Req(
                    "sub1".into(),
                    vec![Filter {
                        kinds: Some(vec![23194]),
                        authors: Some(vec![pk.into()]),
                        ..Default::default()
                    }],
                ),
            )
            .await;
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                message: RelayMessage::Eose(_),
                ..
            }
        )));

        let event = signed_event(&wallet_sk(), 23194, vec![], "payload");
        let event_id = event.id.clone();
        let responses = engine.route_verified_event(1, event).await;

        assert!(
            responses.iter().any(|r| matches!(
                r,
                EngineResponse::Send {
                    recipient_id: 2,
                    message: RelayMessage::Event(sub_id, e),
                } if sub_id == "sub1" && e.id == event_id
            )),
            "subscriber must receive the event: {responses:?}"
        );
    });
}

#[test]
fn test_route_verified_event_wakes_hibernated_subscriber() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let storage = MockStorage::new();
        crate::common::test_helpers::seed_subscription(
            &storage,
            42,
            "sub1",
            crate::common::test_helpers::TEST_WALLET_PK,
            vec![Filter {
                kinds: Some(vec![23194]),
                authors: Some(vec![crate::common::test_helpers::TEST_WALLET_PK.into()]),
                ..Default::default()
            }],
        )
        .await;
        let mut engine = NostrEngine::new_with_storage(storage, Limits::default(), test_now);

        let event = signed_event(&wallet_sk(), 23194, vec![], "payload");
        let responses = engine.route_verified_event(1, event).await;

        assert!(responses
            .iter()
            .any(|r| matches!(r, EngineResponse::WakeUp { connection_id: 42 })));
    });
}

#[test]
fn test_process_req_replays_cached_info() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        let pk = crate::common::test_helpers::TEST_WALLET_PK;

        let info = Event {
            id: "info1".into(),
            pubkey: pk.into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        engine.process_info_event(info.clone()).await;

        let responses = engine
            .process_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![13194]),
                    authors: Some(vec![pk.into()]),
                    ..Default::default()
                }],
            )
            .await;

        let event_count = responses
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    EngineResponse::Send {
                        message: RelayMessage::Event(..),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(
            event_count, 1,
            "cached info must be replayed: {responses:?}"
        );
        assert!(matches!(
            responses.last(),
            Some(EngineResponse::Send { message: RelayMessage::Eose(sub), .. }) if sub == "sub1"
        ));
    });
}

#[test]
fn test_process_req_limit_stops_after_cached_info() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        for (i, pk) in ["pk1", "pk2"].iter().enumerate() {
            let info = Event {
                id: format!("info{i}"),
                pubkey: (*pk).into(),
                created_at: test_now(),
                kind: 13194,
                tags: vec![],
                content: String::new(),
                sig: "sig".into(),
            };
            engine.process_info_event(info).await;
        }

        let responses = engine
            .process_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![13194]),
                    authors: Some(vec!["pk1".into(), "pk2".into()]),
                    limit: Some(1),
                    ..Default::default()
                }],
            )
            .await;

        let events = responses
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    EngineResponse::Send {
                        message: RelayMessage::Event(..),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(events, 1, "limit must cap replayed info events");
        assert!(matches!(
            responses.last(),
            Some(EngineResponse::Send {
                message: RelayMessage::Eose(_),
                ..
            })
        ));
    });
}

#[test]
fn test_engine_on_disconnect_keeps_persisted_state() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(7).await;
        let _ = engine
            .process_req(
                7,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;

        let responses = engine.on_disconnect(7).await;
        assert!(responses.is_empty());

        // The live index is gone, but the subscription is still in storage.
        assert!(engine.load(7).await);
    });
}

#[test]
fn test_engine_add_connection_and_load() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        let mut subs = HashMap::new();
        subs.insert(
            "sub1".to_string(),
            vec![Filter {
                authors: Some(vec!["alice".into()]),
                ..Default::default()
            }],
        );

        engine.add_connection(9, subs).await;
        assert!(engine.load(9).await);

        let responses = engine
            .process_event(
                1,
                Event {
                    id: "e1".into(),
                    pubkey: "alice".into(),
                    created_at: test_now(),
                    kind: 23194,
                    tags: vec![],
                    content: String::new(),
                    sig: "sig".into(),
                },
            )
            .await;
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                recipient_id: 9,
                ..
            }
        )));
    });
}

#[test]
fn test_engine_load_by_pubkey() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(5).await;
        let _ = engine
            .process_req(
                5,
                "sub1".into(),
                vec![Filter {
                    p_tags: Some(vec!["wallet_pk".into()]),
                    ..Default::default()
                }],
            )
            .await;

        assert_eq!(engine.load_by_pubkey("wallet_pk").await, vec![5]);
        assert!(engine.load_by_pubkey("unknown_pk").await.is_empty());
    });
}

#[test]
fn test_kind_5_deletion_with_unknown_e_tag() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        let info = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        engine.process_info_event(info).await;

        let deletion = Event {
            id: "del1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![Tag::E("unknown_event".into(), vec![])],
            content: String::new(),
            sig: "sig".into(),
        };
        engine.process_deletion_event(&deletion).await;

        assert!(engine.get_wallet_info("alice").await.is_some());
    });
}

#[test]
fn test_kind_5_deletion_with_unrelated_k_tag() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        let info = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        engine.process_info_event(info).await;

        let deletion = Event {
            id: "del1".into(),
            pubkey: "alice".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![Tag::Other("k".into(), vec![serde_json::json!("23194")])],
            content: String::new(),
            sig: "sig".into(),
        };
        engine.process_deletion_event(&deletion).await;

        assert!(engine.get_wallet_info("alice").await.is_some());
    });
}

#[test]
fn test_engine_load_unknown_connection_is_false() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        assert!(
            !engine.load(4242).await,
            "a connection that was never persisted cannot be loaded"
        );
    });
}

// ── Filter reach: `#p` info replay, `#e` / `ids`-only subscriptions ──

fn info_event_for(id: &str, p_tags: Vec<Tag>) -> Event {
    Event {
        id: id.into(),
        pubkey: "wallet".into(),
        created_at: test_now(),
        kind: 13194,
        tags: p_tags,
        content: "get_info".into(),
        sig: "sig".into(),
    }
}

fn replay_events(responses: &[EngineResponse]) -> Vec<(String, String)> {
    responses
        .iter()
        .filter_map(|r| match r {
            EngineResponse::Send {
                message: RelayMessage::Event(sub_id, event),
                ..
            } => Some((sub_id.clone(), event.id.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn test_req_replays_info_for_p_tag_filter() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        engine
            .process_info_event(info_event_for("info1", vec![Tag::p("app")]))
            .await;

        // NWA apps address the wallet through the info event's `p` tag.
        let msg =
            ClientMessage::from_json(r##"["REQ","nwa",{"kinds":[13194],"#p":["app"]}]"##).unwrap();
        let responses = engine.handle_typed(1, msg).await;

        assert_eq!(
            replay_events(&responses),
            vec![("nwa".to_string(), "info1".to_string())]
        );
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                message: RelayMessage::Eose(_),
                ..
            }
        )));
    });
}

#[test]
fn test_req_replays_info_once_when_author_and_p_tag_match() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        engine
            .process_info_event(info_event_for("info1", vec![Tag::p("app")]))
            .await;

        let msg = ClientMessage::from_json(
            r##"["REQ","both",{"kinds":[13194],"authors":["wallet"],"#p":["app"]}]"##,
        )
        .unwrap();
        let responses = engine.handle_typed(1, msg).await;

        assert_eq!(
            replay_events(&responses).len(),
            1,
            "the same cached info event must not be replayed twice"
        );
    });
}

#[test]
fn test_e_tag_only_subscription_receives_event() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let msg = ClientMessage::from_json(r##"["REQ","sub-e",{"kinds":[23195],"#e":["req1"]}]"##)
            .unwrap();
        let _ = engine.handle_typed(1, msg).await;

        let event = signed_event(
            &wallet_sk(),
            23195,
            vec![
                Tag::p(crate::common::test_helpers::TEST_WALLET_PK),
                Tag::E("req1".into(), vec![]),
            ],
            "ciphertext",
        );
        let responses = engine
            .handle_typed(1, ClientMessage::Event(event.clone()))
            .await;

        assert!(ok_response(&responses).0, "event must be accepted");
        assert_eq!(
            replay_events(&responses),
            vec![("sub-e".to_string(), event.id)]
        );
    });
}

#[test]
fn test_ids_only_subscription_receives_event() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let event = signed_event(
            &wallet_sk(),
            23195,
            vec![
                Tag::p(crate::common::test_helpers::TEST_WALLET_PK),
                Tag::E("req1".into(), vec![]),
            ],
            "ciphertext",
        );
        let msg =
            ClientMessage::from_json(&format!(r#"["REQ","sub-id",{{"ids":["{}"]}}]"#, event.id))
                .unwrap();
        let _ = engine.handle_typed(1, msg).await;

        let responses = engine
            .handle_typed(1, ClientMessage::Event(event.clone()))
            .await;

        assert_eq!(
            replay_events(&responses),
            vec![("sub-id".to_string(), event.id)]
        );
    });
}
