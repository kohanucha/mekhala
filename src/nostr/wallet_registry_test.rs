use super::*;
use crate::common::test_helpers::*;
use crate::nostr::Tag;
use std::sync::{Arc, Mutex};



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
        assert!(data.contains_key("conn:1"), "Storage should contain connection state");
        assert!(data.contains_key("pk:alice"), "Storage should contain pubkey index");
    });
}

#[test]
fn test_hibernation_contract() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

        let mut registry2 = simulate_hibernation(&registry.storage).await;

        let event = Event {
            id: "e1".into(),
            pubkey: "alice".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: "".into(),
            sig: "sig".into(),
        };
        let responses = registry2.match_event(&event).await;
        assert!(responses.contains(&RegistryResponse::WakeUp(1)),
            "hibernated connection must produce WakeUp");
        assert!(responses.contains(&RegistryResponse::Send {
            recipient_id: 1, sub_id: "sub1".into()
        }), "hibernated connection must produce Send");
    });
}

#[test]
fn test_hibernation_info_survival() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

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
            content: "".into(),
            sig: "sig".into(),
        };
        let responses = registry2.match_event(&event).await;
        assert!(responses.contains(&RegistryResponse::WakeUp(1)),
            "hibernated connection must still produce WakeUp with info");
        assert!(responses.contains(&RegistryResponse::Send {
            recipient_id: 1, sub_id: "sub1".into()
        }), "hibernated connection must still produce Send with info");
    });
}

#[test]
fn test_hibernation_subscribe_cycles() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

        registry.unsubscribe(1, "sub1".into()).await.unwrap();

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["bob".into()]),
            ..Default::default()
        }]).await.unwrap();

        let mut registry2 = simulate_hibernation(&registry.storage).await;

        let alice_event = Event {
            id: "e1".into(),
            pubkey: "alice".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: "".into(),
            sig: "sig".into(),
        };
        let responses = registry2.match_event(&alice_event).await;
        assert!(!responses.contains(&RegistryResponse::Send { recipient_id: 1, sub_id: "sub1".into() }),
            "alice event should NOT match after re-subscribe to bob");

        let bob_event = Event {
            id: "e2".into(),
            pubkey: "bob".into(),
            created_at: 1000,
            kind: 23194,
            tags: vec![],
            content: "".into(),
            sig: "sig".into(),
        };
        let responses2 = registry2.match_event(&bob_event).await;
        assert!(responses2.contains(&RegistryResponse::WakeUp(1)),
            "bob event must produce WakeUp after re-subscribe");
        assert!(responses2.contains(&RegistryResponse::Send {
            recipient_id: 1, sub_id: "sub1".into()
        }), "bob event must produce Send after re-subscribe");
    });
}

#[test]
fn test_registry_match_routing() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());
        
        let wallet_pk = "wallet_pk";
        registry.subscribe(1, "sub1".into(), vec![Filter {
            p_tags: Some(vec![wallet_pk.into()]),
            ..Default::default()
        }]).await.unwrap();
        
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
        assert!(matches.contains(&RegistryResponse::Send { recipient_id: 1, sub_id: "sub1".into() }));
    });
}

#[test]
fn test_registry_lazy_load() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let wallet_pk = "hibernated_pk";
        let conn_id = 42;
        
        let mut entries = HashMap::new();
        entries.insert(format!("pk:{}", wallet_pk), serde_json::json!(vec![conn_id]));
        entries.insert(format!("conn:{}", conn_id), serde_json::json!({
            "subscriptions": {
                "sub1": [{"#p": [wallet_pk]}]
            },
            "info_event": null
        }));
        storage.put_batch(entries).await.unwrap();
        
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
        assert!(responses.contains(&RegistryResponse::Send { recipient_id: conn_id, sub_id: "sub1".into() }));
    });
}

#[test]
fn test_index_matching_grouped() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();
        registry.subscribe(2, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

        let event_alice = Event {
            id: "id".into(),
            pubkey: "alice".into(),
            kind: 1,
            tags: vec![],
            content: "".into(),
            sig: "".into(),
            created_at: 1000,
        };
        let responses = registry.match_event(&event_alice).await;

        let grouped: Vec<_> = responses.iter()
            .filter_map(|r| match r {
                RegistryResponse::Send { recipient_id, sub_id } if sub_id == "sub1" => Some(*recipient_id),
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
            content: "".into(),
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
            content: "".into(),
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
            content: "".into(),
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

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

        let event = Event {
            id: "id1".into(),
            pubkey: "alice".into(),
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig1".into(),
            created_at: 1000,
        };
        registry.cache_info(event.clone()).await;

        registry.delete_info("alice").await;

        assert!(registry.get_info("alice").await.is_none());
        assert!(registry.has_subscription(1, "sub1"));
    });
}

#[test]
fn test_registry_sync() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

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

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

        registry.on_terminate(1).await;

        let data = registry.storage.data.lock().unwrap();
        assert!(!data.contains_key("conn:1"));
        assert!(!data.contains_key("pk:alice"));
        assert!(!registry.has_subscription(1, "sub1"));
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

        let result = registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too many subscriptions"));
    });
}

#[test]
fn test_subscribe_rejected_on_storage_failure() {
    futures::executor::block_on(async {
        let storage = MockStorage {
            data: Arc::new(Mutex::new(HashMap::new())),
            fail_put_batch: true,
        };
        let mut registry = WalletRegistry::new(storage, Limits::default());

        let result = registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("persist failed"), "expected persist failure, got: {}", err);
        assert!(err.contains("mock storage unavailable"), "expected mock error message, got: {}", err);
    });
}

#[test]
fn test_unsubscribe_graceful_on_storage_failure() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut registry = WalletRegistry::new(storage, Limits::default());

        registry.subscribe(1, "sub1".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();
        registry.subscribe(1, "sub2".into(), vec![Filter {
            authors: Some(vec!["alice".into()]),
            ..Default::default()
        }]).await.unwrap();

        registry.storage.fail_put_batch = true;

        let result = registry.unsubscribe(1, "sub1".into()).await;
        assert!(result.is_ok(), "unsubscribe should not propagate storage error");

        assert!(!registry.has_subscription(1, "sub1"), "sub1 should be removed in-memory");
        assert!(registry.has_subscription(1, "sub2"), "sub2 should still exist");
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
fn test_read_pk_list_with_number_value() {
    futures::executor::block_on(async {
        let storage = MockStorage::new();
        let mut entries = HashMap::new();
        entries.insert("pk:single".to_string(), serde_json::json!(42));
        entries.insert("conn:42".to_string(), serde_json::json!({
            "subscriptions": {
                "sub1": [{"#p": ["single"]}]
            },
            "info_event": null
        }));
        storage.put_batch(entries).await.unwrap();

        let mut registry = WalletRegistry::new(storage, Limits::default());
        let ids = registry.load_by_pubkey("single").await;
        assert!(ids.contains(&42));
    });
}

#[test]
fn test_read_pk_list_with_unknown_value_type() {
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

        registry.subscribe(1, "wallet_sub".into(), vec![Filter {
            kinds: Some(vec![23194]),
            p_tags: Some(vec![shared_pk.into()]),
            ..Default::default()
        }]).await.unwrap();

        registry.subscribe(2, "app_sub".into(), vec![Filter {
            kinds: Some(vec![23195]),
            authors: Some(vec![shared_pk.into()]),
            ..Default::default()
        }]).await.unwrap();

        #[allow(clippy::await_holding_lock)]
        let snapshot: HashMap<String, Value> = {
            let data = registry.storage.data.lock().unwrap();
            let pk_val = data.get(&format!("pk:{}", shared_pk)).expect("pk: entry should exist");
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
            assert!(responses.contains(&RegistryResponse::Send { recipient_id: 1, sub_id: "wallet_sub".into() }));
            assert!(!responses.contains(&RegistryResponse::Send { recipient_id: 2, sub_id: "app_sub".into() }));
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
            assert!(responses2.contains(&RegistryResponse::Send { recipient_id: 2, sub_id: "app_sub".into() }));
            assert!(!responses2.contains(&RegistryResponse::Send { recipient_id: 1, sub_id: "wallet_sub".into() }));
        }
    });
}
