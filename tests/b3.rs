use std::collections::BTreeMap;
use zenpi::b3::*;

#[test]
fn signed_handoff_and_artifact_policy_are_fail_closed() {
    let record = HandoffRecord::new(
        "a",
        "b",
        "claim",
        "ready",
        vec!["Docs/a.md".into()],
        "s",
        "2026-01-01T00:00:00Z",
    )
    .unwrap();
    assert!(HandoffRecord::from_wire(record.clone()).is_ok());
    assert!(HandoffRecord::decode_line(&record.encode_line().unwrap()).is_ok());
    let mut tampered = record;
    tampered.summary.push('!');
    assert!(matches!(tampered.validate(), Err(B3Error::DigestMismatch)));
    assert!(validate_artifact_path("../secret").is_err());
    assert!(validate_artifact_path("/tmp/file").is_err());
    assert!(
        HandoffRecord::new(
            "a",
            "b",
            "claim",
            "x".repeat(MAX_HANDOFF_SUMMARY_BYTES + 1),
            Vec::new(),
            "s",
            "2026-01-01T00:00:00Z"
        )
        .is_err()
    );
}

#[test]
fn accounting_evidence_and_gate_records_validate() {
    let limit = ResourceBudget {
        tokens: 10,
        wall_clock_ms: 20,
        attempts: 2,
        disk_bytes: 30,
    };
    let mut envelope = ResourceEnvelope::new("env", "worker", limit).unwrap();
    envelope
        .record(ResourceBudget {
            tokens: 1,
            ..ResourceBudget::default()
        })
        .unwrap();
    let lease = ResourceLease {
        lease_id: "lease".into(),
        owner: "worker".into(),
        workspace: "work".into(),
        issued_at_ms: 1,
        expires_at_ms: 10,
        budget: limit,
        spent: ResourceBudget::default(),
        status: LeaseStatus::Leased,
        parent: Some(ParentLeaseRef {
            parent_lease_id: "parent".into(),
            nested_run_id: "run".into(),
            max_tokens: 1,
        }),
    };
    assert!(lease.validate().is_ok());
    assert!(lease.validate_at(10).is_err());
    assert!(
        EvidenceRecord {
            evidence_id: "e".into(),
            claim_id: "c".into(),
            changed_paths: vec!["src/lib.rs".into()],
            commands: vec!["cargo test".into()],
            validation: "passed".into(),
            master_state: "self_tested".into()
        }
        .validate()
        .is_ok()
    );
    assert!(
        RouteDecision {
            route_id: "r".into(),
            parent_ref: "p".into(),
            route_class: "local".into(),
            runner: "echo".into(),
            validator_strength: "strict".into()
        }
        .validate()
        .is_ok()
    );
    assert!(
        EstimatorPolicy {
            estimate_id: "est".into(),
            task_ref: "task".into(),
            estimated_parameters: BTreeMap::new(),
            hard_caps: limit,
            rationale: "bounded".into()
        }
        .validate()
        .is_ok()
    );
    assert!(
        LooperLog {
            log_id: "log".into(),
            grain: LogGrain::Task,
            target_ref: "target".into(),
            instrument_ref: "tool".into(),
            observed_effect: ObservedEffect::Helped,
            target_movement: "forward".into(),
            evidence_refs: vec!["e".into()],
            master_state: "self_tested".into()
        }
        .validate()
        .is_ok()
    );
    let gate =
        SideEffectGate::deny("g", SideEffectKind::PushOrPublish, "master", "wait", 1).unwrap();
    assert_eq!(
        gate.authorize(SideEffectKind::PushOrPublish),
        Err(B3Error::SideEffectDenied)
    );
}

#[test]
fn manifest_requires_worker_self_test_then_master() {
    let check = ValidationRecord::new("cargo test", ValidationOutcome::Passed).unwrap();
    let mut manifest =
        ResultManifest::new("claim", "worker", vec!["src/lib.rs".into()], vec![check]).unwrap();
    assert_eq!(
        manifest.accept_by("worker"),
        Err(B3Error::MasterAuthorityRequired)
    );
    manifest.mark_self_tested().unwrap();
    manifest.accept_by("master").unwrap();
    assert!(manifest.is_accepted());
}

#[test]
fn checked_budget_addition_does_not_wrap() {
    let mut envelope = ResourceEnvelope::new(
        "e",
        "w",
        ResourceBudget {
            tokens: u64::MAX,
            ..ResourceBudget::default()
        },
    )
    .unwrap();
    envelope.spent.tokens = u64::MAX;
    assert_eq!(
        envelope.record(ResourceBudget {
            tokens: 1,
            ..ResourceBudget::default()
        }),
        Err(B3Error::BudgetExceeded { field: "envelope" })
    );
}
