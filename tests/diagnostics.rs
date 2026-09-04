use zenpi::diagnostics::{DiagnosticEvent, Diagnostics, MAX_DIAGNOSTIC_VALUE_BYTES};

const SECRET: &str = "sk-diagnostics-fixture-secret";

#[test]
fn diagnostics_are_quiet_by_default() {
    let diagnostics = Diagnostics::default();
    diagnostics.record(
        DiagnosticEvent {
            kind: "provider_request".into(),
            session_id: Some("session-1".into()),
            request_id: Some("request-1".into()),
            turn_id: Some("turn-1".into()),
            fields: serde_json::json!({"message":"private prompt"}),
        },
        &[],
    );
    diagnostics.increment("provider.requests");
    assert!(diagnostics.events().is_empty());
    assert!(diagnostics.metrics().counters.is_empty());
    let mut output = Vec::new();
    diagnostics.write_jsonl(&mut output).unwrap();
    assert!(output.is_empty());
}

#[test]
fn opt_in_diagnostics_correlate_ids_redact_and_truncate() {
    let diagnostics = Diagnostics::enabled();
    diagnostics.record(
        DiagnosticEvent {
            kind: "provider_request".into(),
            session_id: Some("session-1".into()),
            request_id: Some("request-1".into()),
            turn_id: Some("turn-1".into()),
            fields: serde_json::json!({
                "authorization": format!("Bearer {SECRET}"),
                "summary": format!("{}:{SECRET}", "x".repeat(MAX_DIAGNOSTIC_VALUE_BYTES + 100)),
            }),
        },
        &[SECRET],
    );
    diagnostics.increment("provider.requests");
    let event = &diagnostics.events()[0];
    assert_eq!(event.session_id.as_deref(), Some("session-1"));
    assert_eq!(event.request_id.as_deref(), Some("request-1"));
    assert_eq!(event.turn_id.as_deref(), Some("turn-1"));
    let encoded = serde_json::to_string(event).unwrap();
    assert!(!encoded.contains(SECRET));
    assert!(encoded.contains("<redacted>"));
    assert!(encoded.contains("<truncated>"));
    assert_eq!(diagnostics.metrics().counters["provider.requests"], 1);
}

#[test]
fn metric_cardinality_is_bounded() {
    let diagnostics = Diagnostics::enabled();
    for index in 0..200 {
        diagnostics.increment(&format!("metric_{index}"));
    }
    let metrics = diagnostics.metrics();
    assert_eq!(metrics.counters.len(), 128);
    assert_eq!(metrics.dropped_metric_keys, 72);
}
