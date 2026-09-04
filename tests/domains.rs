use zenpi::{
    b3::ResourceBudget,
    domains::{
        Blueprint, BlueprintItem, DomainError, Goal, GoalStatus, Learn, LeaseRef,
        MAX_BLUEPRINT_ITEMS, MAX_ESTIMATED_LOC_EXCLUSIVE, MAX_LEARN_EVIDENCE,
    },
};

fn blueprint() -> Blueprint {
    Blueprint::new(
        "agent-plan",
        "2.0.0",
        vec![
            BlueprintItem::new("build", 120),
            BlueprintItem::new("test", 240).with_dependencies(["build"]),
        ],
    )
    .unwrap()
}

#[test]
fn blueprint_is_content_addressed_and_dag_validated() {
    let plan = blueprint();
    assert_eq!(plan.items[1].depends_on, vec!["build"]);
    assert_eq!(plan.digest.len(), 64);
    assert!(plan.validate().is_ok());

    let wire = plan.encode_json().unwrap();
    assert_eq!(Blueprint::decode_json(&wire).unwrap(), plan);

    let mut tampered = plan.clone();
    tampered.items[0].estimated_loc += 1;
    assert_eq!(tampered.validate(), Err(DomainError::DigestMismatch));

    let missing = Blueprint::new(
        "bad",
        "1",
        vec![BlueprintItem::new("a", 1).with_dependencies(["missing"])],
    );
    assert!(matches!(
        missing,
        Err(DomainError::MissingDependency { .. })
    ));

    let cycle = Blueprint::new(
        "cycle",
        "1",
        vec![
            BlueprintItem::new("a", 1).with_dependencies(["b"]),
            BlueprintItem::new("b", 1).with_dependencies(["a"]),
        ],
    );
    assert!(matches!(cycle, Err(DomainError::DependencyCycle { .. })));
}

#[test]
fn each_estimated_loc_is_strictly_below_five_thousand() {
    let too_large = Blueprint::new(
        "large",
        "1",
        vec![BlueprintItem::new("item", MAX_ESTIMATED_LOC_EXCLUSIVE)],
    );
    assert!(matches!(
        too_large,
        Err(DomainError::EstimatedLocExceeded { .. })
    ));

    let too_many = Blueprint::new(
        "many",
        "1",
        (0..=MAX_BLUEPRINT_ITEMS)
            .map(|index| BlueprintItem::new(format!("item-{index}"), 1))
            .collect(),
    );
    assert!(matches!(
        too_many,
        Err(DomainError::TooMany { field: "items", .. })
    ));
}

#[test]
fn goal_links_immutable_blueprint_and_tracks_bounded_status() {
    let plan = blueprint();
    let lease = LeaseRef::new("lease-1", "master", 10_000).unwrap();
    let mut goal = Goal::new(
        "goal-1",
        &plan,
        ResourceBudget {
            tokens: 10_000,
            wall_clock_ms: 60_000,
            attempts: 3,
            disk_bytes: 1_024,
        },
        Some(lease),
    )
    .unwrap();
    assert_eq!(goal.status, GoalStatus::Queued);
    goal.validate_against(&plan).unwrap();
    goal.transition_to(GoalStatus::Running).unwrap();
    goal.transition_to(GoalStatus::Done).unwrap();
    assert!(goal.status.is_terminal());
    assert!(matches!(
        goal.transition_to(GoalStatus::Queued),
        Err(DomainError::InvalidStatusTransition { .. })
    ));

    let changed = Blueprint::new("agent-plan", "2.0.1", plan.items.clone()).unwrap();
    assert_eq!(
        goal.validate_against(&changed),
        Err(DomainError::BlueprintLinkMismatch)
    );
}

#[test]
fn goal_status_tokens_have_one_canonical_vocabulary() {
    let cases = [
        ("queued", GoalStatus::Queued),
        ("RUNNING", GoalStatus::Running),
        ("Paused", GoalStatus::Paused),
        ("blocked", GoalStatus::Blocked),
        ("cancelled", GoalStatus::Cancelled),
        // Accept the common US spelling at the command boundary while
        // retaining the canonical serde spelling `cancelled`.
        ("canceled", GoalStatus::Cancelled),
        ("DONE", GoalStatus::Done),
    ];
    for (token, expected) in cases {
        assert_eq!(GoalStatus::parse_token(token), Some(expected));
        assert_eq!(
            expected.as_str(),
            match expected {
                GoalStatus::Queued => "queued",
                GoalStatus::Running => "running",
                GoalStatus::Paused => "paused",
                GoalStatus::Blocked => "blocked",
                GoalStatus::Cancelled => "cancelled",
                GoalStatus::Done => "done",
            }
        );
    }
    for invalid in ["", "ready", "in-progress", "done now", "\n"] {
        assert_eq!(GoalStatus::parse_token(invalid), None);
    }
}

#[test]
fn learn_keeps_bounded_source_target_and_evidence_refs() {
    let mut learn = Learn::new(
        "learn-1",
        "src/tree",
        "target/notes",
        vec!["tests/receipt.json".into()],
    )
    .unwrap();
    learn.add_evidence("Docs/mapping.md").unwrap();
    assert_eq!(learn.evidence.len(), 2);

    let too_many = Learn::new(
        "learn-many",
        "src",
        "dst",
        (0..=MAX_LEARN_EVIDENCE)
            .map(|index| format!("evidence-{index}"))
            .collect(),
    );
    assert!(matches!(
        too_many,
        Err(DomainError::TooMany {
            field: "evidence",
            ..
        })
    ));
}

#[test]
fn rejected_learn_evidence_does_not_mutate_the_record() {
    let mut learn = Learn::new("learn-atomic", "src", "dst", Vec::new()).unwrap();
    let before = learn.clone();
    let too_long = "x".repeat(zenpi::domains::MAX_TEXT_BYTES + 1);
    assert!(learn.add_evidence(too_long).is_err());
    assert_eq!(learn, before);
    assert!(learn.validate().is_ok());
}
