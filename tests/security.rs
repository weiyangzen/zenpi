use zenpi::security::{child_environment, redact_json, redact_text};

const FIXTURE_SECRET: &str = "sk-fixture-secret-123456";

#[test]
fn redaction_covers_headers_urls_nested_json_and_known_values() {
    let text = format!(
        "Authorization: Bearer {FIXTURE_SECRET} https://user:{FIXTURE_SECRET}@example.test/v1?api_key={FIXTURE_SECRET}"
    );
    let redacted = redact_text(&text, &[FIXTURE_SECRET]);
    assert!(!redacted.contains(FIXTURE_SECRET));
    assert!(!redacted.contains("user:"));
    assert!(redacted.contains("<redacted>"));

    let value = serde_json::json!({
        "authorization": format!("Bearer {FIXTURE_SECRET}"),
        "nested": {"api_key": FIXTURE_SECRET, "message": format!("key={FIXTURE_SECRET}")},
    });
    let encoded = serde_json::to_string(&redact_json(&value, &[FIXTURE_SECRET])).unwrap();
    assert!(!encoded.contains(FIXTURE_SECRET));
    assert!(encoded.contains("<redacted>"));
}

#[test]
fn filtered_child_environment_contains_no_credential_names() {
    let environment = child_environment();
    assert!(environment.iter().any(|(key, _)| key == "PATH"));
    assert!(environment.iter().all(|(key, _)| {
        !key.contains("KEY")
            && !key.contains("TOKEN")
            && !key.contains("AUTH")
            && !key.contains("PASSWORD")
    }));
}

#[test]
fn debug_output_redacts_backend_credentials() {
    let backend = zenpi::backend::OpenAiCompatibleBackend::new(
        "https://example.test/v1",
        Some(FIXTURE_SECRET.into()),
        "fixture",
    )
    .unwrap();
    let debug = format!("{backend:?}");
    assert!(!debug.contains(FIXTURE_SECRET));
    assert!(debug.contains("<redacted>"));
}
