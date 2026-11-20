use super::*;

const WALLET_PK: &str = "56b328b30c8bf5839e24058747879408bdb36241dc9c2e7c619faa12b2920967";
const SECRET: &str = "0909090909090909090909090909090909090909090909090909090909090909";

fn uri() -> String {
    format!("nostr+walletconnect://{WALLET_PK}?relay=wss%3A%2F%2Frelay.example&secret={SECRET}")
}

fn methods(names: &[&str]) -> Value {
    serde_json::json!({ "methods": names })
}

fn record() -> LnAddressRecord {
    LnAddressRecord::new(
        "alice",
        &uri(),
        Some(vec!["get_info".to_string(), REQUIRED_METHOD.to_string()]),
    )
}

#[test]
fn test_parse_accepts_a_connection_string() {
    let registration = parse("alice", &uri()).expect("valid submission");
    assert_eq!(registration.username, "alice");
    assert_eq!(registration.nwc_uri, uri());
    assert_eq!(registration.wallet_pubkey, WALLET_PK);
}

#[test]
fn test_parse_trims_surrounding_whitespace() {
    let padded = format!("  {}\n", uri());
    let registration = parse("alice", &padded).expect("valid submission");
    assert_eq!(registration.nwc_uri, uri());
}

#[test]
fn test_parse_rejects_an_invalid_username() {
    for username in ["", ".", "..", "alice smith", "Alice/", &"a".repeat(65)] {
        assert_eq!(
            parse(username, &uri()),
            Err(RegistrationError::Username),
            "username {username:?} should be refused"
        );
    }
}

#[test]
fn test_parse_rejects_another_scheme() {
    assert_eq!(
        parse("alice", "https://wallet.example?secret=00"),
        Err(RegistrationError::NotWalletConnect)
    );
}

#[test]
fn test_parse_rejects_a_missing_secret() {
    let no_secret = format!("nostr+walletconnect://{WALLET_PK}?relay=wss%3A%2F%2Frelay.example");
    assert_eq!(
        parse("alice", &no_secret),
        Err(RegistrationError::NotWalletConnect)
    );
}

#[test]
fn test_parse_rejects_unusable_key_material() {
    // Parses as a URI, but neither of these can be used to sign a request.
    let bad_secret = format!("nostr+walletconnect://{WALLET_PK}?secret=zz");
    assert_eq!(
        parse("alice", &bad_secret),
        Err(RegistrationError::NotWalletConnect)
    );

    let short_secret = format!("nostr+walletconnect://{WALLET_PK}?secret=0102");
    assert_eq!(
        parse("alice", &short_secret),
        Err(RegistrationError::NotWalletConnect)
    );

    let short_key = format!("nostr+walletconnect://0011?secret={SECRET}");
    assert_eq!(
        parse("alice", &short_key),
        Err(RegistrationError::NotWalletConnect)
    );
}

#[test]
fn test_documented_limits() {
    assert_eq!(MAX_NWC_URI_LEN, 1024);
    assert_eq!(MAX_SUBMISSION_LEN, 4096);
    assert_eq!(MAX_METHODS, 32);
    assert_eq!(MAX_METHOD_LEN, 32);
}

#[test]
fn test_parse_accepts_a_uri_exactly_at_the_limit() {
    let base = uri();
    let padding = MAX_NWC_URI_LEN - base.len() - "&pad=".len();
    let exact = format!("{base}&pad={}", "a".repeat(padding));
    assert_eq!(exact.len(), MAX_NWC_URI_LEN);
    assert!(parse("alice", &exact).is_ok());
}

#[test]
fn test_parse_rejects_an_oversized_uri() {
    let padded = format!("{}&padding={}", uri(), "a".repeat(MAX_NWC_URI_LEN));
    assert_eq!(parse("alice", &padded), Err(RegistrationError::TooLong));
}

#[test]
fn test_granted_methods_keeps_what_the_wallet_reports() {
    let granted = granted_methods(&methods(&["get_info", "pay_invoice", "lookup_invoice"]))
        .expect("pay_invoice is granted");
    assert_eq!(
        granted,
        vec![
            "get_info".to_string(),
            "pay_invoice".to_string(),
            "lookup_invoice".to_string()
        ]
    );
}

#[test]
fn test_granted_methods_requires_pay_invoice() {
    assert_eq!(
        granted_methods(&methods(&["get_info", "get_balance"])),
        Err(RegistrationError::MissingScope)
    );
}

#[test]
fn test_granted_methods_requires_the_field() {
    assert_eq!(
        granted_methods(&serde_json::json!({ "alias": "wallet" })),
        Err(RegistrationError::MissingScope)
    );
    assert_eq!(
        granted_methods(&serde_json::json!({ "methods": "pay_invoice" })),
        Err(RegistrationError::MissingScope)
    );
}

#[test]
fn test_granted_methods_skips_unusable_names() {
    let grant = methods(&[
        "",
        "Pay_Invoice",
        "pay-invoice",
        "pay_invoice",
        &"x".repeat(MAX_METHOD_LEN + 1),
        " get_info",
    ]);
    assert_eq!(
        granted_methods(&grant).expect("pay_invoice is granted"),
        vec!["pay_invoice".to_string()]
    );
}

#[test]
fn test_granted_methods_deduplicates() {
    let granted =
        granted_methods(&methods(&["pay_invoice", "pay_invoice"])).expect("pay_invoice is granted");
    assert_eq!(granted, vec!["pay_invoice".to_string()]);
}

#[test]
fn test_granted_methods_is_bounded() {
    let mut names = vec!["pay_invoice".to_string()];
    for index in 0..(MAX_METHODS * 2) {
        names.push(format!("method_{index}"));
    }
    let reported: Vec<&str> = names.iter().map(String::as_str).collect();
    let granted = granted_methods(&methods(&reported)).expect("pay_invoice is granted");
    assert_eq!(granted.len(), MAX_METHODS);
    assert_eq!(granted.first().map(String::as_str), Some(REQUIRED_METHOD));
}

#[test]
fn test_granted_methods_ignores_non_strings() {
    let grant = serde_json::json!({ "methods": [1, null, ["pay_invoice"], "pay_invoice"] });
    assert_eq!(
        granted_methods(&grant).expect("pay_invoice is granted"),
        vec!["pay_invoice".to_string()]
    );
}

#[test]
fn test_summary_shows_the_wallet_without_the_secret() {
    let view = summary(&record());
    assert_eq!(view["username"], "alice");
    assert_eq!(view["wallet_pubkey"], WALLET_PK);
    assert_eq!(view["methods"][0], "get_info");

    let text = view.to_string();
    assert!(!text.contains(SECRET));
    assert!(!text.contains("nostr+walletconnect"));
}

#[test]
fn test_summary_omits_methods_when_never_verified() {
    let view = summary(&LnAddressRecord::new("bob", &uri(), None));
    assert_eq!(view["username"], "bob");
    assert!(view.get("methods").is_none());
}

#[test]
fn test_summary_survives_an_unparsable_uri() {
    let view = summary(&LnAddressRecord::new("bob", "not-a-uri", None));
    assert_eq!(view["username"], "bob");
    assert!(view.get("wallet_pubkey").is_none());
}
