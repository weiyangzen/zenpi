use zenpi::{
    context::{ContextBudget, ContextError, estimate_tokens, prepare_context},
    core::{Turn, TurnRole},
};

fn turns(count: usize, bytes: usize) -> Vec<Turn> {
    (0..count)
        .map(|index| {
            Turn::new(
                format!("turn-{index}"),
                if index % 2 == 0 {
                    TurnRole::User
                } else {
                    TurnRole::Assistant
                },
                format!("{index}:{}", "x".repeat(bytes)),
            )
        })
        .collect()
}

#[test]
fn token_estimate_is_explicitly_approximate() {
    let estimate = estimate_tokens(&turns(2, 100));
    assert!(estimate.approximate);
    assert!(estimate.input_tokens > 0);
}

#[test]
fn compaction_is_deterministic_and_keeps_newest_turns() {
    let source = turns(20, 500);
    let budget = ContextBudget {
        max_tokens: 1_500,
        reserved_output_tokens: 300,
    };
    let first = prepare_context(&source, budget, &|| false).unwrap();
    let second = prepare_context(&source, budget, &|| false).unwrap();
    assert_eq!(first.checkpoint, second.checkpoint);
    assert_eq!(first.turns, second.turns);
    assert!(first.checkpoint.is_some());
    assert_eq!(first.turns.last().unwrap().id, source.last().unwrap().id);
    assert!(first.estimate.input_tokens <= 1_200);
}

#[test]
fn cancellation_during_needed_compaction_preserves_source() {
    let source = turns(20, 500);
    let before = source.clone();
    let error = prepare_context(
        &source,
        ContextBudget {
            max_tokens: 1_500,
            reserved_output_tokens: 300,
        },
        &|| true,
    )
    .unwrap_err();
    assert!(matches!(error, ContextError::Cancelled));
    assert_eq!(source, before);
}
