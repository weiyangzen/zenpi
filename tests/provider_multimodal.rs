//! PA12: versioned ordered content, typed tool results and the recovery
//! contract around them.  Every stored-content case drives the real admission
//! and resume paths of `Agent` instead of a private copy of the DTO, so the
//! journal field the protocol layer treats as authority is what gets tested.
use std::{fs, path::Path};

use serde_json::{Value, json};
use tempfile::tempdir;
use zenpi::backend::{AttachmentKind, InputAttachment, InputContentPart, InputContentSource};
use zenpi::core::{Agent, AgentError, TurnInputRequest};
use zenpi::session::SessionStore;
use zenpi::tools::ToolResult;

/// The metadata field that makes a turn's ordered content authoritative.
const CONTENT_FIELD: &str = "zenpi_content_v1";
const MAX_TEXT_BYTES: usize = 256 * 1024;

fn agent(dir: &Path, name: &str) -> Agent {
    let mut agent = Agent::with_echo(SessionStore::open(dir.join(name)).unwrap());
    agent.set_attachment_workspace(zenpi::tools::ToolContext::new(dir).unwrap());
    agent
}

fn image(file_id: &str) -> InputAttachment {
    InputAttachment {
        kind: AttachmentKind::Image,
        mime_type: "image/png".into(),
        path: None,
        url: None,
        file_id: Some(file_id.into()),
    }
}

fn stored(parts: Value) -> Value {
    json!({ "version": 1, "parts": parts })
}

fn provider_file_part(file_id: &str, scope: Value) -> Value {
    json!({
        "type": "image",
        "media": {
            "id": file_id,
            "mime_type": "image/png",
            "source": {
                "type": "provider_file",
                "file_id": file_id,
                "media_scope": scope,
            },
        },
    })
}

fn workspace_file_part(path: &str) -> Value {
    json!({
        "type": "file",
        "media": {
            "id": path,
            "mime_type": "text/plain",
            "filename": "note.txt",
            "source": {
                "type": "workspace_file",
                "path": path,
                "size_bytes": 24,
                "sha256": "a".repeat(64),
            },
        },
    })
}

/// A journal written by an older build: the header is the only thing a turn
/// needs, and the metadata is exactly what that build recorded.
fn write_journal(path: &Path, metadata: Option<Value>) {
    let header = json!({
        "kind": "session",
        "version": 1,
        "session_id": "session-crafted",
        "created_at_ms": 1,
        "cwd": std::env::current_dir().unwrap().display().to_string(),
        "schema_version": 1,
        "seq": 0,
    });
    let mut turn = json!({
        "id": "turn-1",
        "role": "user",
        "content": "recorded turn",
        "created_at_ms": 2,
    });
    if let Some(metadata) = metadata {
        turn["metadata"] = metadata;
    }
    let record = json!({
        "kind": "turn",
        "turn": turn,
        "schema_version": 1,
        "session_id": "session-crafted",
        "seq": 1,
    });
    fs::write(path, format!("{header}\n{record}\n")).unwrap();
}

/// Resume a journal that records `metadata` on its only turn.
fn resume_crafted(dir: &Path, name: &str, metadata: Option<Value>) -> Result<Agent, AgentError> {
    let path = dir.join(name);
    write_journal(&path, metadata);
    let mut agent =
        Agent::with_echo(SessionStore::open(dir.join(format!("owner-{name}"))).unwrap());
    agent.resume_session(&path)?;
    Ok(agent)
}

fn resume_error(dir: &Path, name: &str, metadata: Option<Value>) -> String {
    resume_crafted(dir, name, metadata)
        .map(|_| String::new())
        .unwrap_or_else(|error| error.to_string())
}

#[test]
fn ordered_content_records_text_then_attachments_in_admission_order() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"bounded attachment bytes").unwrap();
    let mut agent = agent(dir.path(), "ordered.jsonl");
    let attachments = vec![
        image("provider-image-1"),
        InputAttachment {
            kind: AttachmentKind::File,
            mime_type: "text/plain".into(),
            path: Some("note.txt".into()),
            url: None,
            file_id: None,
        },
        InputAttachment {
            kind: AttachmentKind::Image,
            mime_type: "image/png".into(),
            path: None,
            url: Some("https://example.test/image.png?token=secret".into()),
            file_id: None,
        },
    ];
    agent
        .submit(TurnInputRequest::new("describe these").with_attachments(attachments))
        .unwrap();

    let metadata = agent.history()[0].metadata.clone().unwrap();
    let content = metadata[CONTENT_FIELD].clone();
    assert_eq!(content["version"], 1);
    let parts = content["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 4, "{content}");
    assert_eq!(parts[0], json!({"type": "text", "text": "describe these"}));
    assert_eq!(parts[1]["type"], "image");
    assert_eq!(parts[1]["media"]["source"]["type"], "provider_file");
    assert_eq!(parts[1]["media"]["source"]["file_id"], "provider-image-1");
    assert_eq!(parts[2]["type"], "file");
    assert_eq!(parts[2]["media"]["source"]["type"], "workspace_file");
    assert_eq!(parts[2]["media"]["source"]["path"], "note.txt");
    assert_eq!(parts[2]["media"]["source"]["size_bytes"], 24);
    assert_eq!(
        parts[2]["media"]["source"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(parts[3]["media"]["source"]["type"], "ephemeral");

    // The ordered content keeps references only: no bytes, no token, no signed
    // URL. (The legacy attachment list next to it still records what it always
    // did; only this field is the protocol authority.)
    let encoded = content.to_string();
    assert!(!encoded.contains("base64"), "{encoded}");
    assert!(!encoded.contains("token=secret"), "{encoded}");
    assert!(!encoded.contains("bounded attachment bytes"), "{encoded}");
}

#[test]
fn ordered_content_resumes_with_the_same_order_and_scope() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"bounded attachment bytes").unwrap();
    let mut writer = agent(dir.path(), "writer.jsonl");
    writer
        .submit(
            TurnInputRequest::new("keep the order").with_attachments(vec![
                image("provider-image-1"),
                InputAttachment {
                    kind: AttachmentKind::File,
                    mime_type: "text/plain".into(),
                    path: Some("note.txt".into()),
                    url: None,
                    file_id: None,
                },
            ]),
        )
        .unwrap();
    let recorded = writer.history()[0].metadata.clone().unwrap()[CONTENT_FIELD].clone();
    assert_eq!(recorded["parts"].as_array().unwrap().len(), 3);

    // A resume validates what was recorded, including the media scope the file
    // ID was bound to, and must not reorder the parts.
    let mut reader = Agent::with_echo(SessionStore::open(dir.path().join("reader.jsonl")).unwrap());
    reader
        .resume_session(dir.path().join("writer.jsonl"))
        .unwrap();
    assert_eq!(
        reader.history()[0].metadata.as_ref().unwrap()[CONTENT_FIELD],
        recorded
    );
}

#[test]
fn a_provider_file_from_another_account_is_refused_on_resume() {
    let dir = tempdir().unwrap();
    let error = resume_error(
        dir.path(),
        "foreign.jsonl",
        Some(json!({
            CONTENT_FIELD: stored(json!([provider_file_part("file-9", json!({
                "provider": "other-provider",
                "credential_id": "account-b",
                "identity_scope": "other-identity",
            }))])),
        })),
    );
    assert!(error.contains("media scope"), "{error}");
}

#[test]
fn audio_and_video_content_is_refused_explicitly() {
    let cases = [
        InputContentPart::Image {
            mime_type: "audio/mpeg".into(),
            source: InputContentSource::FileId {
                value: "file-1".into(),
            },
        },
        InputContentPart::File {
            mime_type: "video/mp4".into(),
            source: InputContentSource::Path {
                value: "clip.mp4".into(),
            },
        },
    ];
    for part in cases {
        let error = part.validate().unwrap_err().to_string();
        assert!(error.contains("audio and video"), "{error}");
    }

    // The wire shape a host actually sends is refused the same way, and the
    // stored record is refused on recovery.
    let decoded: InputContentPart = serde_json::from_value(json!({
        "type": "image",
        "mime_type": "video/webm",
        "source": {"type": "file_id", "value": "file-2"},
    }))
    .unwrap();
    assert!(decoded.validate().is_err());

    let dir = tempdir().unwrap();
    let error = resume_error(
        dir.path(),
        "video.jsonl",
        Some(json!({
            CONTENT_FIELD: stored(json!([{
                "type": "image",
                "media": {
                    "id": "file-3",
                    "mime_type": "video/mp4",
                    "source": {
                        "type": "provider_file",
                        "file_id": "file-3",
                        "media_scope": {
                            "provider": "backend",
                            "credential_id": "",
                            "identity_scope": "legacy",
                        },
                    },
                },
            }])),
        })),
    );
    assert!(error.contains("audio and video"), "{error}");
}

#[test]
fn input_content_validation_matches_the_attachment_rules() {
    // Only HTTPS images may be fetched remotely.
    let plain = InputContentPart::Image {
        mime_type: "image/png".into(),
        source: InputContentSource::Url {
            value: "http://example.test/image.png".into(),
        },
    };
    assert!(plain.validate().is_err());
    let remote_file = InputContentPart::File {
        mime_type: "application/pdf".into(),
        source: InputContentSource::Url {
            value: "https://example.test/doc.pdf".into(),
        },
    };
    assert!(remote_file.validate().is_err());
    let image_url = InputContentPart::Image {
        mime_type: "image/png".into(),
        source: InputContentSource::Url {
            value: "https://example.test/image.png".into(),
        },
    };
    image_url.validate().unwrap();
    assert_eq!(
        image_url.to_attachment().unwrap().url.as_deref(),
        Some("https://example.test/image.png")
    );

    // A workspace reference stays relative, and text has no attachment form.
    let escape = InputContentPart::File {
        mime_type: "text/plain".into(),
        source: InputContentSource::Path {
            value: "../secrets.txt".into(),
        },
    };
    assert!(escape.validate().is_err());
    let text = InputContentPart::Text {
        text: "hello".into(),
    };
    text.validate().unwrap();
    assert!(text.to_attachment().is_err());

    // A caller cannot smuggle internal identity or unknown fields.
    assert!(
        serde_json::from_value::<InputContentSource>(json!({
            "type": "file_id",
            "value": "file-1",
            "media_scope": {"provider": "self-certified"},
        }))
        .is_err()
    );
    assert!(
        serde_json::from_value::<InputContentPart>(json!({
            "type": "text",
            "text": "hello",
            "handle_id": "internal",
        }))
        .is_err()
    );
}

#[test]
fn stored_content_limits_and_unknown_versions_are_refused() {
    let dir = tempdir().unwrap();

    let overflow = (0..257)
        .map(|_| json!({"type": "text", "text": "x"}))
        .collect::<Vec<_>>();
    let error = resume_error(
        dir.path(),
        "parts.jsonl",
        Some(json!({CONTENT_FIELD: stored(Value::Array(overflow))})),
    );
    assert!(error.contains("256 parts"), "{error}");

    let error = resume_error(
        dir.path(),
        "text.jsonl",
        Some(json!({
            CONTENT_FIELD: stored(json!([{"type": "text", "text": "x".repeat(MAX_TEXT_BYTES + 1)}])),
        })),
    );
    assert!(error.contains("text exceeds"), "{error}");

    // An unknown version is an error, never "this turn had no content".
    let error = resume_error(
        dir.path(),
        "version.jsonl",
        Some(json!({CONTENT_FIELD: {"version": 2, "parts": []}})),
    );
    assert!(error.contains("version 2 is not supported"), "{error}");

    let error = resume_error(
        dir.path(),
        "field.jsonl",
        Some(json!({CONTENT_FIELD: "not an object"})),
    );
    assert!(error.contains("stored content is invalid"), "{error}");
}

#[test]
fn stored_workspace_references_are_bounded_and_hashed() {
    let dir = tempdir().unwrap();

    let error = resume_error(
        dir.path(),
        "escape.jsonl",
        Some(json!({CONTENT_FIELD: stored(json!([workspace_file_part("../escape.txt")]))})),
    );
    assert!(error.contains("relative workspace path"), "{error}");

    let mut part = workspace_file_part("note.txt");
    part["media"]["source"]["sha256"] = json!("A".repeat(64));
    let error = resume_error(
        dir.path(),
        "digest.jsonl",
        Some(json!({CONTENT_FIELD: stored(json!([part]))})),
    );
    assert!(error.contains("sha256"), "{error}");
}

#[test]
fn a_turn_without_ordered_content_resumes_unchanged() {
    let dir = tempdir().unwrap();
    let legacy = json!({
        "attachments": [{
            "kind": "file",
            "mime_type": "text/plain",
            "path": "note.txt",
            "url": null,
            "file_id": null,
            "size_bytes": 24,
            "sha256": "a".repeat(64),
        }],
        "resource": {"kind": "prompt"},
    });
    let agent = resume_crafted(dir.path(), "legacy.jsonl", Some(legacy.clone())).unwrap();
    let turn = &agent.history()[0];
    assert_eq!(turn.content, "recorded turn");
    assert_eq!(turn.metadata.as_ref().unwrap(), &legacy);
    assert!(turn.metadata.as_ref().unwrap().get(CONTENT_FIELD).is_none());

    let dir = tempdir().unwrap();
    let agent = resume_crafted(dir.path(), "bare.jsonl", None).unwrap();
    assert!(agent.history()[0].metadata.is_none());
}

#[test]
fn a_turn_whose_content_is_invalid_is_refused_before_adoption() {
    let dir = tempdir().unwrap();
    let error = resume_error(
        dir.path(),
        "mid.jsonl",
        Some(json!({CONTENT_FIELD: stored(json!([{"type": "drawing"}]))})),
    );
    assert!(error.contains("stored content is invalid"), "{error}");
}

#[test]
fn tool_results_without_typed_content_keep_their_old_shape() {
    let legacy: ToolResult = serde_json::from_value(json!({
        "status": "success",
        "call_id": "call-1",
        "tool": "read_file",
        "output": {"contents": "ok"},
    }))
    .unwrap();
    let ToolResult::Success {
        content,
        output,
        call_id,
        tool,
    } = &legacy
    else {
        panic!("legacy JSON must stay a success result");
    };
    assert!(content.is_none());
    assert_eq!(output["contents"], "ok");
    assert_eq!(call_id, "call-1");
    assert_eq!(tool, "read_file");

    let encoded: Value = serde_json::from_str(&serde_json::to_string(&legacy).unwrap()).unwrap();
    assert!(encoded.get("content").is_none(), "{encoded}");

    // Typed content is the authority next to a compatibility record, and it
    // round trips without losing the output.
    let typed = ToolResult::Success {
        call_id: "call-2".into(),
        tool: "read_file".into(),
        output: json!({"note": "compatibility record"}),
        content: Some(vec![InputContentPart::Image {
            mime_type: "image/png".into(),
            source: InputContentSource::FileId {
                value: "file-7".into(),
            },
        }]),
    };
    let decoded: ToolResult =
        serde_json::from_str(&serde_json::to_string(&typed).unwrap()).unwrap();
    assert_eq!(decoded, typed);
}
