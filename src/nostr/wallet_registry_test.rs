use super::*;
use crate::common::test_helpers::{seed_subscription, simulate_hibernation, MockStorage};
use crate::nostr::Tag;

fn has_subscription(registry: &WalletRegistry<MockStorage>, conn_id: u32, sub_id: &str) -> bool {
    registry
        .index
        .get_subscriptions(conn_id)
        .contains_key(sub_id)
}

#[test]
fn test_registry_sub_persistence() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let filters = vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }];

        registry.subscribe(1, "sub1".into(), filters).await.unwrap();

        let data = registry.storage.data.lock().unwrap();
        assert!(
            data.contains_key("conn:1"),
            "Storage should contain connection state"
        );
        assert!(
            data.contains_key("pk:alice"),
            "Storage should contain pubkey index"
        );
    });
}

#[test]
fn test_hibernation_contract() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let mut registry2 = simulate_hibernation(&registry.storage).await;

        let event = Event {
            id: "e1".into(),
            pubkey: "alice".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        let responses = registry2.match_event(&event).await;
        assert!(
            responses.contains(&RegistryResponse::WakeUp(1)),
            "hibernated connection must produce WakeUp"
        );
        assert!(
            responses.contains(&RegistryResponse::Send {
                recipient_id: 1,
                sub_id: "sub1".into()
            }),
            "hibernated connection must produce Send"
        );
    });
}

#[test]
fn test_hibernation_info_survival() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let info = Event {
            id: "info1".into(),
            pubkey: "alice".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![],
            content: "wallet info".into(),
            sig: "sig".into(),
        };
        registry.cache_info(info.clone()).await;

        let mut registry2 = simulate_hibernation(&registry.storage).await;

        let retrieved = registry2.get_info("alice").await;
        assert!(retrieved.is_some(), "info event should survive hibernation");
        assert_eq!(retrieved.unwrap().id, "info1");

        let event = Event {
            id: "e1".into(),
            pubkey: "alice".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        let responses = registry2.match_event(&event).await;
        assert!(
            responses.contains(&RegistryResponse::WakeUp(1)),
            "hibernated connection must still produce WakeUp with info"
        );
        assert!(
            responses.contains(&RegistryResponse::Send {
                recipient_id: 1,
                sub_id: "sub1".into()
            }),
            "hibernated connection must still produce Send with info"
        );
    });
}

#[test]
fn test_hibernation_subscribe_cycles() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        registry.unsubscribe(1, "sub1".into()).await.unwrap();

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["bob".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let mut registry2 = simulate_hibernation(&registry.storage).await;

        let alice_event = Event {
            id: "e1".into(),
            pubkey: "alice".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        let responses = registry2.match_event(&alice_event).await;
        assert!(
            !responses.contains(&RegistryResponse::Send {
                recipient_id: 1,
                sub_id: "sub1".into()
            }),
            "alice event should NOT match after re-subscribe to bob"
        );

        let bob_event = Event {
            id: "e2".into(),
            pubkey: "bob".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: String::new(),
            sig: "sig".into(),
        };
        let responses2 = registry2.match_event(&bob_event).await;
        assert!(
            responses2.contains(&RegistryResponse::WakeUp(1)),
            "bob event must produce WakeUp after re-subscribe"
        );
        assert!(
            responses2.contains(&RegistryResponse::Send {
                recipient_id: 1,
                sub_id: "sub1".into()
            }),
            "bob event must produce Send after re-subscribe"
        );
    });
}

#[test]
fn test_registry_match_routing() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let wallet_pk = "wallet_pk";
        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    p_tags: Some(vec![wallet_pk.into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let event = Event {
            id: "event1".into(),
            pubkey: "app_pk".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![Tag::p(wallet_pk)],
            content: "test".into(),
            sig: "sig".into(),
        };

        let matches = registry.match_event(&event).await;
        assert!(matches.contains(&RegistryResponse::WakeUp(1)));
        assert!(matches.contains(&RegistryResponse::Send {
            recipient_id: 1,
            sub_id: "sub1".into()
        }));
    });
}

#[test]
fn test_registry_lazy_load() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let wallet_pk = "hibernated_pk";
        let conn_id = 42;

        seed_subscription(
            &storage,
            conn_id,
            "sub1",
            wallet_pk,
            vec![Filter {
                p_tags: Some(vec![wallet_pk.into()]),
                ..Default::default()
            }],
        )
        .await;

        let mut registry = WalletRegistry::new(storage, Limits::default());

        let event = Event {
            id: "event1".into(),
            pubkey: "app_pk".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![Tag::p(wallet_pk)],
            content: "test".into(),
            sig: "sig".into(),
        };

        let responses = registry.match_event(&event).await;

        assert!(responses.contains(&RegistryResponse::WakeUp(conn_id)));
        assert!(responses.contains(&RegistryResponse::Send {
            recipient_id: conn_id,
            sub_id: "sub1".into()
        }));
    });
}

#[test]
fn test_index_matching_grouped() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();
        registry
            .subscribe(
                2,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let event_alice = Event {
            id: "id".into(),
            pubkey: "alice".into(),
            kind: 1,
            tags: vec![],
            content: String::new(),
            sig: String::new(),
            created_at: 1000,
        };
        let responses = registry.match_event(&event_alice).await;

        let grouped: Vec<_> = responses
            .iter()
            .filter_map(|r| match r {
                RegistryResponse::Send {
                    recipient_id,
                    sub_id,
                } if sub_id == "sub1" => Some(*recipient_id),
                _ => None,
            })
            .collect();
        assert_eq!(grouped.len(), 1);
        assert!(grouped.contains(&2));
        assert!(!grouped.contains(&1));
    });
}

#[test]
fn test_info_event_caching() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let event = Event {
            id: "id1".into(),
            pubkey: "alice".into(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig1".into(),
            created_at: 1000,
        };
        registry.cache_info(event.clone()).await;

        let stored = registry.get_info("alice").await;
        assert!(stored.is_some());
        assert_eq!(stored.unwrap().id, event.id);
    });
}

#[test]
fn test_info_id_index() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let event = Event {
            id: "id1".into(),
            pubkey: "alice".into(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig1".into(),
            created_at: 1000,
        };
        registry.cache_info(event.clone()).await;

        let pk = registry.find_info_pubkey_by_id("id1").await;
        assert_eq!(pk, Some("alice".into()));

        let pk_missing = registry.find_info_pubkey_by_id("nonexistent").await;
        assert_eq!(pk_missing, None);
    });
}

#[test]
fn test_delete_info() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let event = Event {
            id: "id1".into(),
            pubkey: "alice".into(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig1".into(),
            created_at: 1000,
        };
        registry.cache_info(event.clone()).await;

        assert!(registry.get_info("alice").await.is_some());
        assert_eq!(
            registry.find_info_pubkey_by_id("id1").await,
            Some("alice".into())
        );

        registry.delete_info("alice").await;

        assert!(registry.get_info("alice").await.is_none());
        assert_eq!(registry.find_info_pubkey_by_id("id1").await, None);

        let data = registry.storage.data.lock().unwrap();
        assert!(!data.contains_key("info:alice"));
    });
}

#[test]
fn test_delete_info_preserves_subscriptions() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let event = Event {
            id: "id1".into(),
            pubkey: "alice".into(),
            kind: 13194,
            tags: vec![],
            content: String::new(),
            sig: "sig1".into(),
            created_at: 1000,
        };
        registry.cache_info(event.clone()).await;

        registry.delete_info("alice").await;

        assert!(registry.get_info("alice").await.is_none());
        assert!(has_subscription(&registry, 1, "sub1"));
    });
}

#[test]
fn test_registry_sync() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let data = registry.storage.data.lock().unwrap();
        assert!(data.contains_key("conn:1"));
        assert!(data.contains_key("pk:alice"));
    });
}

#[test]
fn test_registry_terminate() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        registry.on_terminate(1).await;

        let data = registry.storage.data.lock().unwrap();
        assert!(!data.contains_key("conn:1"));
        assert!(!data.contains_key("pk:alice"));
        assert!(!has_subscription(&registry, 1, "sub1"));
    });
}

/// Closing the last subscription must remove `conn:` and
/// its `pk:`/`eid:`/`etag:` entries.
#[test]
fn test_unsubscribe_prunes_persisted_routing_indexes() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "rpc_sub".into(),
                vec![Filter {
                    authors: Some(vec!["app-pk".into()]),
                    ids: Some(vec!["req-id".into()]),
                    e_tags: Some(vec!["req-id".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        {
            let stored = data.lock().unwrap();
            assert!(stored.contains_key("conn:1"));
            assert!(stored.contains_key("pk:app-pk"));
            assert!(stored.contains_key("eid:req-id"));
            assert!(stored.contains_key("etag:req-id"));
        }

        registry.unsubscribe(1, "rpc_sub".into()).await.unwrap();

        let keys: Vec<String> = data.lock().unwrap().keys().cloned().collect();
        assert!(
            keys.is_empty(),
            "unsubscribe left persisted state behind: {keys:?}"
        );
        assert!(!registry.load(1).await);
        assert!(registry.load_by_pubkey("app-pk").await.is_empty());
    });
}

/// Unsubscribing one of two subscriptions keeps keys the other one uses.
#[test]
fn test_unsubscribe_keeps_shared_index_membership() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(1, "a".into(), vec![authors_filter("shared")])
            .await
            .unwrap();
        registry
            .subscribe(1, "b".into(), vec![authors_filter("shared")])
            .await
            .unwrap();

        registry.unsubscribe(1, "a".into()).await.unwrap();

        {
            let stored = data.lock().unwrap();
            assert_eq!(stored["pk:shared"], serde_json::json!([1]));
            assert!(stored.contains_key("conn:1"));
        }

        registry.unsubscribe(1, "b".into()).await.unwrap();

        let keys: Vec<String> = data.lock().unwrap().keys().cloned().collect();
        assert!(
            keys.is_empty(),
            "last unsubscribe left state behind: {keys:?}"
        );
    });
}

#[test]
fn test_registry_lazy_deletion() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut entries = HashMap::new();
        entries.insert("pk:stale".to_string(), serde_json::json!(vec![99]));
        storage.put_batch(entries).await.unwrap();

        let mut registry = WalletRegistry::new(storage, Limits::default());

        let result = registry.load_by_pubkey("stale").await;
        assert!(result.is_empty());

        let data = registry.storage.data.lock().unwrap();
        assert!(!data.contains_key("pk:stale"));
    });
}

#[test]
fn test_subscription_limit_exceeded() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let limits = Limits::new(65536, 0);
        let mut registry = WalletRegistry::new(storage, limits);

        let result = registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("too many subscriptions"));
    });
}

#[test]
fn test_subscribe_rejected_on_storage_failure() {
    futures::executor::block_on(async {
        let storage = MockStorage::with_fail();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let result = registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("persist failed"),
            "expected persist failure, got: {err}"
        );
        assert!(
            err.contains("mock storage unavailable"),
            "expected mock error message, got: {err}"
        );
    });
}

#[test]
fn test_unsubscribe_graceful_on_storage_failure() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(
                1,
                "sub1".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();
        registry
            .subscribe(
                1,
                "sub2".into(),
                vec![Filter {
                    authors: Some(vec!["alice".into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        registry.storage.fail_put_batch = true;

        let result = registry.unsubscribe(1, "sub1".into()).await;
        assert!(
            result.is_ok(),
            "unsubscribe should not propagate storage error"
        );

        assert!(
            !has_subscription(&registry, 1, "sub1"),
            "sub1 should be removed in-memory"
        );
        assert!(
            has_subscription(&registry, 1, "sub2"),
            "sub2 should still exist"
        );
    });
}

#[test]
fn test_delete_info_for_nonexistent_pubkey() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());
        registry.delete_info("nobody").await;
        assert!(registry.get_info("nobody").await.is_none());
    });
}

#[test]
fn test_load_by_pubkey_with_number_value() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut entries = HashMap::new();
        entries.insert("pk:single".to_string(), serde_json::json!(42));
        entries.insert(
            "conn:42".to_string(),
            serde_json::json!({
                "subscriptions": {
                    "sub1": [{"#p": ["single"]}]
                },
                "info_event": null
            }),
        );
        storage.put_batch(entries).await.unwrap();

        let mut registry = WalletRegistry::new(storage, Limits::default());
        let ids = registry.load_by_pubkey("single").await;
        assert!(ids.contains(&42));
    });
}

#[test]
fn test_load_by_pubkey_with_unknown_value_type() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut entries = HashMap::new();
        entries.insert("pk:weird".to_string(), serde_json::json!("string_value"));
        storage.put_batch(entries).await.unwrap();

        let mut registry = WalletRegistry::new(storage, Limits::default());
        let ids = registry.load_by_pubkey("weird").await;
        assert!(ids.is_empty());
    });
}

#[test]
fn test_two_connections_same_pubkey_different_filters() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let shared_pk = "shared_pk";

        registry
            .subscribe(
                1,
                "wallet_sub".into(),
                vec![Filter {
                    kinds: Some(vec![23194]),
                    p_tags: Some(vec![shared_pk.into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        registry
            .subscribe(
                2,
                "app_sub".into(),
                vec![Filter {
                    kinds: Some(vec![23195]),
                    authors: Some(vec![shared_pk.into()]),
                    ..Default::default()
                }],
            )
            .await
            .unwrap();

        let snapshot: HashMap<String, Value> = {
            let data = registry.storage.data.lock().unwrap();
            let pk_val = data
                .get(&format!("pk:{shared_pk}"))
                .expect("pk: entry should exist");
            let ids: Vec<u32> = serde_json::from_value(pk_val.clone()).unwrap();
            assert!(ids.contains(&1), "pk: entry should contain conn 1");
            assert!(ids.contains(&2), "pk: entry should contain conn 2");
            data.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };

        {
            let storage2 = MockStorage::new();
            storage2.put_batch(snapshot.clone()).await.unwrap();
            let mut registry2 = WalletRegistry::new(storage2, Limits::default());

            let req_event = Event {
                id: "req1".into(),
                pubkey: "app_pk".into(),
                created_at: 1000,
                kind: 23194,
                tags: vec![Tag::p(shared_pk)],
                content: "pay invoice".into(),
                sig: "sig".into(),
            };
            let responses = registry2.match_event(&req_event).await;
            assert!(responses.contains(&RegistryResponse::WakeUp(1)));
            assert!(responses.contains(&RegistryResponse::Send {
                recipient_id: 1,
                sub_id: "wallet_sub".into()
            }));
            assert!(!responses.contains(&RegistryResponse::Send {
                recipient_id: 2,
                sub_id: "app_sub".into()
            }));
        }

        {
            let storage3 = MockStorage::new();
            storage3.put_batch(snapshot).await.unwrap();
            let mut registry3 = WalletRegistry::new(storage3, Limits::default());

            let resp_event = Event {
                id: "resp1".into(),
                pubkey: shared_pk.into(),
                created_at: 1000,
                kind: 23195,
                tags: vec![Tag::p("app_pk")],
                content: "paid".into(),
                sig: "sig".into(),
            };
            let responses2 = registry3.match_event(&resp_event).await;
            assert!(responses2.contains(&RegistryResponse::WakeUp(2)));
            assert!(responses2.contains(&RegistryResponse::Send {
                recipient_id: 2,
                sub_id: "app_sub".into()
            }));
            assert!(!responses2.contains(&RegistryResponse::Send {
                recipient_id: 1,
                sub_id: "wallet_sub".into()
            }));
        }
    });
}

fn authors_filter(pk: &str) -> Filter {
    Filter {
        authors: Some(vec![pk.into()]),
        ..Default::default()
    }
}

fn send_targets(responses: &[RegistryResponse]) -> Vec<(u32, String)> {
    responses
        .iter()
        .filter_map(|r| match r {
            RegistryResponse::Send {
                recipient_id,
                sub_id,
            } => Some((*recipient_id, sub_id.clone())),
            RegistryResponse::WakeUp(_) => None,
        })
        .collect()
}

#[test]
fn test_subscribe_same_subscription_twice_is_idempotent() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        let event = crate::common::test_helpers::test_event("e1", "pk", 23194, vec![], 0);
        assert_eq!(
            send_targets(&registry.match_event(&event).await),
            vec![(1, "s1".to_string())]
        );
    });
}

#[test]
fn test_unsubscribe_clears_live_index_and_conn_state() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        assert!(data.lock().unwrap().contains_key("pk:pk"));

        registry.unsubscribe(1, "s1".into()).await.unwrap();

        assert!(
            !registry.index.pk_index.contains_key("pk"),
            "orphan live pk index left behind"
        );
        assert!(
            !data.lock().unwrap().contains_key("conn:1"),
            "empty conn state left behind"
        );
    });
}

#[test]
fn test_delete_info_removes_id_index_entry() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());
        let info = crate::common::test_helpers::test_event("info1", "wallet", 13194, vec![], 0);

        registry.cache_info(info).await;
        assert_eq!(
            registry.find_info_pubkey_by_id("info1").await,
            Some("wallet".to_string())
        );

        registry.delete_info("wallet").await;
        assert_eq!(registry.find_info_pubkey_by_id("info1").await, None);
    });
}

#[test]
fn test_match_event_delivers_only_to_latest_connection() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        registry
            .subscribe(2, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        let event = crate::common::test_helpers::test_event("e1", "pk", 23194, vec![], 0);
        assert_eq!(
            send_targets(&registry.match_event(&event).await),
            vec![(2, "s1".to_string())],
            "the most recent connection for a subscription wins"
        );
    });
}

#[test]
fn test_load_by_pubkey_falls_back_to_live_connection() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry
            .subscribe(3, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        registry
            .subscribe(5, "s2".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        // Drop the persisted index so the in-memory fallback is used.
        data.lock().unwrap().remove("pk:pk");

        assert_eq!(
            registry.load_by_pubkey("pk").await,
            vec![5],
            "highest connection id must win"
        );
        assert!(registry.load_by_pubkey("other").await.is_empty());
    });
}

#[test]
fn test_saved_connection_state_has_no_info_event_copy() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        let mut registry = WalletRegistry::new(storage, Limits::default());
        let info = crate::common::test_helpers::test_event("info1", "pk", 13194, vec![], 0);

        registry.cache_info(info).await;
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        let data = data.lock().unwrap();
        let saved = &data["conn:1"];
        assert!(
            saved.get("info_event").is_none(),
            "conn state must not duplicate the authoritative info: key"
        );
    });
}

// Regression: a hibernated connection snapshot used to carry a copy of the
// info event. After the wallet republished, restoring that connection
// re-cached the stale copy and every replay served the old event.
#[test]
fn test_hibernation_does_not_resurrect_stale_info_event() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("wallet")])
            .await
            .unwrap();

        let mut restarted = simulate_hibernation(&registry.storage).await;
        restarted
            .cache_info(info_with_p_tags("info2", "wallet", &["app"]))
            .await;

        // Restoring the old connection must not clobber the newer event.
        assert!(restarted.load(1).await);
        assert_eq!(
            restarted.index.get_info("wallet").map(|e| e.id),
            Some("info2".into()),
            "stale connection snapshot overwrote the republished info event"
        );
        assert_eq!(
            restarted.get_info("wallet").await.map(|e| e.id),
            Some("info2".into())
        );
    });
}

#[test]
fn test_load_by_pubkey_prunes_stale_ids() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        // conn 7 exists, conn 99 was terminated without cleaning the index.
        seed_subscription(&storage, 7, "s1", "pk", vec![authors_filter("pk")]).await;
        data.lock()
            .unwrap()
            .insert("pk:pk".into(), serde_json::json!([7, 99]));

        let mut registry = WalletRegistry::new(storage, Limits::default());
        assert_eq!(registry.load_by_pubkey("pk").await, vec![7]);
        assert_eq!(data.lock().unwrap()["pk:pk"], serde_json::json!([7]));
    });
}

#[test]
fn test_on_terminate_handles_numeric_pk_index() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        seed_subscription(&storage, 42, "s1", "pk", vec![authors_filter("pk")]).await;
        data.lock()
            .unwrap()
            .insert("pk:pk".into(), serde_json::json!(42));

        let mut registry = WalletRegistry::new(storage, Limits::default());
        assert!(registry.load(42).await);
        registry.on_terminate(42).await;

        assert!(!data.lock().unwrap().contains_key("pk:pk"));
    });
}

#[test]
fn test_on_terminate_rewrites_shared_pk_index() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        seed_subscription(&storage, 1, "s1", "pk", vec![authors_filter("pk")]).await;
        seed_subscription(&storage, 2, "s2", "pk", vec![authors_filter("pk")]).await;
        data.lock()
            .unwrap()
            .insert("pk:pk".into(), serde_json::json!([1, 2]));

        let mut registry = WalletRegistry::new(storage, Limits::default());
        assert!(registry.load(2).await);
        registry.on_terminate(2).await;

        assert_eq!(data.lock().unwrap()["pk:pk"], serde_json::json!([1]));
    });
}

#[test]
fn test_on_terminate_ignores_missing_pk_index() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        seed_subscription(&storage, 3, "s1", "ghost", vec![authors_filter("ghost")]).await;
        // The pk index entry is gone even though the connection state remains.
        data.lock().unwrap().remove("pk:ghost");

        let mut registry = WalletRegistry::new(storage, Limits::default());
        assert!(registry.load(3).await);
        registry.on_terminate(3).await;

        assert!(!data.lock().unwrap().contains_key("conn:3"));
    });
}

#[test]
fn test_get_info_ignores_corrupt_stored_event() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        storage
            .put_batch(HashMap::from([(
                "info:pk".to_string(),
                serde_json::json!("not an event"),
            )]))
            .await
            .unwrap();

        let mut registry = WalletRegistry::new(storage, Limits::default());
        assert!(registry.get_info("pk").await.is_none());
    });
}

#[test]
fn test_sync_merges_numeric_existing_pk_index() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        data.lock()
            .unwrap()
            .insert("pk:pk".into(), serde_json::json!(42));

        let mut registry = WalletRegistry::new(storage, Limits::default());
        registry
            .subscribe(9, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        assert_eq!(data.lock().unwrap()["pk:pk"], serde_json::json!([42, 9]));
    });
}

#[test]
fn test_sync_ignores_unparseable_pk_index() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        data.lock()
            .unwrap()
            .insert("pk:pk".into(), serde_json::json!("garbage"));

        let mut registry = WalletRegistry::new(storage, Limits::default());
        registry
            .subscribe(9, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        assert_eq!(data.lock().unwrap()["pk:pk"], serde_json::json!([9]));
    });
}

#[test]
fn test_unsubscribe_keeps_pk_index_while_info_is_cached() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        let info = crate::common::test_helpers::test_event("info1", "pk", 13194, vec![], 0);
        registry.cache_info(info).await;
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        registry.unsubscribe(1, "s1".into()).await.unwrap();

        // The last subscription is gone, but the pk index entry still owns the
        // cached info event and must survive.
        assert!(
            registry.index.get_info("pk").is_some(),
            "cached info was dropped with the last subscription"
        );
    });
}

#[test]
fn test_subscribe_enforces_max_subscriptions() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::new(65536, 2));
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        registry
            .subscribe(1, "s2".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        let err = registry
            .subscribe(1, "s3".into(), vec![authors_filter("pk")])
            .await
            .expect_err("third subscription exceeds the limit of 2");
        assert!(err.to_string().contains("too many subscriptions"), "{err}");
    });
}

#[test]
fn test_delete_info_keeps_pk_index_while_subscriptions_remain() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();
        let info = crate::common::test_helpers::test_event("info1", "pk", 13194, vec![], 0);
        registry.cache_info(info).await;

        registry.delete_info("pk").await;

        // Info is gone, but the entry still carries a live subscription.
        assert!(registry.index.get_info("pk").is_none());
        assert!(
            registry.index.pk_index.contains_key("pk"),
            "subscription lost its pk index entry with the deleted info"
        );
    });
}

#[test]
fn test_get_connection_id_picks_highest_connection() {
    futures::executor::block_on(async {
        // Two distinct subscriptions give the entry multiple candidates, so a
        // max-reduction that just takes the last iterated id is exposed.
        for _ in 0..32 {
            let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
            registry
                .subscribe(1, "s1".into(), vec![authors_filter("pk")])
                .await
                .unwrap();
            registry
                .subscribe(2, "s1".into(), vec![authors_filter("pk")])
                .await
                .unwrap();
            registry
                .subscribe(3, "s2".into(), vec![authors_filter("pk")])
                .await
                .unwrap();

            assert_eq!(registry.index.get_connection_id("pk"), Some(3));
        }
    });
}

#[test]
fn test_on_terminate_keeps_numeric_pk_index_for_other_connections() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let data = storage.data.clone();
        seed_subscription(&storage, 42, "s1", "pk", vec![authors_filter("pk")]).await;
        // Legacy single-value index pointing at a different connection.
        data.lock()
            .unwrap()
            .insert("pk:pk".into(), serde_json::json!(99));

        let mut registry = WalletRegistry::new(storage, Limits::default());
        assert!(registry.load(42).await);
        registry.on_terminate(42).await;

        assert_eq!(
            data.lock().unwrap().get("pk:pk"),
            Some(&serde_json::json!([99])),
            "terminating one connection must keep the others indexed"
        );
    });
}

// ── NWA `#p` info replay ──

fn info_with_p_tags(id: &str, author: &str, p_tags: &[&str]) -> Event {
    crate::common::test_helpers::test_event(
        id,
        author,
        13194,
        p_tags.iter().map(|pk| Tag::p(*pk)).collect(),
        1000,
    )
}

#[test]
fn test_get_info_resolves_p_tag_alias() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        // The author keeps working as before.
        assert_eq!(
            registry.index.get_info("wallet").map(|e| e.id),
            Some("info1".into())
        );
        // A NWA app finds the event through its `p` tag.
        assert_eq!(
            registry.get_info("app").await.map(|e| e.id),
            Some("info1".into())
        );
        assert!(registry.get_info("unrelated").await.is_none());
    });
}

#[test]
fn test_p_tag_alias_survives_hibernation() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        let mut restarted = simulate_hibernation(&registry.storage).await;
        // Fresh index, only persisted keys available.
        assert!(restarted.index.get_info("app").is_none());

        let info = restarted.get_info("app").await.expect("alias must resolve");
        assert_eq!(info.id, "info1");
        assert_eq!(info.pubkey, "wallet");
        // Resolving through the alias re-caches the event in memory.
        assert_eq!(
            restarted.index.get_info("app").map(|e| e.id),
            Some("info1".into())
        );
    });
}

#[test]
fn test_republish_drops_stale_p_tag_alias() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app1"]))
            .await;
        registry
            .cache_info(info_with_p_tags("info2", "wallet", &["app2"]))
            .await;

        assert!(
            registry.get_info("app1").await.is_none(),
            "the old app must not resolve to the new info event"
        );
        assert_eq!(
            registry.get_info("app2").await.map(|e| e.id),
            Some("info2".into())
        );
        assert!(
            registry
                .storage
                .data
                .lock()
                .unwrap()
                .get("info-p:app1")
                .is_none(),
            "stale alias must be deleted from storage"
        );
    });
}

#[test]
fn test_delete_info_removes_p_tag_alias() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        registry.delete_info("wallet").await;

        assert!(registry.get_info("app").await.is_none());
        assert!(registry.get_info("wallet").await.is_none());
        let data = registry.storage.data.lock().unwrap();
        assert!(data.get("info:wallet").is_none());
        assert!(data.get("info-p:app").is_none());
    });
}

// ── `#e` / `ids`-only routing ──

fn e_tag_filter(kind: u64, event_id: &str) -> Filter {
    Filter {
        kinds: Some(vec![kind]),
        e_tags: Some(vec![event_id.into()]),
        ..Default::default()
    }
}

fn ids_filter(event_id: &str) -> Filter {
    Filter {
        ids: Some(vec![event_id.into()]),
        ..Default::default()
    }
}

fn response_event(id: &str, e_tag: &str) -> Event {
    crate::common::test_helpers::test_event(
        id,
        "wallet",
        23195,
        vec![Tag::p("app"), Tag::E(e_tag.into(), vec![])],
        1000,
    )
}

#[test]
fn test_e_tag_only_filter_routes_live() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-e".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();

        let responses = registry.match_event(&response_event("resp1", "req1")).await;
        assert_eq!(send_targets(&responses), vec![(1, "sub-e".into())]);
        assert!(
            registry
                .storage
                .data
                .lock()
                .unwrap()
                .contains_key("etag:req1"),
            "the #e index must be persisted for hibernation"
        );
    });
}

#[test]
fn test_e_tag_only_filter_requires_matching_kind() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-e".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();

        // Same `e` tag, different kind: the full filter must still match.
        let deletion = crate::common::test_helpers::test_event(
            "del1",
            "wallet",
            5,
            vec![Tag::E("req1".into(), vec![])],
            1000,
        );
        assert!(send_targets(&registry.match_event(&deletion).await).is_empty());
    });
}

#[test]
fn test_e_tag_only_filter_survives_hibernation() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-e".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();

        let mut restarted = simulate_hibernation(&registry.storage).await;
        let responses = restarted
            .match_event(&response_event("resp1", "req1"))
            .await;

        assert!(responses.contains(&RegistryResponse::WakeUp(1)));
        assert!(responses.contains(&RegistryResponse::Send {
            recipient_id: 1,
            sub_id: "sub-e".into()
        }));
    });
}

#[test]
fn test_ids_only_filter_routes_live() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-id".into(), vec![ids_filter("resp1")])
            .await
            .unwrap();

        let responses = registry.match_event(&response_event("resp1", "req1")).await;
        assert_eq!(send_targets(&responses), vec![(1, "sub-id".into())]);
        assert!(
            registry
                .storage
                .data
                .lock()
                .unwrap()
                .contains_key("eid:resp1"),
            "the ids index must be persisted for hibernation"
        );
    });
}

#[test]
fn test_ids_only_filter_survives_hibernation() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-id".into(), vec![ids_filter("resp1")])
            .await
            .unwrap();

        let mut restarted = simulate_hibernation(&registry.storage).await;
        let responses = restarted
            .match_event(&response_event("resp1", "req1"))
            .await;

        assert!(responses.contains(&RegistryResponse::WakeUp(1)));
        assert!(responses.contains(&RegistryResponse::Send {
            recipient_id: 1,
            sub_id: "sub-id".into()
        }));
    });
}

#[test]
fn test_unsubscribe_stops_secondary_index_routing() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-e".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();
        registry
            .subscribe(1, "sub-id".into(), vec![ids_filter("resp1")])
            .await
            .unwrap();

        registry.unsubscribe(1, "sub-e".into()).await.unwrap();
        registry.unsubscribe(1, "sub-id".into()).await.unwrap();

        let responses = registry.match_event(&response_event("resp1", "req1")).await;
        assert!(
            responses.is_empty(),
            "unsubscribed indexes must not deliver"
        );
    });
}

#[test]
fn test_unsubscribe_keeps_other_subscription_in_same_index() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-a".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();
        registry
            .subscribe(1, "sub-b".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();

        registry.unsubscribe(1, "sub-a".into()).await.unwrap();

        let responses = registry.match_event(&response_event("resp1", "req1")).await;
        assert_eq!(send_targets(&responses), vec![(1, "sub-b".into())]);
    });
}

#[test]
fn test_delete_info_keeps_alias_owned_by_other_author() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet-a", &["shared", "only-a"]))
            .await;
        registry
            .cache_info(info_with_p_tags("info2", "wallet-b", &["shared", "only-b"]))
            .await;

        registry.delete_info("wallet-a").await;

        // In memory: the author's own alias is dropped, the shared one is
        // still owned by the surviving author.
        assert!(!registry.index.info_p_index.contains_key("only-a"));
        assert_eq!(
            registry
                .index
                .info_p_index
                .get("shared")
                .map(String::as_str),
            Some("wallet-b")
        );

        let info = registry
            .get_info("shared")
            .await
            .expect("the surviving author's alias must still resolve");
        assert_eq!(info.id, "info2");
        assert_eq!(info.pubkey, "wallet-b");

        let data = registry.storage.data.lock().unwrap();
        assert_eq!(
            data.get("info-p:shared").and_then(Value::as_str),
            Some("wallet-b"),
            "deleting one author must not delete another author's alias"
        );
        assert!(data.get("info-p:only-a").is_none());
        assert!(data.get("info:wallet-a").is_none());
        assert!(data.get("info:wallet-b").is_some());
    });
}

#[test]
fn test_delete_info_after_hibernation_removes_event_and_alias() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        // Fresh process: the in-memory index is empty, only storage remains.
        let mut restarted = simulate_hibernation(&registry.storage).await;
        restarted.delete_info("wallet").await;

        assert!(restarted.get_info("wallet").await.is_none());
        assert!(restarted.get_info("app").await.is_none());
        let data = restarted.storage.data.lock().unwrap();
        assert!(data.get("info:wallet").is_none());
        assert!(data.get("info-p:app").is_none());
    });
}

#[test]
fn test_delete_info_after_hibernation_keeps_other_authors_alias() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet-a", &["app"]))
            .await;
        registry
            .cache_info(info_with_p_tags("info2", "wallet-b", &["app"]))
            .await;

        let mut restarted = simulate_hibernation(&registry.storage).await;
        restarted.delete_info("wallet-a").await;

        let info = restarted
            .get_info("app")
            .await
            .expect("the surviving author's alias must still resolve");
        assert_eq!(info.id, "info2");
        let data = restarted.storage.data.lock().unwrap();
        assert_eq!(
            data.get("info-p:app").and_then(Value::as_str),
            Some("wallet-b")
        );
        assert!(data.get("info:wallet-a").is_none());
    });
}

#[test]
fn test_unsubscribe_prunes_secondary_index_keys() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .subscribe(1, "sub-e".into(), vec![e_tag_filter(23195, "req1")])
            .await
            .unwrap();
        registry
            .subscribe(1, "sub-id".into(), vec![ids_filter("resp1")])
            .await
            .unwrap();

        assert!(registry.index.e_index.contains_key("req1"));
        assert!(registry.index.id_index.contains_key("resp1"));

        registry.unsubscribe(1, "sub-e".into()).await.unwrap();
        assert!(
            registry.index.e_index.is_empty(),
            "an index key with no subscribers must be pruned"
        );
        assert!(registry.index.id_index.contains_key("resp1"));

        registry.unsubscribe(1, "sub-id".into()).await.unwrap();
        assert!(
            registry.index.id_index.is_empty(),
            "an index key with no subscribers must be pruned"
        );
    });
}

#[test]
fn test_index_delete_info_returns_removed_event() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        let removed = registry.index.delete_info("wallet");
        assert_eq!(removed.map(|e| e.id), Some("info1".into()));
        // A second delete finds nothing left to remove.
        assert!(registry.index.delete_info("wallet").is_none());
    });
}

// ── subscription limit vs. NIP-01 replace semantics ──

#[test]
fn test_subscribe_replaces_existing_subscription_at_limit() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::new(65536, 1));
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("alice")])
            .await
            .unwrap();

        // At the limit, reusing the same sub_id replaces the subscription
        // instead of being rejected (NIP-01).
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("bob")])
            .await
            .expect("re-REQ of an existing sub_id must replace it at the limit");

        assert_eq!(registry.index.sub_count(1), 1, "replace must not add a sub");
        assert!(has_subscription(&registry, 1, "s1"));

        let alice = crate::common::test_helpers::test_event("e1", "alice", 23194, vec![], 0);
        assert!(
            send_targets(&registry.match_event(&alice).await).is_empty(),
            "the replaced filter must no longer route"
        );
        let bob = crate::common::test_helpers::test_event("e2", "bob", 23194, vec![], 0);
        assert_eq!(
            send_targets(&registry.match_event(&bob).await),
            vec![(1, "s1".to_string())],
            "the new filter must route"
        );
    });
}

#[test]
fn test_subscribe_new_subscription_at_limit_is_rejected() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::new(65536, 1));
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("alice")])
            .await
            .unwrap();

        let err = registry
            .subscribe(1, "s2".into(), vec![authors_filter("bob")])
            .await
            .expect_err("a new sub_id must still respect the limit");
        assert!(err.to_string().contains("too many subscriptions"), "{err}");
    });
}

// ── persisted `infoid:` reverse index ──

#[test]
fn test_cache_info_persists_infoid_index() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        assert_eq!(
            registry.index.find_info_pubkey_by_id("info1").as_deref(),
            Some("wallet")
        );
        let data = registry.storage.data.lock().unwrap();
        assert_eq!(
            data.get("infoid:info1").and_then(Value::as_str),
            Some("wallet"),
            "the id reverse index must be persisted for post-hibernation deletion"
        );
    });
}

#[test]
fn test_find_info_pubkey_by_id_falls_back_to_storage() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        let restarted = simulate_hibernation(&registry.storage).await;
        assert!(restarted.index.info_id_index.is_empty());
        assert_eq!(
            restarted.find_info_pubkey_by_id("info1").await.as_deref(),
            Some("wallet"),
            "the lookup must fall back to the persisted infoid key"
        );
        assert!(restarted.find_info_pubkey_by_id("missing").await.is_none());
    });
}

#[test]
fn test_republish_moves_infoid_index() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;
        registry
            .cache_info(info_with_p_tags("info2", "wallet", &["app"]))
            .await;

        assert!(
            registry.index.find_info_pubkey_by_id("info1").is_none(),
            "the superseded event id must leave the in-memory index"
        );
        assert_eq!(
            registry.find_info_pubkey_by_id("info1").await,
            None,
            "the superseded event id must leave storage too"
        );
        assert_eq!(
            registry.find_info_pubkey_by_id("info2").await.as_deref(),
            Some("wallet")
        );
        let data = registry.storage.data.lock().unwrap();
        assert!(data.get("infoid:info1").is_none());
        assert_eq!(
            data.get("infoid:info2").and_then(Value::as_str),
            Some("wallet")
        );
    });
}

#[test]
fn test_republish_after_hibernation_cleans_infoid_and_stale_alias() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app1", "shared"]))
            .await;

        // Fresh index: the republish can only learn the old event from storage.
        let mut restarted = simulate_hibernation(&registry.storage).await;
        restarted
            .cache_info(info_with_p_tags("info2", "wallet", &["app2", "shared"]))
            .await;

        assert_eq!(
            restarted.find_info_pubkey_by_id("info1").await,
            None,
            "the stale reverse index must be deleted from storage"
        );
        assert_eq!(
            restarted.find_info_pubkey_by_id("info2").await.as_deref(),
            Some("wallet")
        );
        assert!(
            restarted.get_info("app1").await.is_none(),
            "a p tag the new event no longer carries must not resolve"
        );
        assert_eq!(
            restarted.get_info("app2").await.map(|e| e.id),
            Some("info2".into())
        );
        let data = restarted.storage.data.lock().unwrap();
        assert!(data.get("info-p:app1").is_none());
        assert!(data.get("infoid:info1").is_none());
    });
}

#[test]
fn test_delete_info_removes_infoid_index() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        registry.delete_info("wallet").await;

        assert!(registry.index.find_info_pubkey_by_id("info1").is_none());
        assert!(registry.find_info_pubkey_by_id("info1").await.is_none());
        let data = registry.storage.data.lock().unwrap();
        assert!(data.get("infoid:info1").is_none());
    });
}

#[test]
fn test_delete_info_by_event_after_hibernation() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        let mut restarted = simulate_hibernation(&registry.storage).await;
        assert!(restarted.delete_info_by_event("info1", "wallet").await);

        assert!(restarted.get_info("wallet").await.is_none());
        assert!(restarted.get_info("app").await.is_none());
        assert!(restarted.find_info_pubkey_by_id("info1").await.is_none());
        let data = restarted.storage.data.lock().unwrap();
        assert!(data.get("info:wallet").is_none());
        assert!(data.get("info-p:app").is_none());
        assert!(data.get("infoid:info1").is_none());
    });
}

#[test]
fn test_delete_info_by_event_rejects_other_author() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        assert!(!registry.delete_info_by_event("info1", "attacker").await);
        assert_eq!(
            registry.get_info("wallet").await.map(|e| e.id),
            Some("info1".into())
        );
    });
}

#[test]
fn test_delete_info_by_event_ignores_superseded_id() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;
        registry
            .cache_info(info_with_p_tags("info2", "wallet", &["app"]))
            .await;

        assert!(
            !registry.delete_info_by_event("info1", "wallet").await,
            "an old event id must not delete the republished event"
        );
        assert_eq!(
            registry.get_info("wallet").await.map(|e| e.id),
            Some("info2".into())
        );
    });
}

#[test]
fn test_delete_info_by_event_after_hibernation_ignores_superseded_id() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info1", "wallet", &["app"]))
            .await;

        let mut restarted = simulate_hibernation(&registry.storage).await;
        restarted
            .cache_info(info_with_p_tags("info2", "wallet", &["app"]))
            .await;

        assert!(!restarted.delete_info_by_event("info1", "wallet").await);
        assert_eq!(
            restarted.get_info("wallet").await.map(|e| e.id),
            Some("info2".into())
        );
    });
}

#[test]
fn test_delete_info_by_event_unknown_id() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        assert!(!registry.delete_info_by_event("missing", "wallet").await);
    });
}

#[test]
fn test_delete_info_by_event_ignores_stale_reverse_index() {
    futures::executor::block_on(async {
        let mut registry = WalletRegistry::new(MockStorage::new(), Limits::default());
        registry
            .cache_info(info_with_p_tags("info2", "wallet", &["app"]))
            .await;

        // Simulate a leftover reverse mapping for a superseded event id (e.g.
        // legacy state written before republishing cleaned it up). The mapping
        // resolves, but the current event is `info2`, so the delete must be
        // refused instead of removing the newer event.
        let mut leftover = std::collections::HashMap::new();
        leftover.insert("infoid:info1".to_string(), serde_json::json!("wallet"));
        registry.storage.put_batch(leftover).await.unwrap();

        assert!(!registry.delete_info_by_event("info1", "wallet").await);
        assert_eq!(
            registry.get_info("wallet").await.map(|e| e.id),
            Some("info2".into())
        );
    });
}
