use super::*;

    #[test]
    fn test_nwc_uri_from_uri() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri).unwrap();
        assert_eq!(
            nwc_uri.wallet_pubkey,
            "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f"
        );
        assert_eq!(
            nwc_uri.secret,
            "0101010101010101010101010101010101010101010101010101010101010101"
        );
    }

    #[test]
    fn test_nwc_client_roundtrip() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let client = NwcClient::new(nwc_uri).unwrap();

        let payload = serde_json::json!({"test": "data"});
        let encrypted = client.encrypt(&payload).unwrap();
        let decrypted = client.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);

        let (event, request_id) = client
            .create_request_event(NwcMethod::MakeInvoice, payload.clone(), vec![])
            .unwrap();
        assert_eq!(event.pubkey, client.my_pubkey);
        assert_eq!(event.kind, KIND_NWC_REQUEST);

        // Test parsing back (mocking a response)
        let resp_payload = serde_json::json!({"result": {"invoice": "lnbc1..."}});
        let resp_encrypted = client.encrypt(&resp_payload).unwrap();
        let resp_event = Event {
            id: "resp_id".into(),
            pubkey: client.wallet_pubkey.clone(),
            created_at: crate::util::now(),
            kind: 23195,
            tags: vec![
                Tag::E(request_id.clone(), vec![]),
                Tag::p(&client.my_pubkey),
            ],
            content: resp_encrypted,
            sig: "sig".into(), // verify(now()) will fail in test unless we sign it properly, but we'll bypass verification for this unit test if needed or just sign it.
        };
        
        // Actually we need to sign it to pass verify(now())
        let wallet_sk_bytes = hex::decode("0101010101010101010101010101010101010101010101010101010101010101").unwrap();
        let wallet_sk_arr: [u8; 32] = wallet_sk_bytes.try_into().unwrap();
        let wallet_sk = SigningKey::from_bytes(&wallet_sk_arr).unwrap();
        
        let (resp_id, resp_id_bytes) = Event::compute_id(&client.wallet_pubkey, resp_event.created_at, resp_event.kind, &resp_event.tags, &resp_event.content).unwrap();
        let resp_sig = hex::encode(wallet_sk.sign_prehash(&resp_id_bytes).unwrap().to_bytes());
        
        let signed_resp_event = Event {
            id: resp_id,
            sig: resp_sig,
            ..resp_event
        };

        let parsed_resp = client.parse_response_event(&signed_resp_event, &request_id).unwrap();
        assert_eq!(parsed_resp, resp_payload);
    }

    #[test]
    fn test_nwc_client_nip44_roundtrip() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let mut client = NwcClient::new(nwc_uri).unwrap();
        client.encryption_method = EncryptionMethod::Nip44;

        let payload = serde_json::json!({"test": "nip44 data"});
        let encrypted = client.encrypt(&payload).unwrap();
        let decrypted = client.decrypt(&encrypted).unwrap();
        let decrypted_json: Value = serde_json::from_str(&decrypted).unwrap();
        assert_eq!(payload, decrypted_json);
    }

    #[test]
    #[should_panic(expected = "InvalidScheme")]
    fn test_uri_invalid_scheme() {
        let uri = "http://invalid.example.com?secret=0101010101010101010101010101010101010101010101010101010101010101";
        let _ = NwcUri::from_uri(uri).unwrap();
    }

    #[test]
    fn test_uri_missing_pubkey_returns_error() {
        let uri = "nostr+walletconnect://?secret=0101010101010101010101010101010101010101010101010101010101010101";
        let result = NwcUri::from_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_uri_missing_secret_returns_error() {
        let uri = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f";
        let result = NwcUri::from_uri(uri);
        assert!(result.is_err());
    }

    #[test]
    fn test_client_encrypt_deterministic() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let client = NwcClient::new(nwc_uri).unwrap();

        let payload = serde_json::json!({"same": "data"});
        let encrypted1 = client.encrypt(&payload).unwrap();
        let encrypted2 = client.encrypt(&payload).unwrap();
        assert_ne!(encrypted1, encrypted2);
    }

    #[test]
    fn test_client_created_has_required_fields() {
        let uri_str = "nostr+walletconnect://1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f?relay=ws%3A%2F%2Flocalhost%3A8787%2F&secret=0101010101010101010101010101010101010101010101010101010101010101";
        let nwc_uri = NwcUri::from_uri(uri_str).unwrap();
        let client = NwcClient::new(nwc_uri).unwrap();

        assert!(!client.my_pubkey.is_empty());
    }

    #[test]
    fn test_kind_constants() {
        assert_eq!(KIND_NWC_REQUEST, 23194);
    }

    #[test]
    fn test_encryption_method_from_protocol_str() {
        assert_eq!(EncryptionMethod::from_protocol_str("nip04"), Some(EncryptionMethod::Nip04));
        assert_eq!(EncryptionMethod::from_protocol_str("nip44_v2"), Some(EncryptionMethod::Nip44));
        assert_eq!(EncryptionMethod::from_protocol_str("unknown"), None);
    }

    #[test]
    fn test_encryption_method_to_protocol_string() {
        assert_eq!(EncryptionMethod::Nip04.to_protocol_string(), "nip04");
        assert_eq!(EncryptionMethod::Nip44.to_protocol_string(), "nip44_v2");
    }

    #[test]
    fn test_nwc_uri_error_display() {
        assert!(NwcUriError::InvalidUrl("bad".into()).to_string().contains("url failure"));
        assert!(NwcUriError::InvalidScheme.to_string().contains("Invalid scheme"));
        assert!(NwcUriError::MissingPubkey.to_string().contains("Missing wallet pubkey"));
        assert!(NwcUriError::MissingSecret.to_string().contains("Missing secret"));
    }

    #[test]
    fn test_parse_wallet_info_no_encryption_tag_defaults_nip04() {
        let event = Event {
            id: "id1".into(),
            pubkey: "pk1".into(),
            created_at: 1000,
            kind: 13194,
            tags: vec![],
            content: "".into(),
            sig: "sig".into(),
        };
        let info = parse_wallet_info(&event);
        assert_eq!(info.encryption_algorithms, vec![EncryptionMethod::Nip04]);
    }
