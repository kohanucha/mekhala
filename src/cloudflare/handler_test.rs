use super::*;
    use std::collections::HashMap;

    struct MockUserStore {
        uris: HashMap<String, String>,
    }

    #[async_trait::async_trait(?Send)]
    impl UserStore for MockUserStore {
        async fn get_nwc_uri(&self, username: &str) -> Option<String> {
            self.uris.get(username).cloned()
        }
    }

    #[test]
    fn test_lookup_user_found() {
        let mut uris = HashMap::new();
        uris.insert("alice".to_string(), "nostr+walletconnect://pk?secret=s&relay=wss%3A%2F%2Frelay.com".to_string());
        let store = MockUserStore { uris };
        let handler = LnAddressHandler::new(&store);

        futures::executor::block_on(async {
            let result = handler.lookup_user("alice").await;
            assert!(result.is_some());
            assert!(result.unwrap().contains("nostr+walletconnect"));
        });
    }

    #[test]
    fn test_lookup_user_not_found() {
        let store = MockUserStore { uris: HashMap::new() };
        let handler = LnAddressHandler::new(&store);

        futures::executor::block_on(async {
            let result = handler.lookup_user("nobody").await;
            assert!(result.is_none());
        });
    }

    #[test]
    fn test_lookup_user_empty_uri() {
        let mut uris = HashMap::new();
        uris.insert("bob".to_string(), String::new());
        let store = MockUserStore { uris };
        let handler = LnAddressHandler::new(&store);

        futures::executor::block_on(async {
            let result = handler.lookup_user("bob").await;
            assert!(result.is_some());
        });
    }
