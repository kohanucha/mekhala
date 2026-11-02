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

        let pk = registry.find_info_pubkey_by_id("id1");
        assert_eq!(pk, Some("alice".into()));

        let pk_missing = registry.find_info_pubkey_by_id("nonexistent");
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
        assert_eq!(registry.find_info_pubkey_by_id("id1"), Some("alice".into()));

        registry.delete_info("alice").await;

        assert!(registry.get_info("alice").await.is_none());
        assert_eq!(registry.find_info_pubkey_by_id("id1"), None);

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
            registry.find_info_pubkey_by_id("info1"),
            Some("wallet".to_string())
        );

        registry.delete_info("wallet").await;
        assert_eq!(registry.find_info_pubkey_by_id("info1"), None);
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
fn test_save_persists_cached_info_event() {
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
        assert_eq!(saved["info_event"]["id"], "info1");
    });
}

#[test]
fn test_restore_recaches_persisted_info_event() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let shared = storage.data.clone();
        let info = crate::common::test_helpers::test_event("info1", "pk", 13194, vec![], 0);
        let mut registry = WalletRegistry::new(storage, Limits::default());
        registry.cache_info(info).await;
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        let cold = MockStorage {
            data: shared,
            fail_put_batch: false,
        };
        let mut hibernated = simulate_hibernation(&cold).await;
        assert!(hibernated.load(1).await);
        assert!(
            hibernated.index.get_info("pk").is_some(),
            "restore must re-cache the info event in the live index"
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
fn test_on_disconnect_keeps_persisted_state() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());
        registry
            .subscribe(1, "s1".into(), vec![authors_filter("pk")])
            .await
            .unwrap();

        registry.on_disconnect(1).await;

        assert!(!has_subscription(&registry, 1, "s1"));
        assert!(registry.load(1).await, "storage must still hold the state");
        assert!(has_subscription(&registry, 1, "s1"));
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
