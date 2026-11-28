use super::super::{ClientMessage, Tag};
use super::*;
use crate::common::test_helpers::{
    hex_key, new_test_engine, set_test_time, test_limits, test_now, MockStorage,
};

/// Drives the engine the way `CloudflareTransport::websocket_message` does:
/// an `EVENT` is validated, acknowledged with `OK`, then routed; `REQ` and
/// `CLOSE` go straight to the engine. Tests use it so they exercise the same
/// path as production.
async fn handle_typed(
    engine: &mut NostrEngine<MockStorage>,
    id: u32,
    message: ClientMessage,
) -> Vec<EngineResponse> {
    match message {
        ClientMessage::Event(event) => match engine.validate_event(&event) {
            Ok(()) => {
                let mut responses = vec![EngineResponse::send(
                    id,
                    RelayMessage::Ok(event.id.clone(), true, String::new()),
                )];
                responses.extend(engine.route_verified_event(id, event).await);
                responses
            }
            Err((event_id, error)) => vec![EngineResponse::send(
                id,
                RelayMessage::Ok(event_id, false, error),
            )],
        },
        ClientMessage::Req(sub_id, filters) => engine.handle_req(id, sub_id, filters).await,
        ClientMessage::Close(sub_id) => engine.process_close(id, sub_id).await,
    }
}

/// Acknowledges and routes `event` without verifying it, for tests that only
/// care about routing and use unsigned fixtures.
async fn route_verified(
    engine: &mut NostrEngine<MockStorage>,
    id: u32,
    event: Event,
) -> Vec<EngineResponse> {
    let mut responses = vec![EngineResponse::send(
        id,
        RelayMessage::Ok(event.id.clone(), true, String::new()),
    )];
    responses.extend(engine.route_verified_event(id, event).await);
    responses
}

/// Whether a wallet info event is cached for `pubkey`.
async fn has_info(engine: &mut NostrEngine<MockStorage>, pubkey: &str) -> bool {
    engine.registry.get_info(pubkey).await.is_some()
}

#[test]
fn test_engine_req_storage() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let req = format!(
            r#"["REQ", "sub1", {{"kinds": [23194], "authors": ["{}"]}}]"#,
            hex_key("pk1")
        );
        let msg = ClientMessage::from_json(&req).unwrap();
        let responses = handle_typed(&mut engine, 1, msg).await;

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

        assert!(has_info(&mut engine, "pk1").await);
    });
}

#[test]
fn test_get_wallet_info_none() {
    let mut engine = new_test_engine();
    futures::executor::block_on(async {
        assert!(!has_info(&mut engine, "pk1").await);
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

        let responses = route_verified(&mut engine, 2, event).await;

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
        let responses_after = route_verified(
            &mut engine,
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

        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);

        // 2. Handle an event that targets the hibernated pubkey
        let event = Event {
            id: "event1".into(),
            pubkey: pk.into(),
            created_at: test_now(),
            kind: 23194,
            tags: vec![super::super::Tag::P(String::from(pk), vec![])],
            content: "wake up!".into(),
            sig: "sig".into(),
        };

        let responses = route_verified(&mut engine, 99, event).await;

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
        let responses = handle_typed(&mut engine, 1, msg).await;

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
        let responses = handle_typed(&mut engine, 1, msg).await;

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
        assert!(has_info(&mut engine, "alice").await);

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
        assert!(!has_info(&mut engine, "alice").await);
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
        assert!(has_info(&mut engine, "alice").await);

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
        assert!(has_info(&mut engine, "alice").await);
    });
}

#[test]
fn test_kind_5_deletion_with_e_tag_after_hibernation() {
    futures::executor::block_on(async {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let data = Arc::new(Mutex::new(HashMap::new()));
        let storage = MockStorage {
            data: data.clone(),
            fail_put_batch: false,
        };
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let sk = wallet_sk();
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let info = signed_event(&sk, 13194, vec![Tag::P(String::from("app"), vec![])], "{}");
        engine.process_info_event(info.clone()).await;
        assert!(has_info(&mut engine, pk).await);
        assert!(
            !data.lock().unwrap().contains_key("info-p:app"),
            "caching a p-tagged info event must not write an alias"
        );

        // Simulate hibernation: only the persisted keys survive.
        let snapshot = data.lock().unwrap().clone();
        let restarted_storage = MockStorage::new();
        *restarted_storage.data.lock().unwrap() = snapshot;
        let mut restarted =
            NostrEngine::new_with_storage(restarted_storage, test_limits(), test_now);
        restarted.on_connect(1).await;

        let deletion = signed_event(&sk, 5, vec![Tag::E(info.id.clone(), vec![])], "");
        restarted.process_deletion_event(&deletion).await;

        assert!(!has_info(&mut restarted, pk).await);
    });
}

#[test]
fn test_kind_5_deletion_with_e_tag_after_hibernation_unauthorized() {
    futures::executor::block_on(async {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let data = Arc::new(Mutex::new(HashMap::new()));
        let storage = MockStorage {
            data: data.clone(),
            fail_put_batch: false,
        };
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let sk = wallet_sk();
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let info = signed_event(&sk, 13194, vec![], "{}");
        engine.process_info_event(info.clone()).await;

        let snapshot = data.lock().unwrap().clone();
        let restarted_storage = MockStorage::new();
        *restarted_storage.data.lock().unwrap() = snapshot;
        let mut restarted =
            NostrEngine::new_with_storage(restarted_storage, test_limits(), test_now);
        restarted.on_connect(1).await;

        // Another author cannot delete an event it did not sign.
        let attacker = Event {
            id: "del1".into(),
            pubkey: "attacker".into(),
            created_at: test_now(),
            kind: 5,
            tags: vec![Tag::E(info.id.clone(), vec![])],
            content: String::new(),
            sig: "sig".into(),
        };
        restarted.process_deletion_event(&attacker).await;

        assert!(has_info(&mut restarted, pk).await);
    });
}

#[test]
fn test_kind_5_deletion_by_old_e_tag_keeps_republished_info() {
    futures::executor::block_on(async {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let data = Arc::new(Mutex::new(HashMap::new()));
        let storage = MockStorage {
            data: data.clone(),
            fail_put_batch: false,
        };
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let sk = wallet_sk();
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let old = signed_event(&sk, 13194, vec![], "{}");
        engine.process_info_event(old.clone()).await;

        let snapshot = data.lock().unwrap().clone();
        let restarted_storage = MockStorage::new();
        *restarted_storage.data.lock().unwrap() = snapshot;
        let mut restarted =
            NostrEngine::new_with_storage(restarted_storage, test_limits(), test_now);
        restarted.on_connect(1).await;

        // Republish, then delete using the superseded event id.
        set_test_time(test_now() + 1);
        let new = signed_event(&sk, 13194, vec![], "{}");
        restarted.process_info_event(new).await;

        let deletion = signed_event(&sk, 5, vec![Tag::E(old.id, vec![])], "");
        restarted.process_deletion_event(&deletion).await;

        assert!(
            has_info(&mut restarted, pk).await,
            "the newer info event must survive deletion of the old id"
        );
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
        assert!(has_info(&mut engine, "alice").await);

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

        assert!(!has_info(&mut engine, "alice").await);
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
        assert!(has_info(&mut engine, "alice").await);
    });
}

#[test]
fn test_handle_close_removes_subscription() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let msg = ClientMessage::Close("sub1".into());
        let responses = handle_typed(&mut engine, 1, msg).await;
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
        let responses_after = route_verified(
            &mut engine,
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

/// The internal RPC session must leave no persisted state,
/// or `load_indexed` resurrects it on the next round trip.
#[test]
fn test_internal_rpc_session_terminate_leaves_no_state() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);

        engine.on_connect(42).await;
        let _ = engine
            .handle_req(
                42,
                "rpc_sub".to_string(),
                vec![Filter {
                    e_tags: Some(vec!["req-1".into()]),
                    p_tags: Some(vec!["app-pk".into()]),
                    ..Default::default()
                }],
            )
            .await;

        engine.on_terminate(42).await;

        let keys: Vec<String> = data.lock().unwrap().keys().cloned().collect();
        assert!(
            keys.is_empty(),
            "terminated session leaked persisted state: {keys:?}"
        );
        assert!(
            !engine.load(42).await,
            "terminated session must not restore"
        );
    });
}

/// Success path: `CLOSE` alone must prune `etag:`.
#[test]
fn test_close_prunes_internal_rpc_indexes() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);

        engine.on_connect(7).await;
        let _ = engine
            .handle_req(
                7,
                "rpc_sub".to_string(),
                vec![Filter {
                    e_tags: Some(vec!["req-9".into()]),
                    p_tags: Some(vec!["app-pk".into()]),
                    ..Default::default()
                }],
            )
            .await;

        let _ = engine.process_close(7, "rpc_sub".to_string()).await;

        let keys: Vec<String> = data.lock().unwrap().keys().cloned().collect();
        assert!(
            keys.is_empty(),
            "CLOSE left persisted state behind: {keys:?}"
        );
        assert!(!engine.load(7).await);
    });
}

#[test]
fn test_req_filter_too_broad() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;

        let msg = ClientMessage::Req("sub1".into(), vec![Filter::default()]);
        let responses = handle_typed(&mut engine, 1, msg).await;
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
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let responses = engine
            .handle_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    authors: Some(vec![hex_key("alice")]),
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
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let responses = engine
            .handle_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    authors: Some(vec![hex_key("alice")]),
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
        assert!(has_info(&mut engine, "alice").await);

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

        assert!(!has_info(&mut engine, "alice").await);
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

        let event = signed_event(
            &wallet_sk(),
            13194,
            vec![Tag::Other(
                "encryption".into(),
                vec![serde_json::json!("nip44_v2")],
            )],
            "{}",
        );
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(event)).await;

        assert!(ok_response(&responses).0);
        assert!(
            has_info(&mut engine, crate::common::test_helpers::TEST_WALLET_PK).await,
            "info event must be cached"
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
        assert!(has_info(&mut engine, pk).await);

        let deletion = signed_event(&sk, 5, vec![Tag::E(info.id, vec![])], "");
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(deletion)).await;

        assert!(ok_response(&responses).0);
        assert!(!has_info(&mut engine, pk).await);
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
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(event)).await;
        let (success, msg) = ok_response(&responses);
        assert!(!success);
        assert!(msg.contains("kind not allowed"), "{msg}");

        // A disallowed kind with a broken signature still reports the kind
        // rejection, because the kind check runs before verification.
        let mut broken = signed_event(&wallet_sk(), 1, vec![], "text");
        broken.id = "tampered".into();
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(broken)).await;
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
        assert!(!has_info(&mut engine, pk).await);
    });
}

#[test]
fn test_validate_event_accepts_nwc_kinds() {
    set_test_time(crate::util::now());
    let engine = new_test_engine();
    let sk = wallet_sk();

    for kind in [23194u64, 23195, 23196, 23197] {
        let tags = match kind {
            23195 => vec![
                Tag::P(String::from("recipient"), vec![]),
                Tag::E("event".into(), vec![]),
            ],
            23196 | 23197 => vec![Tag::P(String::from("recipient"), vec![])],
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

        let responses = handle_typed(
            &mut engine,
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
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);

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

        let responses = route_verified(
            &mut engine,
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

        assert!(has_info(&mut engine, "alice").await);
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

        assert!(has_info(&mut engine, "alice").await);
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
        pubkey: hex_key("wallet"),
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
fn test_req_does_not_replay_info_for_a_p_tag_scoped_app() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        engine
            .process_info_event(info_event_for(
                "info1",
                vec![Tag::P(hex_key("app"), vec![])],
            ))
            .await;

        // A wallet announcing itself to an app through a `p` tag is not a
        // discovery path this relay serves: the info event is cached under its
        // author only, so an app-scoped REQ gets EOSE and nothing else.
        let req = format!(
            r##"["REQ","nwa",{{"kinds":[13194],"#p":["{}"]}}]"##,
            hex_key("app")
        );
        let msg = ClientMessage::from_json(&req).unwrap();
        let responses = handle_typed(&mut engine, 1, msg).await;

        assert!(replay_events(&responses).is_empty());
        assert!(responses.iter().any(|r| matches!(
            r,
            EngineResponse::Send {
                message: RelayMessage::Eose(_),
                ..
            }
        )));

        // The same event still reaches a subscriber that names the author.
        let req = format!(
            r##"["REQ","author",{{"kinds":[13194],"authors":["{}"]}}]"##,
            hex_key("wallet")
        );
        let msg = ClientMessage::from_json(&req).unwrap();
        let responses = handle_typed(&mut engine, 1, msg).await;
        assert_eq!(
            replay_events(&responses),
            vec![("author".to_string(), "info1".to_string())]
        );
    });
}

#[test]
fn test_req_replays_info_once_when_author_and_p_tag_match() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        // Both filter values resolve to the same cached event, so the replay
        // must deduplicate them.
        engine
            .process_info_event(info_event_for(
                "info1",
                vec![Tag::P(hex_key("wallet"), vec![])],
            ))
            .await;

        let wallet = hex_key("wallet");
        let req = format!(
            r##"["REQ","both",{{"kinds":[13194],"authors":["{wallet}"],"#p":["{wallet}"]}}]"##
        );
        let msg = ClientMessage::from_json(&req).unwrap();
        let responses = handle_typed(&mut engine, 1, msg).await;

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

        let request_id = hex_key("req1");
        let req = format!(r##"["REQ","sub-e",{{"kinds":[23195],"#e":["{request_id}"]}}]"##);
        let msg = ClientMessage::from_json(&req).unwrap();
        let _ = handle_typed(&mut engine, 1, msg).await;

        let event = signed_event(
            &wallet_sk(),
            23195,
            vec![
                Tag::P(
                    String::from(crate::common::test_helpers::TEST_WALLET_PK),
                    vec![],
                ),
                Tag::E(request_id, vec![]),
            ],
            "ciphertext",
        );
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(event.clone())).await;

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
                Tag::P(
                    String::from(crate::common::test_helpers::TEST_WALLET_PK),
                    vec![],
                ),
                Tag::E("req1".into(), vec![]),
            ],
            "ciphertext",
        );
        let msg =
            ClientMessage::from_json(&format!(r#"["REQ","sub-id",{{"ids":["{}"]}}]"#, event.id))
                .unwrap();
        let _ = handle_typed(&mut engine, 1, msg).await;

        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(event.clone())).await;

        assert_eq!(
            replay_events(&responses),
            vec![("sub-id".to_string(), event.id)]
        );
    });
}

#[test]
fn test_route_delivers_to_every_subscribed_connection() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        engine.on_connect(2).await;
        let pk = crate::common::test_helpers::TEST_WALLET_PK;

        for (conn, sub_id) in [(1u32, "app_a"), (2, "app_b")] {
            engine
                .handle_req(
                    conn,
                    sub_id.into(),
                    vec![Filter {
                        kinds: Some(vec![23194]),
                        authors: Some(vec![pk.into()]),
                        ..Default::default()
                    }],
                )
                .await;
        }

        let event = signed_event(&wallet_sk(), 23194, vec![], "payload");
        let event_id = event.id.clone();
        let responses = engine.route_verified_event(3, event).await;

        let mut recipients: Vec<(u32, String)> = responses
            .iter()
            .filter_map(|r| match r {
                EngineResponse::Send {
                    recipient_id,
                    message: RelayMessage::Event(sub_id, e),
                } if e.id == event_id => Some((*recipient_id, sub_id.clone())),
                _ => None,
            })
            .collect();
        recipients.sort();
        assert_eq!(
            recipients,
            vec![(1, "app_a".to_string()), (2, "app_b".to_string())],
            "both subscribers must receive the event: {responses:?}"
        );
    });
}

#[test]
fn test_route_delivers_to_same_sub_id_on_two_connections() {
    futures::executor::block_on(async {
        set_test_time(crate::util::now());
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        engine.on_connect(2).await;
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let filter = Filter {
            kinds: Some(vec![23194]),
            authors: Some(vec![pk.into()]),
            ..Default::default()
        };

        for conn in [1u32, 2] {
            engine
                .handle_req(conn, "sub1".into(), vec![filter.clone()])
                .await;
        }

        let event = signed_event(&wallet_sk(), 23194, vec![], "payload");
        let event_id = event.id.clone();
        let responses = engine.route_verified_event(3, event).await;

        let mut recipients: Vec<u32> = responses
            .iter()
            .filter_map(|r| match r {
                EngineResponse::Send {
                    recipient_id,
                    message: RelayMessage::Event(_, e),
                } if e.id == event_id => Some(*recipient_id),
                _ => None,
            })
            .collect();
        recipients.sort_unstable();
        assert_eq!(recipients, vec![1, 2]);
    });
}

#[test]
fn test_handle_req_rejects_filter_with_too_many_values() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let authors: Vec<String> = (0..11).map(|i| format!("author{i}")).collect();
        let responses = engine
            .handle_req(
                1,
                "sub1".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    authors: Some(authors),
                    ..Default::default()
                }],
            )
            .await;

        assert_eq!(responses.len(), 1, "expected only a CLOSED: {responses:?}");
        match &responses[0] {
            EngineResponse::Send {
                recipient_id: 1,
                message: RelayMessage::Closed(sub_id, reason),
            } => {
                assert_eq!(sub_id, "sub1");
                assert_eq!(reason, "filter too large (max 10 values per field)");
            }
            other => panic!("expected CLOSED, got: {other:?}"),
        }
        assert!(
            data.lock().unwrap().is_empty(),
            "a rejected filter must not write routing keys"
        );
    });
}

#[test]
fn test_handle_req_rejects_oversized_sub_id() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;

        let sub_id = "s".repeat(crate::nostr::limits::MAX_SUB_ID_LENGTH + 1);
        let responses = engine
            .handle_req(
                1,
                sub_id.clone(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;

        assert_eq!(responses.len(), 1, "expected only a CLOSED: {responses:?}");
        match &responses[0] {
            EngineResponse::Send {
                recipient_id: 1,
                message: RelayMessage::Closed(closed_sub_id, reason),
            } => {
                assert_eq!(closed_sub_id, &sub_id);
                assert!(
                    reason.contains("subscription id too long"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected CLOSED, got: {other:?}"),
        }
        assert!(
            data.lock().unwrap().is_empty(),
            "a rejected subscription must not write routing keys"
        );
    });
}

#[test]
fn test_handle_req_rejects_routing_key_budget_overrun() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let limits = Limits {
            max_routing_keys_per_connection: 5,
            ..test_limits()
        };
        let mut engine = NostrEngine::new_with_storage(storage, limits, test_now);
        engine.on_connect(1).await;

        // 4 authors + 4 `#p` values = 8 routing keys, all inside the
        // per-field cap but over the connection budget.
        let responses = engine
            .handle_req(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some((0..4).map(|i| hex_key(&format!("author{i}"))).collect()),
                    kinds: Some(vec![23194]),
                    p_tags: Some((0..4).map(|i| hex_key(&format!("payee{i}"))).collect()),
                    ..Default::default()
                }],
            )
            .await;

        assert_eq!(responses.len(), 1, "expected only a CLOSED: {responses:?}");
        match &responses[0] {
            EngineResponse::Send {
                recipient_id: 1,
                message: RelayMessage::Closed(sub_id, reason),
            } => {
                assert_eq!(sub_id, "sub1");
                assert!(
                    reason.contains("too many routing keys"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected CLOSED, got: {other:?}"),
        }
        assert!(
            data.lock().unwrap().is_empty(),
            "a rejected subscription must not write routing keys"
        );
    });
}

#[test]
fn test_engine_has_subscriptions_tracks_req_and_close() {
    futures::executor::block_on(async {
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        assert!(!engine.has_subscriptions(1).await);

        let req = format!(
            r#"["REQ", "sub1", {{"kinds": [23194], "authors": ["{}"]}}]"#,
            hex_key("pk1")
        );
        let msg = ClientMessage::from_json(&req).unwrap();
        handle_typed(&mut engine, 1, msg).await;
        assert!(engine.has_subscriptions(1).await);

        let close = r#"["CLOSE", "sub1"]"#;
        let msg = ClientMessage::from_json(close).unwrap();
        handle_typed(&mut engine, 1, msg).await;
        assert!(!engine.has_subscriptions(1).await);
    });
}

#[test]
fn test_engine_prune_info_cache_removes_expired_events() {
    futures::executor::block_on(async {
        set_test_time(1_000_000);
        let limits = Limits {
            info_ttl_seconds: 60,
            ..test_limits()
        };
        let mut engine = NostrEngine::new_with_storage(MockStorage::new(), limits, test_now);
        engine.on_connect(1).await;

        engine
            .process_info_event(Event {
                id: "info1".into(),
                pubkey: "wallet".into(),
                created_at: 999_980,
                kind: 13194,
                tags: vec![],
                content: String::new(),
                sig: "sig".into(),
            })
            .await;
        assert!(has_info(&mut engine, "wallet").await);

        set_test_time(1_000_100);
        assert_eq!(engine.prune_info_cache().await, 1);
        assert!(!has_info(&mut engine, "wallet").await);

        set_test_time(1_700_000_000);
    });
}

#[test]
fn test_engine_prune_retired_keys_removes_stale_keys() {
    futures::executor::block_on(async {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        let data = Arc::new(Mutex::new(HashMap::new()));
        let storage = MockStorage {
            data: data.clone(),
            fail_put_batch: false,
        };
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);

        // Keys left behind by the version that indexed `p` tags.
        {
            let mut data = data.lock().unwrap();
            data.insert("info-p:app".to_string(), serde_json::json!("wallet"));
            data.insert("info-p:other".to_string(), serde_json::json!("wallet2"));
        }

        assert_eq!(engine.prune_retired_keys().await, 2);

        {
            let data = data.lock().unwrap();
            assert!(data.get("info-p:app").is_none());
            assert!(data.get("info-p:other").is_none());
        }

        // Nothing left to sweep.
        assert_eq!(engine.prune_retired_keys().await, 0);
    });
}

#[test]
fn test_engine_prune_orphaned_connections_reports_the_count() {
    futures::executor::block_on(async {
        set_test_time(1_700_000_000);
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let filter = Filter {
            kinds: Some(vec![23194]),
            authors: Some(vec![hex_key("alice")]),
            ..Default::default()
        };
        engine.handle_req(1, "s".into(), vec![filter]).await;

        let none_live = std::collections::HashSet::new();
        assert_eq!(engine.prune_orphaned_connections(&none_live, 3600).await, 1);
        assert_eq!(engine.prune_orphaned_connections(&none_live, 3600).await, 0);
    });
}

fn closed_reason(responses: &[EngineResponse]) -> Option<String> {
    responses.iter().find_map(|r| match r {
        EngineResponse::Send {
            message: RelayMessage::Closed(_, reason),
            ..
        } => Some(reason.clone()),
        _ => None,
    })
}

fn author_filter(label: &str) -> Filter {
    Filter {
        kinds: Some(vec![23194]),
        authors: Some(vec![hex_key(label)]),
        ..Default::default()
    }
}

#[test]
fn test_req_over_the_filter_limit_is_closed_and_writes_nothing() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;
        let max = test_limits().max_filters_per_req;

        let filters = (0..=max).map(|i| author_filter(&format!("w{i}"))).collect();
        let responses = engine.handle_req(1, "big".into(), filters).await;
        assert_eq!(
            closed_reason(&responses),
            Some(format!("too many filters (max {max})"))
        );
        assert!(data.lock().unwrap().is_empty());

        let filters = (0..max).map(|i| author_filter(&format!("w{i}"))).collect();
        let responses = engine.handle_req(1, "fits".into(), filters).await;
        assert_eq!(closed_reason(&responses), None);
    });
}

#[test]
fn test_req_with_a_non_hex_filter_value_is_closed_and_writes_nothing() {
    futures::executor::block_on(async {
        let good = hex_key("wallet");
        let bad_values = [
            "alice".to_string(),
            good.to_uppercase(),
            good[..63].to_string(),
            format!("{good}0"),
        ];
        for bad in bad_values {
            for field in ["ids", "authors", "#p", "#e"] {
                let storage = MockStorage::new();
                let data = storage.data.clone();
                let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
                engine.on_connect(1).await;
                let filter: Filter = serde_json::from_value(serde_json::json!({
                    "kinds": [23194],
                    field: [bad],
                }))
                .unwrap();
                let responses = engine.handle_req(1, "s".into(), vec![filter]).await;
                let reason = closed_reason(&responses).unwrap_or_default();
                assert!(
                    reason.starts_with("invalid filter value"),
                    "{field}={bad}: {reason}"
                );
                assert!(data.lock().unwrap().is_empty(), "{field}={bad}");
            }
        }
    });
}

/// `MockStorage` that records which `info:` keys were read.
struct InfoReads {
    inner: MockStorage,
    keys: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

#[async_trait::async_trait(?Send)]
impl crate::nostr::wallet_registry::Storage for InfoReads {
    async fn get(&self, key: &str) -> Option<serde_json::Value> {
        if key.starts_with("info:") {
            self.keys.borrow_mut().push(key.to_string());
        }
        self.inner.get(key).await
    }
    async fn put_batch(
        &self,
        entries: std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), crate::nostr::error::StorageError> {
        self.inner.put_batch(entries).await
    }
    async fn delete_batch(&self, keys: Vec<String>) {
        self.inner.delete_batch(keys).await;
    }
    async fn list_page(
        &self,
        prefix: &str,
        start_after: Option<&str>,
        limit: usize,
    ) -> Vec<(String, serde_json::Value)> {
        self.inner.list_page(prefix, start_after, limit).await
    }
}

#[test]
fn test_replay_reads_each_wallet_once_however_many_filters_name_it() {
    futures::executor::block_on(async {
        let keys = std::rc::Rc::default();
        let storage = InfoReads {
            inner: MockStorage::new(),
            keys: std::rc::Rc::clone(&keys),
        };
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;
        let wallet = hex_key("wallet");
        let named_twice = Filter {
            kinds: Some(vec![13194]),
            authors: Some(vec![wallet.clone()]),
            p_tags: Some(vec![wallet.clone()]),
            ..Default::default()
        };

        let filters = vec![named_twice.clone(), named_twice.clone(), named_twice];
        engine.handle_req(1, "s".into(), filters).await;

        assert_eq!(*keys.borrow(), vec![format!("info:{wallet}")]);
    });
}

fn info_req(sub_id: &str, filter: &serde_json::Value) -> ClientMessage {
    ClientMessage::from_json(&serde_json::json!(["REQ", sub_id, filter]).to_string()).unwrap()
}

/// A forged info event: the id is right for the wallet's key, but another key
/// signed it. Without the signature check anyone could replace a wallet's
/// advertised methods and encryption.
#[test]
fn test_forged_info_event_is_refused_and_not_cached() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut engine = NostrEngine::new_with_storage(storage, test_limits(), test_now);
        engine.on_connect(1).await;
        let forger = SigningKey::from_slice(&[2u8; 32]).unwrap();
        let mut forged = signed_event(&wallet_sk(), 13194, vec![], "nip04");
        let (_, id_bytes) = Event::compute_id(
            &forged.pubkey,
            forged.created_at,
            forged.kind,
            &forged.tags,
            &forged.content,
        )
        .unwrap();
        forged.sig = hex::encode(forger.sign_prehash(&id_bytes).unwrap().to_bytes());

        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(forged)).await;

        assert_eq!(
            ok_response(&responses),
            (false, "invalid: signature verification failed".to_string())
        );
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        assert!(!data.lock().unwrap().contains_key(&format!("info:{pk}")));
        assert!(!has_info(&mut engine, pk).await);
    });
}

/// An info event reaches a live subscriber, even one older than the cached
/// event: it is routed like any event but does not replace the cache.
#[test]
fn test_info_events_reach_live_subscribers_even_when_not_cached() {
    futures::executor::block_on(async {
        let now = 1_700_000_000;
        set_test_time(now);
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let req = info_req(
            "info",
            &serde_json::json!({"kinds": [13194], "authors": [pk]}),
        );
        handle_typed(&mut engine, 1, req).await;

        let newer = signed_event(&wallet_sk(), 13194, vec![], "newer");
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(newer.clone())).await;
        assert_eq!(
            replay_events(&responses),
            vec![("info".to_string(), newer.id.clone())]
        );

        set_test_time(now - 10);
        let older = signed_event(&wallet_sk(), 13194, vec![], "older");
        set_test_time(now);
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(older.clone())).await;
        assert_eq!(ok_response(&responses), (true, String::new()));
        assert_eq!(
            replay_events(&responses),
            vec![("info".to_string(), older.id)]
        );

        let cached = engine.registry.get_info(pk).await.unwrap();
        assert_eq!(cached.id, newer.id);
    });
}

/// Replay is by author key, but the event must still match the filter.
#[test]
fn test_replay_sends_only_cached_info_that_matches_the_filter() {
    futures::executor::block_on(async {
        set_test_time(1_700_000_000);
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let info = signed_event(&wallet_sk(), 13194, vec![], "pay_invoice");
        handle_typed(&mut engine, 1, ClientMessage::Event(info.clone())).await;

        let other_kind = serde_json::json!({"kinds": [23195], "authors": [pk]});
        let too_new =
            serde_json::json!({"kinds": [13194], "authors": [pk], "since": info.created_at + 1});
        for (sub_id, filter) in [("other-kind", other_kind), ("too-new", too_new)] {
            let responses = handle_typed(&mut engine, 1, info_req(sub_id, &filter)).await;
            assert!(replay_events(&responses).is_empty(), "{sub_id}");
        }

        let matching = serde_json::json!({"kinds": [13194], "authors": [pk]});
        let responses = handle_typed(&mut engine, 1, info_req("match", &matching)).await;
        assert_eq!(
            replay_events(&responses),
            vec![("match".to_string(), info.id)]
        );
    });
}

/// With an `e` tag the deletion is by id only: a `k` tag next to it must not
/// widen it into deleting the author's info event.
#[test]
fn test_deletion_with_e_and_k_tags_deletes_by_id_only() {
    futures::executor::block_on(async {
        set_test_time(1_700_000_000);
        let mut engine = new_test_engine();
        engine.on_connect(1).await;
        let pk = crate::common::test_helpers::TEST_WALLET_PK;
        let info = signed_event(&wallet_sk(), 13194, vec![], "pay_invoice");
        handle_typed(&mut engine, 1, ClientMessage::Event(info)).await;

        let deletion = signed_event(
            &wallet_sk(),
            5,
            vec![
                Tag::E(hex_key("some other event"), vec![]),
                Tag::Other("k".into(), vec![serde_json::json!("13194")]),
            ],
            "",
        );
        let responses = handle_typed(&mut engine, 1, ClientMessage::Event(deletion)).await;

        assert!(ok_response(&responses).0);
        assert!(has_info(&mut engine, pk).await);
    });
}
