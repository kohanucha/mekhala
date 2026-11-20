use super::*;

fn guard(secret: Option<&str>, access: Option<AccessApp>) -> AdminGuard {
    AdminGuard {
        secret: secret.map(str::to_owned),
        access,
    }
}

fn access_app_fixture() -> AccessApp {
    AccessApp {
        audience: "0123456789abcdef0123456789abcdef".to_owned(),
        team_domain: "mekhala-test.cloudflareaccess.com".to_owned(),
    }
}

#[test]
fn test_bearer_token_reads_the_scheme_case_insensitively() {
    assert_eq!(bearer_token("Bearer s3cret"), Some("s3cret"));
    assert_eq!(bearer_token("bearer s3cret"), Some("s3cret"));
    assert_eq!(bearer_token("BEARER s3cret"), Some("s3cret"));
    assert_eq!(bearer_token("Bearer   s3cret  "), Some("s3cret"));
}

#[test]
fn test_bearer_token_survives_a_multibyte_header() {
    // Slicing at a fixed byte offset would split 'ẞ' and abort the isolate.
    assert_eq!(bearer_token("ẞearer s3cret"), None);
    assert_eq!(bearer_token("ключ s3cret"), None);
    assert_eq!(bearer_token("Bearer ключ"), Some("ключ"));
}

#[test]
fn test_bearer_token_rejects_other_schemes() {
    assert_eq!(bearer_token("Basic s3cret"), None);
    assert_eq!(bearer_token("s3cret"), None);
    assert_eq!(bearer_token(""), None);
    assert_eq!(bearer_token("Bearer "), None);
    assert_eq!(bearer_token("Bearer"), None);
}

#[test]
fn test_a_matching_secret_is_accepted() {
    let guard = guard(Some("s3cret"), None);
    assert!(guard.accepts_secret(Some("Bearer s3cret")));
}

#[test]
fn test_another_secret_is_refused() {
    let guard = guard(Some("s3cret"), None);
    assert!(!guard.accepts_secret(Some("Bearer another")));
    assert!(!guard.accepts_secret(Some("Bearer s3cre")));
    assert!(!guard.accepts_secret(Some("Bearer s3cret2")));
}

#[test]
fn test_no_authorization_header_is_refused() {
    let guard = guard(Some("s3cret"), None);
    assert!(!guard.accepts_secret(None));
}

#[test]
fn test_an_unconfigured_secret_never_matches() {
    let guard = guard(None, Some(access_app_fixture()));
    assert!(!guard.accepts_secret(Some("Bearer anything")));
    assert!(!guard.accepts_secret(Some("Bearer ")));
}

#[test]
fn test_the_relay_secret_shape_is_not_accepted_without_admin_config() {
    // A guard built from a config with only `RELAY_SECRET` set has no admin
    // credential at all, so even that exact value must not pass.
    let guard = guard(None, None);
    assert!(!guard.is_configured());
    assert!(!guard.accepts_secret(Some("Bearer relay-password")));
}

#[test]
fn test_either_credential_configures_the_channel() {
    assert!(!guard(None, None).is_configured());
    assert!(guard(Some("s3cret"), None).is_configured());
    assert!(guard(None, Some(access_app_fixture())).is_configured());
}

#[test]
fn test_access_needs_both_settings() {
    assert!(access_app(Some("aud".into()), Some("team.example".into())).is_some());
    assert!(access_app(Some("aud".into()), None).is_none());
    assert!(access_app(None, Some("team.example".into())).is_none());
    assert!(access_app(None, None).is_none());
}

#[test]
fn test_access_verification_without_config_is_refused() {
    let guard = guard(Some("s3cret"), None);
    let result = futures::executor::block_on(guard.verify_access("a.b.c"));
    assert!(matches!(result, Err(JwtError::NoKeys)));
}
