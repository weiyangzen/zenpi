//! Versioned, ordered content recorded in turn metadata, and the views
//! protocol encoders take over materialized request attachments.  Everything
//! here is a bounded reference; this module never reads paths or owns bytes.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::{
    AttachmentKind, BackendError, MAX_ATTACHMENT_FILE_ID_BYTES, MAX_ATTACHMENT_PATH_BYTES,
    RequestAttachment, unsupported_media_type, validate_attachment_mime, validate_provider_file_id,
    validate_relative_attachment_path,
};
use crate::core::Turn;
use crate::protocol::MAX_TEXT_BYTES;

/// The metadata field that makes a turn's ordered content authoritative.
/// `Turn.content` stays a compatibility display string, and a version this
/// build does not know is an error rather than "no content".
pub(crate) const STORED_CONTENT_FIELD: &str = "zenpi_content_v1";
pub(crate) const STORED_CONTENT_VERSION: u8 = 1;

/// Per-content ceiling.  A longer list cannot be represented without dropping
/// history, so the content is rejected instead of trimmed.
pub(crate) const MAX_CONTENT_PARTS: usize = 256;

const MAX_FILENAME_BYTES: usize = MAX_ATTACHMENT_PATH_BYTES;
const MAX_SCOPE_BYTES: usize = 2_048;
const SHA256_HEX_LEN: usize = 64;

/// Ordered content of one turn.  The field only exists once a host records it;
/// a turn written before it existed keeps its legacy text-only behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredContentV1 {
    pub version: u8,
    pub parts: Vec<StoredPart>,
}

/// One part of that content.  Text is inline; media is a reference, never
/// bytes and never a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum StoredPart {
    Text { text: String },
    Image { media: MediaRef },
    File { media: MediaRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MediaRef {
    /// Stable identity of the reference itself, so an encoder can correlate a
    /// stored part with the bytes the host materialized for this request.
    pub id: String,
    pub mime_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub source: MediaSource,
}

/// Exactly one source, by construction.  A workspace reference is replayable
/// after a restart, a provider file ID only under the account that issued it,
/// and an ephemeral handle never.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum MediaSource {
    WorkspaceFile {
        path: String,
        size_bytes: u64,
        sha256: String,
    },
    ProviderFile {
        file_id: String,
        media_scope: MediaScope,
    },
    Ephemeral {
        handle_id: String,
    },
}

/// Provider, credential and storage namespace a provider file belongs to.
/// A file ID is meaningless outside the account that issued it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MediaScope {
    pub provider: String,
    pub credential_id: String,
    pub identity_scope: String,
}

impl StoredPart {
    fn media(&self) -> Option<&MediaRef> {
        match self {
            Self::Text { .. } => None,
            Self::Image { media } | Self::File { media } => Some(media),
        }
    }
}

/// Validate content that is already in memory.  Callers that read content from
/// a journal should use [`parse_stored_content`], which applies the same rules
/// to untrusted JSON.
pub(crate) fn validate_stored_content(content: &StoredContentV1) -> Result<(), BackendError> {
    if content.version != STORED_CONTENT_VERSION {
        return Err(invalid(format!(
            "stored content version {} is not supported",
            content.version
        )));
    }
    if content.parts.len() > MAX_CONTENT_PARTS {
        return Err(invalid(format!(
            "stored content exceeds {MAX_CONTENT_PARTS} parts"
        )));
    }
    let mut text_bytes = 0_usize;
    for part in &content.parts {
        match part {
            StoredPart::Text { text } => {
                text_bytes = text_bytes.saturating_add(text.len());
                if text_bytes > MAX_TEXT_BYTES {
                    return Err(invalid(format!(
                        "stored content text exceeds {MAX_TEXT_BYTES} bytes"
                    )));
                }
            }
            StoredPart::Image { media } => validate_media(media, AttachmentKind::Image)?,
            StoredPart::File { media } => validate_media(media, AttachmentKind::File)?,
        }
    }
    Ok(())
}

/// Read the ordered content a turn records.
///
/// A metadata value that is not an object, or an object without the field, is
/// a turn written before ordered content existed: `Ok(None)`, unchanged
/// behaviour.  Once the field is present it is protocol authority, so a record
/// that does not validate is an error and never silently becomes empty content.
pub(crate) fn parse_stored_content(
    metadata: &Value,
) -> Result<Option<StoredContentV1>, BackendError> {
    let Some(value) = metadata.get(STORED_CONTENT_FIELD) else {
        return Ok(None);
    };
    let content = serde_json::from_value::<StoredContentV1>(value.clone())
        .map_err(|error| invalid(format!("stored content is invalid: {error}")))?;
    validate_stored_content(&content)?;
    Ok(Some(content))
}

/// Compare provider file references against the scope that is authorized now.
/// Restoring a file ID under another account is refused rather than hoped to
/// still resolve, and a user cannot self-certify a scope through input.
pub(crate) fn validate_stored_content_scope(
    content: &StoredContentV1,
    current: &MediaScope,
) -> Result<(), BackendError> {
    for part in &content.parts {
        let Some(MediaSource::ProviderFile { media_scope, .. }) =
            part.media().map(|media| &media.source)
        else {
            continue;
        };
        if media_scope != current {
            return Err(invalid(
                "stored provider file reference belongs to another media scope".into(),
            ));
        }
    }
    Ok(())
}

/// The ordered content to record for one user turn: the turn text first, then
/// the materialized attachments in exactly the order they were admitted.
///
/// Nothing here is secret.  A workspace reference keeps the relative path and
/// its verified hash, a provider file keeps the scope that issued it, and a URL
/// source keeps only an opaque handle: restart makes it unavailable instead of
/// replaying a signed URL out of the journal.  Bytes are never recorded.
pub(crate) fn stored_content(
    text: &str,
    attachments: &[RequestAttachment],
    scope: &MediaScope,
) -> Result<Option<StoredContentV1>, BackendError> {
    if text.is_empty() && attachments.is_empty() {
        return Ok(None);
    }
    let mut parts = Vec::with_capacity(attachments.len().saturating_add(1));
    if !text.is_empty() {
        parts.push(StoredPart::Text {
            text: text.to_owned(),
        });
    }
    for (index, attachment) in attachments.iter().enumerate() {
        parts.push(stored_part(attachment, scope, index)?);
    }
    let content = StoredContentV1 {
        version: STORED_CONTENT_VERSION,
        parts,
    };
    // A host that cannot record replayable content must not append a turn whose
    // recovery would be rejected later.
    validate_stored_content(&content)?;
    Ok(Some(content))
}

/// The metadata value recorded under [`STORED_CONTENT_FIELD`].
pub(crate) fn stored_content_metadata(
    text: &str,
    attachments: &[RequestAttachment],
    scope: &MediaScope,
) -> Result<Option<Value>, BackendError> {
    let Some(content) = stored_content(text, attachments, scope)? else {
        return Ok(None);
    };
    serde_json::to_value(content)
        .map(Some)
        .map_err(|error| invalid(format!("stored content cannot be recorded: {error}")))
}

fn stored_part(
    attachment: &RequestAttachment,
    scope: &MediaScope,
    index: usize,
) -> Result<StoredPart, BackendError> {
    let input = &attachment.input;
    // The journal identity of a reference: the same string an encoder can match
    // a materialized attachment against, and never a URL or a token.
    let id = input
        .path
        .clone()
        .or_else(|| input.file_id.clone())
        .unwrap_or_else(|| format!("{}-{index}", attachment.turn_id));
    let media = MediaRef {
        id: id.clone(),
        mime_type: input.mime_type.clone(),
        filename: attachment.filename.clone(),
        source: media_source(attachment, &id, scope)?,
    };
    Ok(match input.kind {
        AttachmentKind::Image => StoredPart::Image { media },
        AttachmentKind::File => StoredPart::File { media },
    })
}

fn media_source(
    attachment: &RequestAttachment,
    id: &str,
    scope: &MediaScope,
) -> Result<MediaSource, BackendError> {
    let input = &attachment.input;
    if let Some(path) = &input.path {
        // The path is only replayable because the host already materialized and
        // hashed it for this request; without that, it is not a reference yet.
        let (Some(size_bytes), Some(sha256)) = (attachment.size_bytes, attachment.sha256.clone())
        else {
            return Err(invalid("attachment source was not materialized".into()));
        };
        return Ok(MediaSource::WorkspaceFile {
            path: path.clone(),
            size_bytes,
            sha256,
        });
    }
    if let Some(file_id) = &input.file_id {
        return Ok(MediaSource::ProviderFile {
            file_id: file_id.clone(),
            media_scope: scope.clone(),
        });
    }
    if input.url.is_some() {
        // The URL is deliberately not recorded: signed parameters are secrets,
        // and this handle only resolves inside the process that created it.
        return Ok(MediaSource::Ephemeral {
            handle_id: id.to_owned(),
        });
    }
    Err(invalid("attachment source was not materialized".into()))
}

fn validate_media(media: &MediaRef, kind: AttachmentKind) -> Result<(), BackendError> {
    if media.id.trim().is_empty()
        || media.id.len() > MAX_ATTACHMENT_PATH_BYTES
        || media.id.chars().any(char::is_control)
    {
        return Err(invalid("stored media ID is invalid".into()));
    }
    // Unsupported media is refused before anything else, so bytes of an audio
    // or video file can never be presented as an image.
    if unsupported_media_type(&media.mime_type) {
        return Err(invalid("audio and video content are not supported".into()));
    }
    // The reference rules are the attachment rules: a MIME type, path or file
    // ID that is invalid on the way in cannot become valid on the way out.
    validate_attachment_mime(kind, &media.mime_type)?;
    if let Some(filename) = &media.filename
        && (filename.trim().is_empty()
            || filename.len() > MAX_FILENAME_BYTES
            || filename.chars().any(char::is_control))
    {
        return Err(invalid("stored media filename is invalid".into()));
    }
    match &media.source {
        MediaSource::WorkspaceFile { path, sha256, .. } => {
            validate_relative_attachment_path(path)?;
            if sha256.len() != SHA256_HEX_LEN
                || !sha256
                    .chars()
                    .all(|character| character.is_ascii_digit() || ('a'..='f').contains(&character))
            {
                return Err(invalid(
                    "stored workspace reference requires a lowercase sha256 digest".into(),
                ));
            }
        }
        MediaSource::ProviderFile {
            file_id,
            media_scope,
        } => {
            validate_provider_file_id(file_id)?;
            validate_scope(media_scope)?;
        }
        // An ephemeral handle never names a workspace path, so a restart leaves
        // it unavailable rather than pointing at some other file.
        MediaSource::Ephemeral { handle_id } => {
            if handle_id.trim().is_empty()
                || handle_id.len() > MAX_ATTACHMENT_FILE_ID_BYTES
                || handle_id.chars().any(char::is_control)
            {
                return Err(invalid("stored ephemeral handle is invalid".into()));
            }
        }
    }
    Ok(())
}

fn validate_scope(scope: &MediaScope) -> Result<(), BackendError> {
    // An anonymous or legacy connection carries no credential identifier, so an
    // empty one is meaningful; a provider or identity scope never is.
    if scope.provider.trim().is_empty() {
        return Err(invalid("stored media scope provider is invalid".into()));
    }
    if scope.identity_scope.trim().is_empty() {
        return Err(invalid("stored media scope identity is invalid".into()));
    }
    for value in [&scope.provider, &scope.credential_id, &scope.identity_scope] {
        if value.len() > MAX_SCOPE_BYTES || value.chars().any(char::is_control) {
            return Err(invalid("stored media scope is invalid".into()));
        }
    }
    Ok(())
}

fn invalid(message: String) -> BackendError {
    BackendError::Configuration(message)
}

pub(super) fn attachments_for_context<'a>(
    turns: &[Turn],
    attachments: &'a [RequestAttachment],
) -> Vec<&'a RequestAttachment> {
    attachments
        .iter()
        .filter(|attachment| turns.iter().any(|turn| turn.id == attachment.turn_id))
        .collect()
}

pub(super) fn attachment_source(attachment: &RequestAttachment) -> Result<String, BackendError> {
    if let Some(url) = &attachment.input.url {
        return Ok(url.clone());
    }
    if let Some(data) = &attachment.data {
        return Ok(data_url(&attachment.input.mime_type, data));
    }
    if let Some(file_id) = &attachment.input.file_id {
        return Ok(file_id.clone());
    }
    Err(BackendError::Configuration(
        "attachment source was not materialized".into(),
    ))
}

pub(super) fn data_url(mime_type: &str, bytes: &[u8]) -> String {
    use base64::Engine;

    format!(
        "data:{mime_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

pub(super) fn attachment_turn_matches(message: &Value, turn_id: &str) -> bool {
    message
        .get("_zenpi_turn_id")
        .and_then(Value::as_str)
        .is_none_or(|candidate| candidate == turn_id)
}

pub(super) fn strip_internal_turn_ids(items: &mut [Value]) {
    for item in items {
        if let Some(object) = item.as_object_mut() {
            object.remove("_zenpi_turn_id");
        }
    }
}
