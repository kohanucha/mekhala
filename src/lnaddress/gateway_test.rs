use super::*;

    #[test]
    fn test_build_callback_url_local() {
        let url = Url::parse("http://localhost:8787/.well-known/lnurlp/alice").unwrap();
        let callback = build_callback_url("alice", &url);
        assert_eq!(callback, "http://localhost:8787/lnaddress/alice/callback");
    }

    #[test]
    fn test_build_callback_url_remote() {
        let url = Url::parse("https://relay.com/.well-known/lnurlp/bob").unwrap();
        let callback = build_callback_url("bob", &url);
        assert_eq!(callback, "https://relay.com/lnaddress/bob/callback");
    }

    #[test]
    fn test_build_callback_url_no_port() {
        let url = Url::parse("https://relay.com/.well-known/lnurlp/charlie").unwrap();
        let callback = build_callback_url("charlie", &url);
        assert_eq!(callback, "https://relay.com/lnaddress/charlie/callback");
    }

    #[test]
    fn test_generate_metadata_format() {
        let metadata = generate_metadata("alice");
        assert_eq!(metadata, "[[\"text/plain\",\"Payment to alice\"]]");
    }

    #[test]
    fn test_description_hash_length() {
        let hash = get_description_hash("testuser");
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_description_hash_deterministic() {
        let hash1 = get_description_hash("testuser");
        let hash2 = get_description_hash("testuser");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_pay_request_info_structure() {
        let url = Url::parse("https://relay.com/.well-known/lnurlp/alice").unwrap();
        let info = pay_request_info("alice", &url);
        assert_eq!(info["tag"], "payRequest");
        assert_eq!(info["callback"], "https://relay.com/lnaddress/alice/callback");
        assert_eq!(info["maxSendable"], 100000000);
        assert_eq!(info["minSendable"], 1000);
        assert_eq!(info["metadata"], "[[\"text/plain\",\"Payment to alice\"]]");
    }

    #[test]
    fn test_pay_request_info_different_username() {
        let url = Url::parse("https://other.com/.well-known/lnurlp/bob").unwrap();
        let info = pay_request_info("bob", &url);
        assert_eq!(info["callback"], "https://other.com/lnaddress/bob/callback");
        assert_eq!(info["metadata"], "[[\"text/plain\",\"Payment to bob\"]]");
    }

    #[test]
    fn test_create_invoice_invalid_uri() {
        let _url = Url::parse("https://relay.com/lnurlp/test").unwrap();
        let transport = crate::common::test_helpers::MockTransport::wallet_not_found();
        let result = create_invoice(&transport, "not-a-valid-uri", "test", 1000);
        let err = futures::executor::block_on(result).unwrap_err();
        assert!(matches!(err, NwcError::ProtocolError(_)));
    }
