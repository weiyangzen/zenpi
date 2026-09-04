use serde_json::json;

use zenpi::{
    backend::ProviderEvent,
    core::{AgentEvent, NotSubmittedReason},
    protocol::TurnMode,
    render::MarkdownBlock,
    tools::ToolCall,
    view_model::{
        MAX_VIEW_LIST_ITEMS, MAX_VIEW_TEXT_BYTES, ToolStatus, VIEW_MODEL_VERSION, ViewBlock,
        ViewEvent, ViewEventBuffer, ViewEventKind, ViewMessage, ViewModelError, ViewRejection,
        ViewRole, ViewTurnMode,
    },
};

#[test]
fn blocks_and_messages_round_trip_with_bounded_shapes() {
    let message = ViewMessage::new(
        ViewRole::Assistant,
        Some("turn-1".into()),
        vec![
            ViewBlock::Paragraph {
                text: "hello **world**".into(),
            },
            ViewBlock::Code {
                language: Some("rust".into()),
                text: "fn main() {}".into(),
            },
            ViewBlock::Diff {
                path: Some("src/lib.rs".into()),
                patch: "@@ -1 +1 @@\n-old\n+new".into(),
            },
            ViewBlock::ToolStatus {
                call_id: "call-1".into(),
                name: "read_file".into(),
                status: ToolStatus::Succeeded,
                output: Some("ok".into()),
            },
        ],
    )
    .unwrap();

    let encoded = serde_json::to_string(&message).unwrap();
    let decoded: ViewMessage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, message);
    assert_eq!(
        decoded.blocks[0].kind(),
        zenpi::view_model::ViewBlockKind::Paragraph
    );
    decoded.validate().unwrap();
}

#[test]
fn markdown_parser_maps_to_shared_blocks() {
    let fence = "\x60\x60\x60";
    let markdown = format!(
        "# Heading\n\n- first\n- second\n\n> quoted\n\n{fence}rust\nfn main() {{}}\n{fence}\n"
    );
    let blocks = ViewBlock::from_markdown_text(&markdown).unwrap();
    assert!(matches!(blocks[0], ViewBlock::Heading { level: 1, .. }));
    assert!(matches!(blocks[1], ViewBlock::List { ordered: false, .. }));
    assert!(matches!(blocks[2], ViewBlock::List { ordered: false, .. }));
    assert!(matches!(blocks[3], ViewBlock::Quote { .. }));
    assert!(matches!(blocks[4], ViewBlock::Code { .. }));

    let from_existing = ViewBlock::from_markdown(&MarkdownBlock::Rule).unwrap();
    assert_eq!(from_existing.kind(), zenpi::view_model::ViewBlockKind::Rule);
}

#[test]
fn validation_rejects_control_bytes_and_unbounded_blocks() {
    let bad = ViewBlock::PlainText {
        text: "\u{1b}[31m".into(),
    };
    assert!(matches!(
        bad.validate(),
        Err(ViewModelError::ControlCharacter {
            field: "block text"
        })
    ));

    let mut items = Vec::with_capacity(MAX_VIEW_LIST_ITEMS + 1);
    items.resize(MAX_VIEW_LIST_ITEMS + 1, "item".to_owned());
    let bad_list = ViewBlock::List {
        ordered: false,
        items,
    };
    assert!(matches!(
        bad_list.validate(),
        Err(ViewModelError::TooLarge {
            field: "list items",
            max: MAX_VIEW_LIST_ITEMS
        })
    ));

    let long = ViewBlock::Paragraph {
        text: "x".repeat(MAX_VIEW_TEXT_BYTES + 1),
    };
    assert!(matches!(
        long.validate(),
        Err(ViewModelError::TooLarge {
            field: "block text",
            max: MAX_VIEW_TEXT_BYTES
        })
    ));
}

#[test]
fn event_envelope_requires_associations_and_flattens_payload() {
    let missing = ViewEvent::new(
        4,
        ViewEventKind::TextDelta {
            delta: "chunk".into(),
        },
    );
    assert!(matches!(
        missing,
        Err(ViewModelError::MissingTurn { field: "event" })
    ));

    let event = ViewEvent::with_context(
        4,
        Some("request-1".into()),
        Some("turn-1".into()),
        Some("turn-1:assistant".into()),
        ViewEventKind::TextDelta {
            delta: "chunk".into(),
        },
    )
    .unwrap();
    assert_eq!(event.schema_version, VIEW_MODEL_VERSION);
    let value = serde_json::to_value(&event).unwrap();
    assert_eq!(value["type"], "text_delta");
    assert_eq!(value["sequence"], 4);
    assert_eq!(value["request_id"], "request-1");
    assert_eq!(value["turn_id"], "turn-1");
    assert_eq!(value["block_id"], "turn-1:assistant");
    assert_eq!(value["delta"], "chunk");
    let round_trip: ViewEvent = serde_json::from_value(value).unwrap();
    assert_eq!(round_trip, event);
}

#[test]
fn agent_and_provider_adapters_preserve_turn_and_block_correlation() {
    let accepted = AgentEvent::TurnAccepted {
        turn_id: "turn-2".into(),
        mode: TurnMode::StartIfIdle,
    };
    let normalized = ViewEvent::from_agent_event(0, Some("request-2".into()), &accepted).unwrap();
    assert_eq!(normalized.turn_id.as_deref(), Some("turn-2"));
    assert!(matches!(
        normalized.event,
        ViewEventKind::RequestAccepted {
            mode: ViewTurnMode::StartIfIdle
        }
    ));

    let assistant = AgentEvent::AssistantMessage {
        turn_id: "turn-2".into(),
        content: "answer".into(),
    };
    let normalized = ViewEvent::from_agent_event(1, None, &assistant).unwrap();
    assert_eq!(normalized.block_id.as_deref(), Some("turn-2:assistant"));
    assert!(matches!(
        normalized.event,
        ViewEventKind::Block {
            block: ViewBlock::Paragraph { .. }
        }
    ));

    let provider = ProviderEvent::TextDelta {
        delta: "stream".into(),
    };
    let normalized =
        ViewEvent::from_provider_event(2, None, Some("turn-2"), None, &provider).unwrap();
    assert_eq!(normalized.block_id.as_deref(), Some("turn-2:assistant"));
    assert!(matches!(
        normalized.event,
        ViewEventKind::TextDelta { ref delta } if delta == "stream"
    ));

    let tool = ProviderEvent::ToolCallDone {
        call: ToolCall {
            id: "call-2".into(),
            name: "read_file".into(),
            arguments: json!({"path":"README.md"}),
        },
    };
    let normalized = ViewEvent::from_provider_event(3, None, Some("turn-2"), None, &tool).unwrap();
    assert_eq!(normalized.block_id, None);
    assert!(matches!(
        normalized.event,
        ViewEventKind::ToolCallReady { ref call_id, .. } if call_id == "call-2"
    ));

    let rejected = AgentEvent::TurnRejected {
        reason: NotSubmittedReason::NoActiveTurn,
    };
    let normalized = ViewEvent::from_agent_event(4, None, &rejected).unwrap();
    assert!(matches!(
        normalized.event,
        ViewEventKind::RequestRejected {
            reason: ViewRejection::NoActiveTurn
        }
    ));

    let error = AgentEvent::Error {
        message: "backend unavailable".into(),
    };
    let normalized = ViewEvent::from_agent_event(5, None, &error).unwrap();
    assert!(matches!(
        normalized.event,
        ViewEventKind::TurnFailed {
            retryable: false,
            ..
        }
    ));
}

#[test]
fn bounded_buffer_keeps_order_and_reports_replay_gaps() {
    let mut buffer = ViewEventBuffer::new(2, 16 * 1024);
    let first = buffer
        .push(
            Some("r".into()),
            Some("t".into()),
            None,
            ViewEventKind::RequestAccepted {
                mode: ViewTurnMode::StartOrSteer,
            },
        )
        .unwrap();
    let second = buffer
        .push(
            Some("r".into()),
            Some("t".into()),
            None,
            ViewEventKind::ToolStarted {
                call_id: "c".into(),
                name: "read_file".into(),
            },
        )
        .unwrap();
    let third = buffer
        .push(
            Some("r".into()),
            Some("t".into()),
            None,
            ViewEventKind::ToolFinished {
                call_id: "c".into(),
                status: ToolStatus::Succeeded,
                output: None,
            },
        )
        .unwrap();
    assert_eq!((first, second, third), (0, 1, 2));
    assert_eq!(buffer.next_sequence(), 3);
    assert_eq!(buffer.len(), 2);
    assert_eq!(buffer.dropped(), 1);
    assert!(matches!(
        buffer.replay_from(0),
        Err(ViewModelError::ReplayGap {
            requested: 0,
            first_available: 1
        })
    ));
    let suffix = buffer.replay_from(1).unwrap();
    assert_eq!(
        suffix
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(matches!(
        buffer.replay_from(4),
        Err(ViewModelError::ReplayFuture {
            requested: 4,
            next_sequence: 3
        })
    ));
    assert_eq!(buffer.take_dropped(), 1);
    assert_eq!(buffer.dropped(), 0);
}

#[test]
fn buffer_rejects_sequence_discontinuity_without_mutating_state() {
    let mut buffer = ViewEventBuffer::new(4, 16 * 1024);
    let event = ViewEvent::with_context(
        3,
        None,
        Some("turn".into()),
        None,
        ViewEventKind::TurnCompleted {
            response_id: None,
            model: None,
        },
    )
    .unwrap();
    assert!(matches!(
        buffer.append(event),
        Err(ViewModelError::SequenceDiscontinuity {
            expected: 0,
            actual: 3
        })
    ));
    assert_eq!(buffer.next_sequence(), 0);
    assert!(buffer.is_empty());
}
