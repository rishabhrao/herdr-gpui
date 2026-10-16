use crate::host::access::{Access, AccessOptions, Verdict, hint, parse_keys, verify};

/// Tokens signed with a throwaway 2048-bit RSA key, and its public half.
const VECTORS: &str = include_str!("access_vectors.json");
/// Before the tokens' expiry, and after the expired one's.
const NOW: u64 = 1_900_000_000;

fn vector(name: &str) -> String {
    let all: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
    all[name].as_str().unwrap().to_owned()
}

fn options() -> AccessOptions {
    AccessOptions {
        team: "myteam".into(),
        audience: "aud-tag-123".into(),
    }
}

fn check(name: &str) -> Verdict {
    let keys = parse_keys(vector("keys").as_bytes()).unwrap();
    verify(&vector(name), &keys, &options(), NOW)
}

#[test]
fn a_token_cloudflare_signed_for_this_app_passes() {
    assert_eq!(check("valid"), Verdict::Valid);
}

#[test]
fn every_other_token_is_refused() {
    for name in [
        "other_aud",
        "other_team",
        "expired",
        "unknown_kid",
        "tampered",
    ] {
        assert!(matches!(check(name), Verdict::Invalid(_)), "{name}");
    }
    let keys = parse_keys(vector("keys").as_bytes()).unwrap();
    assert!(matches!(
        verify("a.b", &keys, &options(), NOW),
        Verdict::Invalid(_)
    ));
    assert!(matches!(
        verify("", &keys, &options(), NOW),
        Verdict::Invalid(_)
    ));
    let huge = "a".repeat(20_000);
    assert!(matches!(
        verify(&huge, &keys, &options(), NOW),
        Verdict::Invalid(_)
    ));
}

#[test]
fn an_unsigned_none_algorithm_token_is_refused() {
    let valid = vector("valid");
    let mut parts = valid.split('.');
    let _ = parts.next();
    let header = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        br#"{"alg":"none","kid":"test-key"}"#,
    );
    let token = format!("{header}.{}.", parts.next().unwrap());
    let keys = parse_keys(vector("keys").as_bytes()).unwrap();
    assert_eq!(
        verify(&token, &keys, &options(), NOW),
        Verdict::Invalid("not RS256")
    );
}

#[test]
fn the_hint_names_the_team_and_audience_without_trusting_them() {
    assert_eq!(
        hint(&vector("valid")),
        Some(("myteam".into(), "aud-tag-123".into()))
    );
    assert_eq!(hint("not a token"), None);
}

#[test]
fn a_key_set_without_rsa_keys_holds_none() {
    assert_eq!(
        parse_keys(br#"{"keys":[{"kid":"k","kty":"EC","n":"","e":""}]}"#),
        None
    );
    assert_eq!(parse_keys(b"nonsense"), None);
}

fn learning(file: Option<std::path::PathBuf>) -> std::sync::Arc<Access> {
    Access::with_fetch(
        None,
        file,
        Box::new(|team| (team == "myteam").then(|| parse_keys(vector("keys").as_bytes()).unwrap())),
    )
}

#[test]
fn the_first_signed_token_names_the_application_and_is_saved() {
    let dir = super::Dir::new("learn");
    let file = dir.0.join("web-access.json");
    let access = learning(Some(file.clone()));
    assert_eq!(access.check(None), Verdict::Invalid("no Access token"));
    // A forged or tampered token teaches nothing.
    assert!(matches!(
        access.check(Some(&vector("tampered"))),
        Verdict::Invalid(_)
    ));
    assert_eq!(access.learned(), None);
    // Tokens expire in 2033; the learning check uses the real clock.
    assert_eq!(access.check(Some(&vector("valid"))), Verdict::Valid);
    assert_eq!(access.learned(), Some(options()));
    // From now on only that application's tokens pass.
    assert!(matches!(
        access.check(Some(&vector("other_aud"))),
        Verdict::Invalid(_)
    ));
    assert_eq!(access.check(Some(&vector("valid"))), Verdict::Valid);
    // A restart reads the saved application instead of learning again.
    let restarted = learning(Some(file));
    assert_eq!(restarted.learned(), Some(options()));
}

#[test]
fn a_token_from_an_unreachable_team_is_not_learned() {
    let access = learning(None);
    assert_eq!(access.check(Some(&vector("other_team"))), Verdict::Unready);
    assert_eq!(access.learned(), None);
}
