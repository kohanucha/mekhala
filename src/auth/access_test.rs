use super::*;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::PrivateKey;

/// A 2048-bit key generated for these tests. It signs fixtures only, and is
/// public by construction.
const TEST_KEY_PKCS8: &str = "MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCrcvO3LOBeTMPGB6sJS8b9tVtr6qNGllTCB2aVCPRETzjZGeHRBRg8dmhKSyBJJhc08EONEDB7cPpcdnktaR3k7Ag9oRX6nb9LfIJTHL-iMSkEFZ3hPPfQjjdRP0C_GnIBnDOocRuW751hGOxiUaJBIzwdMDVJs3AntcAKR-IXXhvBS-pryXWjl7RgcFvpoCJS3u54t-UCIQXSO5zA6WebSpAwSV91T4869peP27YPArPwgQXqQ-9UCSMqwUUxrCozM6e6CcuFqG6jS6QAlBKSPUIcwsx9npXPerBg3s6muxOJAVqYflx4IpOYc5PoVIuOphJTPp-BGK9fo6EymK21AgMBAAECggEAHHyq4IxxElIUdF65TT_dRjIASPfRXczkM-IJail09rWoFzCM_A8p5f_xl6iQan8M-5ORUcq3F4ef3cWkoEY3PJ54Rw8xKPVVf9wmBlvAPpmG_GIAG9jmvy0mJVaKckZZW4mh7rKueB-DqYq72iFAwaxyVFI6hG6va7Wv_TVQ7HtFc2-L4thhRfd06owU3WWJRzSObG2lSrXsqf_GCcC3fzHQjiAg0osn66bv-8dJoIZ0BK6SB_UdbC7CkFKzgB3Rimsz7TLsitwDtKLGd7KhauD0MOGpet--VtXfTvEP4-5rsmwjKLOter83PrJDJTKxvd5D_CjpGZ2ariV92TnCSQKBgQDiOmU62-WxdADlsXVHvCZCHOqGgN0ZJN0SfsXMNLFJAN7gJd6L4O8PAwWjJ_5ivxod8_MJAI5RyHrxIJABK9fHXcHw-O1TtFI9dzp5REPn2alzqK5l-cEIBon20jV2-UEkaxd7-j0wlrYD9-58NCayS9E6N90dGXIfATcA385q2QKBgQDCAw3wX80BpiXIXYJImSnPdGHK3eeOiZifZK6tXABTaE8SIRl9sJO3i-0Vj4lFC9vOt60Cbl0id1_LOCouiJfjAegTYj-RXCQQBTNXYXDoK-ZmOO71hacc3gd5ArmFW0qzWoxaYyfdePeUDgPOZYCxAW2Tm64e2QUhKFG4R7_4PQKBgQC4mtxWTnC-AKJmzQueEPvLQVtjjK5fQUShPg_cVjdBElPM5zoWPOP3BQlJnW4pWDZQsOvmsaxxMj6Ym3DHc-ZtyqVjJU5M2r2taKivojQjcuui9MfzcP3CtctJPTnhrv4oYLdWNNpP1vwKevwCIHBfWB0Q-9sUTtpgUJZ4wSjo6QKBgQCxpaxYws_rx5_nBMBsC1r-a3m0EVaa7KEEqmNLt-dQeD2WwxodPdc0hPqU-HZf88jUUtknVbkP-zCK2yW8sDQrl65SOW8tgao_xRNK2p1Q6gmw5kbgJ_5p71_2TSlk07_l1G6ZwRtarw470k1n6wdYjI8yTqWz91G8F7gkPgaonQKBgGgh4H9aBNOiuWekckz98RitRF5ozOen1fb38W0S-1jQ5p3QJOBlZWVk_Q36dbBAqbOLJ2Ntq22deE1v2qm6ZGSOQPdEh8hOT6-npTipYGIrb7AmvZ5uh5f_7cpdBsSTcQQyP-Ipy_jtiD0TyR0tE4u8cOSPqA2QUDOYIE48X8Ff";

/// Modulus of [`TEST_KEY_PKCS8`], base64url, as a key document publishes it.
const TEST_MODULUS: &str = "q3LztyzgXkzDxgerCUvG_bVba-qjRpZUwgdmlQj0RE842Rnh0QUYPHZoSksgSSYXNPBDjRAwe3D6XHZ5LWkd5OwIPaEV-p2_S3yCUxy_ojEpBBWd4Tz30I43UT9AvxpyAZwzqHEblu-dYRjsYlGiQSM8HTA1SbNwJ7XACkfiF14bwUvqa8l1o5e0YHBb6aAiUt7ueLflAiEF0jucwOlnm0qQMElfdU-POvaXj9u2DwKz8IEF6kPvVAkjKsFFMawqMzOnugnLhahuo0ukAJQSkj1CHMLMfZ6Vz3qwYN7OprsTiQFamH5ceCKTmHOT6FSLjqYSUz6fgRivX6OhMpittQ";

const TEST_EXPONENT: &str = "AQAB";

const ISSUER: &str = "https://mekhala-test.cloudflareaccess.com";
const AUDIENCE: &str = "0123456789abcdef0123456789abcdef";
const NOW: u64 = 1_700_000_000;

fn key_set(kid: &str) -> AccessKeys {
    let document = format!(
        r#"{{"keys":[{{"kty":"RSA","kid":"{kid}","alg":"RS256","n":"{TEST_MODULUS}","e":"{TEST_EXPONENT}"}}]}}"#
    );
    AccessKeys::from_jwks(&document).expect("fixture key document")
}

fn signing_key() -> SigningKey<Sha256> {
    let der = URL_SAFE_NO_PAD
        .decode(TEST_KEY_PKCS8)
        .expect("fixture private key");
    let key = PrivateKey::from_pkcs8_der(&der).expect("fixture private key");
    SigningKey::<Sha256>::new(key)
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn claims() -> serde_json::Value {
    serde_json::json!({
        "iss": ISSUER,
        "aud": [AUDIENCE],
        "exp": NOW + 300,
        "nbf": NOW - 10,
        "type": "app",
        "common_name": "ops@example.com",
    })
}

/// Builds a signed token; `header` and `claims` are serialised verbatim.
fn sign_with(header: &str, claims: &serde_json::Value) -> String {
    let signing_input = format!(
        "{}.{}",
        b64(header.as_bytes()),
        b64(claims.to_string().as_bytes())
    );
    let signature = signing_key().sign(signing_input.as_bytes());
    format!("{signing_input}.{}", b64(&signature.to_bytes()))
}

fn valid_token() -> String {
    sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims())
}

#[test]
fn test_accepts_a_valid_token() {
    let claims = key_set("test-key")
        .verify(&valid_token(), ISSUER, AUDIENCE, NOW)
        .expect("valid token");
    assert_eq!(claims.subject.as_deref(), Some("ops@example.com"));
}

#[test]
fn test_audience_may_be_a_bare_string() {
    let mut claims = claims();
    claims["aud"] = serde_json::json!(AUDIENCE);
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert!(key_set("test-key")
        .verify(&token, ISSUER, AUDIENCE, NOW)
        .is_ok());
}

#[test]
fn test_rejects_an_edited_payload() {
    let token = valid_token();
    let mut parts = token.split('.');
    let header = parts.next().expect("header");
    parts.next();
    let signature = parts.next().expect("signature");

    let mut edited = claims();
    edited["common_name"] = serde_json::json!("attacker@example.com");
    let forged = format!(
        "{header}.{}.{signature}",
        b64(edited.to_string().as_bytes())
    );

    assert_eq!(
        key_set("test-key").verify(&forged, ISSUER, AUDIENCE, NOW),
        Err(JwtError::InvalidSignature)
    );
}

#[test]
fn test_rejects_an_unknown_key_id() {
    // The token names a key id the document no longer publishes.
    assert_eq!(
        key_set("rotated-key").verify(&valid_token(), ISSUER, AUDIENCE, NOW),
        Err(JwtError::InvalidSignature)
    );
}

#[test]
fn test_rejects_another_algorithm() {
    let token = sign_with(r#"{"alg":"HS256","kid":"test-key"}"#, &claims());
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::UnsupportedAlgorithm)
    );
}

#[test]
fn test_rejects_an_expired_token() {
    let mut claims = claims();
    claims["exp"] = serde_json::json!(NOW);
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::Expired)
    );
}

#[test]
fn test_rejects_a_token_that_is_not_yet_valid() {
    let mut claims = claims();
    claims["nbf"] = serde_json::json!(NOW + 60);
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::NotYetValid)
    );
}

#[test]
fn test_rejects_another_audience() {
    let mut claims = claims();
    claims["aud"] = serde_json::json!(["some-other-application"]);
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::WrongAudience)
    );
}

#[test]
fn test_rejects_an_account_level_token() {
    let mut claims = claims();
    claims["type"] = serde_json::json!("org");
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::WrongType)
    );
}

#[test]
fn test_rejects_another_team() {
    let mut claims = claims();
    claims["iss"] = serde_json::json!("https://other-team.cloudflareaccess.com");
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::WrongIssuer)
    );
}

#[test]
fn test_documented_limits() {
    assert_eq!(MAX_TOKEN_LEN, 8192);
    assert_eq!(MAX_JWKS_LEN, 65536);
}

#[test]
fn test_a_token_exactly_at_the_limit_is_still_parsed() {
    // The cap is a refusal for anything larger; a token of exactly the limit
    // must reach the parser, which then rejects it as malformed.
    let token = "a".repeat(MAX_TOKEN_LEN);
    assert!(matches!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::Malformed)
    ));
}

#[test]
fn test_a_key_document_exactly_at_the_limit_is_still_parsed() {
    let document = " ".repeat(MAX_JWKS_LEN);
    assert!(matches!(
        AccessKeys::from_jwks(&document),
        Err(JwtError::Malformed)
    ));
}

#[test]
fn test_accepts_a_token_whose_not_before_is_now() {
    let mut claims = claims();
    claims["nbf"] = serde_json::json!(NOW);
    let token = sign_with(r#"{"alg":"RS256","kid":"test-key"}"#, &claims);
    assert!(key_set("test-key")
        .verify(&token, ISSUER, AUDIENCE, NOW)
        .is_ok());
}

#[test]
fn test_rejects_an_oversized_token() {
    let token = "a".repeat(MAX_TOKEN_LEN + 1);
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::TooLarge)
    );
}

#[test]
fn test_rejects_a_token_without_three_parts() {
    assert_eq!(
        key_set("test-key").verify("only.two", ISSUER, AUDIENCE, NOW),
        Err(JwtError::Malformed)
    );
    assert_eq!(
        key_set("test-key").verify("a.b.c.d", ISSUER, AUDIENCE, NOW),
        Err(JwtError::Malformed)
    );
}

#[test]
fn test_rejects_an_undecodable_signature() {
    let token = format!("{}.{}.***", b64(b"{}"), b64(b"{}"));
    assert_eq!(
        key_set("test-key").verify(&token, ISSUER, AUDIENCE, NOW),
        Err(JwtError::Malformed)
    );
}

#[test]
fn test_key_document_skips_other_key_types() {
    let document = format!(
        r#"{{"keys":[
            {{"kty":"EC","kid":"ec-key","crv":"P-256","x":"abc","y":"def"}},
            {{"kty":"RSA","kid":"sha1-key","alg":"RS1","n":"{TEST_MODULUS}","e":"{TEST_EXPONENT}"}},
            {{"kty":"RSA","kid":"test-key","alg":"RS256","n":"{TEST_MODULUS}","e":"{TEST_EXPONENT}"}}
        ]}}"#
    );
    let keys = AccessKeys::from_jwks(&document).expect("one usable key");
    assert!(keys.verify(&valid_token(), ISSUER, AUDIENCE, NOW).is_ok());
}

#[test]
fn test_key_document_without_usable_keys() {
    let document = r#"{"keys":[{"kty":"EC","kid":"ec-key","crv":"P-256","x":"abc","y":"def"}]}"#;
    assert!(matches!(
        AccessKeys::from_jwks(document),
        Err(JwtError::NoKeys)
    ));
    assert!(matches!(
        AccessKeys::from_jwks(r#"{"keys":[]}"#),
        Err(JwtError::NoKeys)
    ));
}

#[test]
fn test_key_document_rejects_garbage() {
    assert!(matches!(
        AccessKeys::from_jwks("not json"),
        Err(JwtError::Malformed)
    ));
    let oversized = format!(r#"{{"keys":[]{}"#, " ".repeat(MAX_JWKS_LEN));
    assert!(matches!(
        AccessKeys::from_jwks(&oversized),
        Err(JwtError::TooLarge)
    ));
}

#[test]
fn test_empty_key_set_verifies_nothing() {
    let keys = AccessKeys::default();
    assert_eq!(
        keys.verify(&valid_token(), ISSUER, AUDIENCE, NOW),
        Err(JwtError::InvalidSignature)
    );
}
