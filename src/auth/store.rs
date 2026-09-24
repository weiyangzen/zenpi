//! Explicit-path credential storage. This module never discovers HOME or sends HTTP.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::{AllowedDestination, AuthIdentitySnapshot};

/// Root key owned by this store.  Legacy writers must never carry a copy of it
/// back into the document; the live value is always re-read under the lock.
pub(crate) const STORE_KEY: &str = "zenpi_auth_v1";
const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_ACCOUNTS: usize = 128;
const MAX_TOKEN_BYTES: usize = 16 * 1024;
const MAX_METADATA_BYTES: usize = 2048;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("credential store is unsupported on this platform")]
    Unsupported,
    #[error("credential store path or permissions are unsafe")]
    UnsafePath,
    #[error("credential store data is invalid: {0}")]
    InvalidData(&'static str),
    #[error("credential store version is unsupported")]
    UnsupportedVersion,
    #[error("credential was not found")]
    NotFound,
    #[error("credential revision changed")]
    Conflict,
    #[error("credential is not active")]
    Inactive,
    #[error("credential refresh identity changed")]
    IdentityMismatch,
    #[error("credential store operation was cancelled")]
    Cancelled,
    #[error("credential store lock deadline exceeded")]
    LockTimeout,
    #[error("credential store write or read failed")]
    StorageFailed,
    #[error("credential store commit result is uncertain; reload before proceeding")]
    CommitUncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    Active,
    RefreshInFlight,
    RefreshUncertain,
    LoginRequired,
    Revoked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiKey,
    Oauth,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialStatus {
    pub credential_id: String,
    pub provider: String,
    pub kind: CredentialKind,
    pub state: CredentialState,
    pub revision: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct CredentialMetadata {
    pub(crate) identity: AuthIdentitySnapshot,
    pub(crate) kind: CredentialKind,
    pub(crate) state: CredentialState,
    pub(crate) definition_version: u32,
    pub(crate) issuer: String,
    pub(crate) client_id: String,
    pub(crate) expires_at_ms: Option<u64>,
}

pub struct LockWait<'a> {
    pub deadline: Instant,
    pub cancelled: &'a dyn Fn() -> bool,
}

impl Default for LockWait<'static> {
    fn default() -> Self {
        Self {
            deadline: Instant::now() + Duration::from_secs(10),
            cancelled: &|| false,
        }
    }
}

#[derive(Clone)]
pub struct CredentialStore {
    path: PathBuf,
    #[cfg(test)]
    fault: Option<WriteFault>,
}

// Only the dedicated persistence path serializes these private DTOs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateCredential {
    kind: CredentialKind,
    provider: String,
    definition_version: u32,
    issuer: String,
    client_id: String,
    identity_generation: String,
    revision: u64,
    state: CredentialState,
    account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    #[serde(default)]
    allowed_destinations: Vec<AllowedDestination>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh: Option<RefreshMarker>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshMarker {
    attempt_id: String,
    started_at_ms: u64,
    identity_generation: String,
    previous_revision: u64,
}

pub(crate) struct PendingCredential {
    pub(crate) kind: CredentialKind,
    pub(crate) provider: String,
    pub(crate) definition_version: u32,
    pub(crate) issuer: String,
    pub(crate) client_id: String,
    pub(crate) account_id: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) allowed_destinations: Vec<AllowedDestination>,
    pub(crate) api_key: Option<String>,
    pub(crate) access_token: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_at_ms: Option<u64>,
}

// Runtime handles deliberately implement neither Serialize nor Debug. Only this
// module can construct one, after a successful durable transaction and readback.
pub(crate) struct CommittedCredential {
    credential_id: String,
    credential: PrivateCredential,
}

impl CommittedCredential {
    pub(crate) fn identity(&self) -> Result<AuthIdentitySnapshot, StoreError> {
        if self.credential.state != CredentialState::Active {
            return Err(StoreError::Inactive);
        }
        Ok(self.credential.identity(&self.credential_id))
    }

    pub(crate) fn metadata(&self) -> CredentialMetadata {
        self.credential.metadata(&self.credential_id)
    }

    pub(crate) fn with_request_secret<T>(
        &self,
        action: impl FnOnce(&str) -> T,
    ) -> Result<T, StoreError> {
        if self.credential.state != CredentialState::Active {
            return Err(StoreError::Inactive);
        }
        let secret = match self.credential.kind {
            CredentialKind::ApiKey => self.credential.api_key.as_deref(),
            CredentialKind::Oauth => self.credential.access_token.as_deref(),
        }
        .ok_or(StoreError::Inactive)?;
        Ok(action(secret))
    }
}

pub(crate) enum Replacement {
    Login(PendingCredential),
    AllowedDestinations(Vec<AllowedDestination>),
}

pub(crate) enum Mutation {
    Keep,
    Replace(Replacement),
    Revoke,
}

pub(crate) struct RefreshTokens {
    pub(crate) issuer: String,
    pub(crate) client_id: String,
    pub(crate) account_id: String,
    pub(crate) user_id: Option<String>,
    pub(crate) access_token: String,
    pub(crate) refresh_token: Option<String>,
    pub(crate) expires_at_ms: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct RefreshTicket {
    credential_id: String,
    revision: u64,
    attempt_id: String,
    identity_generation: String,
}

pub(crate) enum RefreshResolution {
    Tokens(RefreshTokens),
    Uncertain,
    LoginRequired,
    // Only use after the transport proves that no request was sent.
    NotSent,
}

pub(crate) struct RefreshGuard {
    _lock: File,
    store_path: PathBuf,
    credential_id: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Namespace {
    version: u32,
    revision: u64,
    accounts: BTreeMap<String, PrivateCredential>,
}

struct Document {
    legacy: Map<String, Value>,
    namespace: Namespace,
}

impl CredentialStore {
    #[cfg(test)]
    pub(super) fn with_uncertain_commit_for_test(&self) -> Self {
        Self {
            fault: Some(WriteFault::AfterRename),
            ..self.clone()
        }
    }

    #[cfg(test)]
    pub(super) fn with_uncertain_revision_for_test(&self, revision: u64) -> Self {
        Self {
            fault: Some(WriteFault::ReadBackAtRevision(revision)),
            ..self.clone()
        }
    }

    pub fn new(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        if !path.is_absolute() || path.file_name().is_none() {
            return Err(StoreError::UnsafePath);
        }
        Ok(Self {
            path,
            #[cfg(test)]
            fault: None,
        })
    }

    /// Identifier for a credential that does not exist yet.  Random rather than
    /// derived from a profile alias: an alias is a user-chosen name for a
    /// connection, while this ID is what a grant and every bound profile refer
    /// to, and several profiles may share one credential.
    pub fn new_credential_id() -> Result<String, StoreError> {
        Ok(format!("cred_{}", random_id()?))
    }

    pub fn list_status(&self) -> Result<Vec<CredentialStatus>, StoreError> {
        Ok(self
            .read_document()?
            .namespace
            .accounts
            .into_iter()
            .map(|(id, value)| CredentialStatus {
                credential_id: id,
                provider: value.provider,
                kind: value.kind,
                state: value.state,
                revision: value.revision,
                expires_at_ms: value.expires_at_ms,
            })
            .collect())
    }

    pub fn identity(&self, id: &str) -> Result<AuthIdentitySnapshot, StoreError> {
        let value = self.read(id)?;
        if value.state != CredentialState::Active {
            return Err(StoreError::Inactive);
        }
        Ok(value.identity(id))
    }

    /// Mutate only legacy root fields under the same stable lock as credentials.
    /// A whole-document caller must remove or reject its namespace before entry.
    pub(crate) fn update_legacy<T>(
        &self,
        wait: &LockWait<'_>,
        update: impl FnOnce(&mut Map<String, Value>) -> Result<T, StoreError>,
    ) -> Result<(T, bool), StoreError> {
        self.transaction(wait, |document| {
            let before = document.legacy.clone();
            let result = update(&mut document.legacy)?;
            if document.legacy.contains_key(STORE_KEY) {
                return Err(StoreError::InvalidData("reserved credential namespace"));
            }
            let changed = document.legacy != before;
            if changed {
                // Apply the reader's size/string/depth constraints before any
                // write, so caller-supplied JSON cannot poison the shared store.
                let mut bytes = Vec::new();
                serde_json::to_writer(&mut BoundedWriter(&mut bytes), &document.legacy)
                    .map_err(|_| StoreError::InvalidData("file limit exceeded"))?;
                check_json_string_bounds(&bytes)?;
                serde_json::from_slice::<StrictValue>(&bytes)
                    .map_err(|_| StoreError::InvalidData("invalid legacy JSON"))?;
            }
            Ok(((result, changed), changed))
        })
    }

    fn read(&self, id: &str) -> Result<PrivateCredential, StoreError> {
        self.read_with_wait(id, &LockWait::default())
    }

    fn read_with_wait(
        &self,
        id: &str,
        wait: &LockWait<'_>,
    ) -> Result<PrivateCredential, StoreError> {
        validate_id(id)?;
        self.read_document_with_wait(wait)?
            .namespace
            .accounts
            .remove(id)
            .ok_or(StoreError::NotFound)
    }

    pub(crate) fn metadata(
        &self,
        id: &str,
        wait: &LockWait<'_>,
    ) -> Result<CredentialMetadata, StoreError> {
        Ok(self.read_with_wait(id, wait)?.metadata(id))
    }

    pub(crate) fn read_committed(
        &self,
        id: &str,
        expected_revision: u64,
        wait: &LockWait<'_>,
    ) -> Result<CommittedCredential, StoreError> {
        let credential = self.read_with_wait(id, wait)?;
        if credential.revision != expected_revision {
            return Err(StoreError::Conflict);
        }
        if credential.state != CredentialState::Active {
            return Err(StoreError::Inactive);
        }
        Ok(CommittedCredential {
            credential_id: id.to_owned(),
            credential,
        })
    }

    // The global file lock is released by read_with_wait before action runs.
    // The caller must retain the account lock while using this private borrow.
    pub(crate) fn with_refresh_token<T>(
        &self,
        guard: &RefreshGuard,
        ticket: &RefreshTicket,
        wait: &LockWait<'_>,
        action: impl FnOnce(&str) -> T,
    ) -> Result<T, StoreError> {
        self.check_guard(guard)?;
        if guard.credential_id != ticket.credential_id {
            return Err(StoreError::Conflict);
        }
        let credential = self.read_with_wait(&ticket.credential_id, wait)?;
        if credential.state != CredentialState::RefreshInFlight
            || credential.revision != ticket.revision
            || credential.identity_generation != ticket.identity_generation
            || credential
                .refresh
                .as_ref()
                .map(|marker| marker.attempt_id.as_str())
                != Some(ticket.attempt_id.as_str())
        {
            return Err(StoreError::Conflict);
        }
        Ok(action(
            credential
                .refresh_token
                .as_deref()
                .ok_or(StoreError::Inactive)?,
        ))
    }

    pub(crate) fn modify(
        &self,
        id: &str,
        expected_revision: Option<u64>,
        mutation: Mutation,
        wait: &LockWait<'_>,
    ) -> Result<CommittedCredential, StoreError> {
        validate_id(id)?;
        self.transaction(wait, |doc| {
            let current = doc.namespace.accounts.get(id);
            if current.map(|v| v.revision) != expected_revision {
                return Err(StoreError::Conflict);
            }
            if matches!(mutation, Mutation::Keep) {
                return current
                    .cloned()
                    .map(|v| (v, false))
                    .ok_or(StoreError::NotFound);
            }
            let revision = next_revision(current.map_or(0, |v| v.revision))?;
            let value = match mutation {
                Mutation::Keep => unreachable!(),
                Mutation::Replace(Replacement::Login(pending)) => PrivateCredential {
                    kind: pending.kind,
                    provider: pending.provider,
                    definition_version: pending.definition_version,
                    issuer: pending.issuer,
                    client_id: pending.client_id,
                    identity_generation: random_id()?,
                    revision,
                    state: CredentialState::Active,
                    account_id: pending.account_id,
                    user_id: pending.user_id,
                    allowed_destinations: pending.allowed_destinations,
                    api_key: pending.api_key,
                    access_token: pending.access_token,
                    refresh_token: pending.refresh_token,
                    expires_at_ms: pending.expires_at_ms,
                    refresh: None,
                },
                Mutation::Replace(Replacement::AllowedDestinations(destinations)) => {
                    let mut value = current.cloned().ok_or(StoreError::NotFound)?;
                    if value.state != CredentialState::Active {
                        return Err(StoreError::Inactive);
                    }
                    value.allowed_destinations = destinations;
                    value.revision = revision;
                    value
                }
                Mutation::Revoke => {
                    let mut value = current.cloned().ok_or(StoreError::NotFound)?;
                    value.clear_secrets();
                    value.state = CredentialState::Revoked;
                    value.revision = revision;
                    value
                }
            };
            value.validate()?;
            if current.is_none() && doc.namespace.accounts.len() >= MAX_ACCOUNTS {
                return Err(StoreError::InvalidData("account limit exceeded"));
            }
            doc.namespace.accounts.insert(id.to_owned(), value.clone());
            Ok((value, true))
        })
        .map(|credential| CommittedCredential {
            credential_id: id.to_owned(),
            credential,
        })
    }

    pub(crate) fn lock_refresh(
        &self,
        id: &str,
        wait: &LockWait<'_>,
    ) -> Result<RefreshGuard, StoreError> {
        validate_id(id)?;
        check_wait(wait)?;
        let directory = self.directory(true)?;
        let digest = format!("{:x}", Sha256::digest(id.as_bytes()));
        let name = format!("{}.refresh.{digest}.lock", self.file_name()?);
        let lock = directory
            .lock(&name, true, wait)?
            .ok_or(StoreError::StorageFailed)?;
        Ok(RefreshGuard {
            _lock: lock,
            store_path: self.path.clone(),
            credential_id: id.to_owned(),
        })
    }

    pub(crate) fn begin_refresh(
        &self,
        guard: &RefreshGuard,
        expected_revision: u64,
        started_at_ms: u64,
        wait: &LockWait<'_>,
    ) -> Result<RefreshTicket, StoreError> {
        self.check_guard(guard)?;
        self.transaction(wait, |doc| {
            let value = doc
                .namespace
                .accounts
                .get_mut(&guard.credential_id)
                .ok_or(StoreError::NotFound)?;
            if value.revision != expected_revision {
                return Err(StoreError::Conflict);
            }
            if value.state != CredentialState::Active || value.kind != CredentialKind::Oauth {
                return Err(StoreError::Inactive);
            }
            let marker = RefreshMarker {
                attempt_id: random_id()?,
                started_at_ms,
                identity_generation: value.identity_generation.clone(),
                previous_revision: value.revision,
            };
            value.revision = next_revision(value.revision)?;
            value.state = CredentialState::RefreshInFlight;
            let ticket = RefreshTicket {
                credential_id: guard.credential_id.clone(),
                revision: value.revision,
                attempt_id: marker.attempt_id.clone(),
                identity_generation: marker.identity_generation.clone(),
            };
            value.refresh = Some(marker);
            Ok((ticket, true))
        })
    }

    pub(crate) fn finish_refresh(
        &self,
        guard: &RefreshGuard,
        ticket: &RefreshTicket,
        resolution: RefreshResolution,
        wait: &LockWait<'_>,
    ) -> Result<CommittedCredential, StoreError> {
        self.check_guard(guard)?;
        if ticket.credential_id != guard.credential_id {
            return Err(StoreError::Conflict);
        }
        self.transaction(wait, |doc| {
            let value = doc
                .namespace
                .accounts
                .get_mut(&ticket.credential_id)
                .ok_or(StoreError::NotFound)?;
            if value.state != CredentialState::RefreshInFlight
                || value.revision != ticket.revision
                || value.identity_generation != ticket.identity_generation
                || value.refresh.as_ref().map(|v| v.attempt_id.as_str())
                    != Some(ticket.attempt_id.as_str())
            {
                return Err(StoreError::Conflict);
            }
            match resolution {
                RefreshResolution::Tokens(tokens) => {
                    if value.issuer != tokens.issuer
                        || value.client_id != tokens.client_id
                        || value.account_id.as_deref() != Some(tokens.account_id.as_str())
                        || (value.user_id.is_some() && value.user_id != tokens.user_id)
                    {
                        return Err(StoreError::IdentityMismatch);
                    }
                    value.access_token = Some(tokens.access_token);
                    if let Some(refresh_token) = tokens.refresh_token {
                        value.refresh_token = Some(refresh_token);
                    }
                    value.expires_at_ms = Some(tokens.expires_at_ms);
                    value.state = CredentialState::Active;
                }
                RefreshResolution::Uncertain => value.state = CredentialState::RefreshUncertain,
                RefreshResolution::LoginRequired => {
                    value.clear_secrets();
                    value.state = CredentialState::LoginRequired;
                }
                RefreshResolution::NotSent => value.state = CredentialState::Active,
            }
            value.refresh = None;
            value.revision = next_revision(value.revision)?;
            value.validate()?;
            Ok((value.clone(), true))
        })
        .map(|credential| CommittedCredential {
            credential_id: ticket.credential_id.clone(),
            credential,
        })
    }

    /// Acquiring the account lock proves that an abandoned marker has no live owner.
    pub(crate) fn recover_abandoned_refresh(
        &self,
        guard: &RefreshGuard,
        wait: &LockWait<'_>,
    ) -> Result<(), StoreError> {
        self.check_guard(guard)?;
        self.transaction(wait, |doc| {
            let value = doc
                .namespace
                .accounts
                .get_mut(&guard.credential_id)
                .ok_or(StoreError::NotFound)?;
            if value.state != CredentialState::RefreshInFlight {
                return Ok(((), false));
            }
            value.state = CredentialState::RefreshUncertain;
            value.refresh = None;
            value.revision = next_revision(value.revision)?;
            Ok(((), true))
        })
    }

    fn check_guard(&self, guard: &RefreshGuard) -> Result<(), StoreError> {
        if guard.store_path == self.path {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        }
    }

    fn file_name(&self) -> Result<&str, StoreError> {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| name.len() <= 128)
            .ok_or(StoreError::UnsafePath)
    }

    fn directory(&self, create: bool) -> Result<secure_fs::Directory, StoreError> {
        secure_fs::Directory::open(self.path.parent().ok_or(StoreError::UnsafePath)?, create)
    }

    fn read_document(&self) -> Result<Document, StoreError> {
        self.read_document_with_wait(&LockWait::default())
    }

    fn read_document_with_wait(&self, wait: &LockWait<'_>) -> Result<Document, StoreError> {
        check_wait(wait)?;
        let directory = match self.directory(false) {
            Ok(directory) => directory,
            Err(StoreError::NotFound) => return Ok(Document::empty()),
            Err(error) => return Err(error),
        };
        let _lock = directory.lock(&format!("{}.lock", self.file_name()?), false, wait)?;
        read_document(&directory, self.file_name()?)
    }

    fn transaction<T>(
        &self,
        wait: &LockWait<'_>,
        change: impl FnOnce(&mut Document) -> Result<(T, bool), StoreError>,
    ) -> Result<T, StoreError> {
        check_wait(wait)?;
        let directory = self.directory(true)?;
        let _lock = directory.lock(&format!("{}.lock", self.file_name()?), true, wait)?;
        let mut document = read_document(&directory, self.file_name()?)?;
        let (result, changed) = change(&mut document)?;
        if !changed {
            return Ok(result);
        }
        document.namespace.revision = next_revision(document.namespace.revision)?;
        let bytes = document.bytes()?;
        check_wait(wait)?;
        self.commit(&directory, &bytes)?;
        #[cfg(test)]
        if self.fault == Some(WriteFault::ReadBack)
            || self.fault == Some(WriteFault::ReadBackAtRevision(document.namespace.revision))
        {
            return Err(StoreError::CommitUncertain);
        }
        let actual = read_document(&directory, self.file_name()?)
            .map_err(|_| StoreError::CommitUncertain)?;
        #[cfg(test)]
        let actual = if self.fault == Some(WriteFault::ReadBackMismatch) {
            let mut actual = actual;
            actual
                .legacy
                .insert("readback_fault".into(), Value::Bool(true));
            actual
        } else {
            actual
        };
        if actual.bytes().map_err(|_| StoreError::CommitUncertain)? != bytes {
            return Err(StoreError::CommitUncertain);
        }
        Ok(result)
    }

    fn commit(&self, directory: &secure_fs::Directory, bytes: &[u8]) -> Result<(), StoreError> {
        let temp_name = format!(".{}.{}.tmp", self.file_name()?, random_id()?);
        let mut file = directory.create_new(&temp_name)?;
        let result = (|| {
            #[cfg(test)]
            if self.fault == Some(WriteFault::BeforeWrite) {
                return Err(StoreError::StorageFailed);
            }
            file.write_all(bytes)
                .map_err(|_| StoreError::StorageFailed)?;
            #[cfg(test)]
            if self.fault == Some(WriteFault::FileSync) {
                return Err(StoreError::StorageFailed);
            }
            file.sync_all().map_err(|_| StoreError::StorageFailed)?;
            #[cfg(test)]
            if self.fault == Some(WriteFault::BeforeRename) {
                return Err(StoreError::StorageFailed);
            }
            let rename_result = directory.replace(&temp_name, self.file_name()?);
            #[cfg(test)]
            let rename_result =
                if self.fault == Some(WriteFault::RenameResultUncertain) && rename_result.is_ok() {
                    Err(StoreError::CommitUncertain)
                } else {
                    rename_result
                };
            rename_result?;
            #[cfg(test)]
            if self.fault == Some(WriteFault::AfterRename) {
                return Err(StoreError::CommitUncertain);
            }
            directory.sync().map_err(|_| StoreError::CommitUncertain)
        })();
        if result.is_err() {
            let _ = directory.remove(&temp_name);
        }
        result
    }
}

impl PrivateCredential {
    fn metadata(&self, id: &str) -> CredentialMetadata {
        CredentialMetadata {
            identity: self.identity(id),
            kind: self.kind,
            state: self.state,
            definition_version: self.definition_version,
            issuer: self.issuer.clone(),
            client_id: self.client_id.clone(),
            expires_at_ms: self.expires_at_ms,
        }
    }

    fn identity(&self, id: &str) -> AuthIdentitySnapshot {
        AuthIdentitySnapshot {
            provider: self.provider.clone(),
            credential_id: id.to_owned(),
            account_id: self.account_id.clone(),
            identity_generation: self.identity_generation.clone(),
            credential_revision: self.revision,
            allowed_destinations: self.allowed_destinations.clone(),
        }
    }

    fn clear_secrets(&mut self) {
        self.api_key = None;
        self.access_token = None;
        self.refresh_token = None;
        self.expires_at_ms = None;
        self.refresh = None;
    }

    fn validate(&self) -> Result<(), StoreError> {
        for field in [&self.provider, &self.identity_generation] {
            validate_text(field, MAX_METADATA_BYTES)?;
        }
        for field in [&self.issuer, &self.client_id] {
            if !field.is_empty() {
                validate_text(field, MAX_METADATA_BYTES)?;
            }
        }
        for value in [&self.account_id, &self.user_id].into_iter().flatten() {
            validate_text(value, MAX_METADATA_BYTES)?;
        }
        if self.revision == 0 || self.definition_version == 0 {
            return Err(StoreError::InvalidData("invalid revision"));
        }
        if self.allowed_destinations.len() > 32 {
            return Err(StoreError::InvalidData("destination limit exceeded"));
        }
        for destination in &self.allowed_destinations {
            validate_text(&destination.origin, MAX_METADATA_BYTES)?;
            validate_text(&destination.path_prefix, MAX_METADATA_BYTES)?;
            if destination.protocols.is_empty()
                || destination.headers.is_empty()
                || destination.protocols.len() > 16
                || destination.headers.len() > 32
            {
                return Err(StoreError::InvalidData("invalid destination scope"));
            }
            for field in destination
                .protocols
                .iter()
                .chain(destination.headers.iter())
            {
                validate_text(field, 128)?;
            }
        }
        for token in [&self.api_key, &self.access_token, &self.refresh_token]
            .into_iter()
            .flatten()
        {
            validate_text(token, MAX_TOKEN_BYTES)?;
        }
        let inactive = matches!(
            self.state,
            CredentialState::Revoked | CredentialState::LoginRequired
        );
        if inactive {
            if self.api_key.is_some()
                || self.access_token.is_some()
                || self.refresh_token.is_some()
                || self.expires_at_ms.is_some()
                || self.refresh.is_some()
            {
                return Err(StoreError::InvalidData(
                    "inactive credential contains secrets",
                ));
            }
        } else {
            match self.kind {
                CredentialKind::ApiKey
                    if self.api_key.is_some()
                        && self.access_token.is_none()
                        && self.refresh_token.is_none()
                        && self.expires_at_ms.is_none()
                        && self.state == CredentialState::Active => {}
                CredentialKind::Oauth
                    if self.api_key.is_none()
                        && self.access_token.is_some()
                        && self.refresh_token.is_some()
                        && self.expires_at_ms.is_some()
                        && self.account_id.is_some()
                        && !self.issuer.is_empty()
                        && !self.client_id.is_empty() => {}
                _ => {
                    return Err(StoreError::InvalidData(
                        "credential material does not match kind",
                    ));
                }
            }
        }
        if (self.state == CredentialState::RefreshInFlight) != self.refresh.is_some() {
            return Err(StoreError::InvalidData("invalid refresh marker"));
        }
        if let Some(marker) = &self.refresh {
            validate_id(&marker.attempt_id)?;
            if marker.identity_generation != self.identity_generation
                || marker.previous_revision.checked_add(1) != Some(self.revision)
            {
                return Err(StoreError::InvalidData("refresh marker revision mismatch"));
            }
        }
        Ok(())
    }
}

impl Document {
    fn empty() -> Self {
        Self {
            legacy: Map::new(),
            namespace: Namespace {
                version: 1,
                ..Namespace::default()
            },
        }
    }

    fn bytes(&self) -> Result<Vec<u8>, StoreError> {
        let mut root = self.legacy.clone();
        root.insert(
            STORE_KEY.to_owned(),
            serde_json::to_value(&self.namespace).map_err(|_| StoreError::StorageFailed)?,
        );
        let mut bytes = Vec::new();
        serde_json::to_writer(&mut BoundedWriter(&mut bytes), &root)
            .map_err(|_| StoreError::InvalidData("file limit exceeded"))?;
        Ok(bytes)
    }
}

struct BoundedWriter<'a>(&'a mut Vec<u8>);
impl Write for BoundedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_FILE_BYTES {
            return Err(std::io::Error::other("file limit exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn read_document(directory: &secure_fs::Directory, name: &str) -> Result<Document, StoreError> {
    let Some(file) = directory.open_file(name)? else {
        return Ok(Document::empty());
    };
    if file
        .metadata()
        .map_err(|_| StoreError::StorageFailed)?
        .len()
        > MAX_FILE_BYTES as u64
    {
        return Err(StoreError::InvalidData("file limit exceeded"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StoreError::StorageFailed)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(StoreError::InvalidData("file limit exceeded"));
    }
    check_json_string_bounds(&bytes)?;
    let StrictValue(value) = serde_json::from_slice(&bytes)
        .map_err(|_| StoreError::InvalidData("invalid JSON or duplicate key"))?;
    let Value::Object(mut legacy) = value else {
        return Err(StoreError::InvalidData("root is not an object"));
    };
    let namespace = match legacy.remove(STORE_KEY) {
        Some(value) => {
            if value.get("version").and_then(Value::as_u64) != Some(1) {
                return Err(StoreError::UnsupportedVersion);
            }
            let namespace: Namespace = serde_json::from_value(value)
                .map_err(|_| StoreError::InvalidData("invalid credential schema"))?;
            if namespace.accounts.len() > MAX_ACCOUNTS {
                return Err(StoreError::InvalidData("account limit exceeded"));
            }
            for (id, credential) in &namespace.accounts {
                validate_id(id)?;
                credential.validate()?;
            }
            namespace
        }
        None => Namespace {
            version: 1,
            ..Namespace::default()
        },
    };
    Ok(Document { legacy, namespace })
}

// serde borrows unescaped strings, but its escaped-string scratch buffer can grow
// before a visitor sees the value. Bound decoded string sizes before that parse.
fn check_json_string_bounds(bytes: &[u8]) -> Result<(), StoreError> {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'"' {
            cursor += 1;
            continue;
        }
        cursor += 1;
        let mut decoded_len = 0;
        while cursor < bytes.len() && bytes[cursor] != b'"' {
            let byte = bytes[cursor];
            cursor += 1;
            if byte == b'\\' {
                let escaped = *bytes
                    .get(cursor)
                    .ok_or(StoreError::InvalidData("invalid JSON string"))?;
                cursor += 1;
                if escaped == b'u' {
                    let digits = bytes
                        .get(cursor..cursor + 4)
                        .ok_or(StoreError::InvalidData("invalid JSON escape"))?;
                    let hex = std::str::from_utf8(digits)
                        .map_err(|_| StoreError::InvalidData("invalid JSON escape"))?;
                    let value = u16::from_str_radix(hex, 16)
                        .map_err(|_| StoreError::InvalidData("invalid JSON escape"))?;
                    cursor += 4;
                    // Counting each surrogate as two bytes gives the pair its
                    // exact UTF-8 size; serde rejects unpaired surrogates.
                    decoded_len += match value {
                        0..=0x7f => 1,
                        0x80..=0x7ff | 0xd800..=0xdfff => 2,
                        _ => 3,
                    };
                } else {
                    decoded_len += 1;
                }
            } else {
                decoded_len += 1;
            }
            if decoded_len > MAX_TOKEN_BYTES {
                return Err(StoreError::InvalidData("string limit exceeded"));
            }
        }
        cursor += 1;
    }
    Ok(())
}

// Decode recursively instead of allowing serde_json::Value's last-key-wins behavior.
struct StrictValue(Value);
#[derive(Clone, Copy)]
enum JsonScope {
    Root,
    Namespace,
    Accounts,
    Other,
}
struct StrictSeed(JsonScope);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        StrictSeed(JsonScope::Root).deserialize(deserializer)
    }
}

impl<'de> DeserializeSeed<'de> for StrictSeed {
    type Value = StrictValue;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        struct StrictVisitor(JsonScope);
        impl<'de> Visitor<'de> for StrictVisitor {
            type Value = StrictValue;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("bounded JSON without duplicate keys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut object = Map::new();
                while let Some(key) = access.next_key::<String>()? {
                    if key.len() > MAX_METADATA_BYTES || object.contains_key(&key) {
                        return Err(serde::de::Error::custom("invalid or duplicate key"));
                    }
                    // Reject the 129th account before deserializing its value.
                    if matches!(self.0, JsonScope::Accounts) && object.len() >= MAX_ACCOUNTS {
                        return Err(serde::de::Error::custom("account limit exceeded"));
                    }
                    let scope = match (self.0, key.as_str()) {
                        (JsonScope::Root, STORE_KEY) => JsonScope::Namespace,
                        (JsonScope::Namespace, "accounts") => JsonScope::Accounts,
                        _ => JsonScope::Other,
                    };
                    object.insert(key, access.next_value_seed(StrictSeed(scope))?.0);
                }
                Ok(StrictValue(Value::Object(object)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = access.next_element_seed(StrictSeed(JsonScope::Other))? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                if value.len() > MAX_TOKEN_BYTES {
                    return Err(E::custom("string limit exceeded"));
                }
                Ok(StrictValue(Value::String(value.to_owned())))
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(value)))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(value.into())))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(value.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| StrictValue(Value::Number(number)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
        }
        deserializer.deserialize_any(StrictVisitor(self.0))
    }
}

fn validate_id(value: &str) -> Result<(), StoreError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        Err(StoreError::InvalidData("invalid credential identifier"))
    } else {
        Ok(())
    }
}

fn validate_text(value: &str, max: usize) -> Result<(), StoreError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(StoreError::InvalidData("invalid credential field"))
    } else {
        Ok(())
    }
}

fn next_revision(value: u64) -> Result<u64, StoreError> {
    value
        .checked_add(1)
        .ok_or(StoreError::InvalidData("revision exhausted"))
}

fn random_id() -> Result<String, StoreError> {
    let mut bytes = [0_u8; 24];
    getrandom::getrandom(&mut bytes).map_err(|_| StoreError::StorageFailed)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn check_wait(wait: &LockWait<'_>) -> Result<(), StoreError> {
    if (wait.cancelled)() {
        Err(StoreError::Cancelled)
    } else if Instant::now() >= wait.deadline {
        Err(StoreError::LockTimeout)
    } else {
        Ok(())
    }
}

#[cfg(unix)]
mod secure_fs {
    use super::*;
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;

    pub(super) struct Directory(File);

    fn name(value: &OsStr) -> Result<CString, StoreError> {
        CString::new(value.as_bytes()).map_err(|_| StoreError::UnsafePath)
    }
    fn open_at(
        parent: &File,
        name: &CString,
        flags: i32,
        mode: libc::mode_t,
    ) -> Result<File, StoreError> {
        // The directory descriptor pins the parent; O_NOFOLLOW rejects final-component links.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                mode as libc::c_uint,
            )
        };
        if fd >= 0 {
            return Ok(unsafe { File::from_raw_fd(fd) });
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ENOENT) => Err(StoreError::NotFound),
            Some(libc::ELOOP | libc::ENOTDIR) => Err(StoreError::UnsafePath),
            _ => Err(StoreError::StorageFailed),
        }
    }

    fn validate_file(file: &File) -> Result<(), StoreError> {
        let metadata = file.metadata().map_err(|_| StoreError::StorageFailed)?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(StoreError::UnsafePath);
        }
        Ok(())
    }

    impl Directory {
        pub(super) fn open(path: &Path, create: bool) -> Result<Self, StoreError> {
            let components: Vec<_> = path.components().collect();
            if components.len() < 2
                || components.first() != Some(&Component::RootDir)
                || components
                    .iter()
                    .skip(1)
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(StoreError::UnsafePath);
            }
            let mut directory = File::open("/").map_err(|_| StoreError::StorageFailed)?;
            for (index, component) in components.iter().enumerate().skip(1) {
                let Component::Normal(component) = component else {
                    return Err(StoreError::UnsafePath);
                };
                let part = name(component)?;
                let final_component = index == components.len() - 1;
                let next = match open_at(&directory, &part, libc::O_RDONLY | libc::O_DIRECTORY, 0) {
                    Err(StoreError::NotFound) if create && final_component => {
                        let result =
                            unsafe { libc::mkdirat(directory.as_raw_fd(), part.as_ptr(), 0o700) };
                        if result < 0
                            && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
                        {
                            return Err(StoreError::StorageFailed);
                        }
                        directory
                            .sync_all()
                            .map_err(|_| StoreError::StorageFailed)?;
                        open_at(&directory, &part, libc::O_RDONLY | libc::O_DIRECTORY, 0)?
                    }
                    result => result?,
                };
                let metadata = next.metadata().map_err(|_| StoreError::StorageFailed)?;
                let uid = unsafe { libc::geteuid() };
                let trusted_owner = metadata.uid() == uid || metadata.uid() == 0;
                let trusted_sticky =
                    metadata.uid() == 0 && metadata.mode() & u32::from(libc::S_ISVTX) != 0;
                if !metadata.is_dir()
                    || !trusted_owner
                    || (metadata.mode() & 0o022 != 0 && !trusted_sticky)
                    || (final_component
                        && (metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700))
                {
                    return Err(StoreError::UnsafePath);
                }
                directory = next;
            }
            Ok(Self(directory))
        }

        pub(super) fn open_file(&self, value: &str) -> Result<Option<File>, StoreError> {
            match open_at(
                &self.0,
                &name(OsStr::new(value))?,
                libc::O_RDONLY | libc::O_NONBLOCK,
                0,
            ) {
                Ok(file) => {
                    validate_file(&file)?;
                    Ok(Some(file))
                }
                Err(StoreError::NotFound) => Ok(None),
                Err(error) => Err(error),
            }
        }

        pub(super) fn create_new(&self, value: &str) -> Result<File, StoreError> {
            let file = open_at(
                &self.0,
                &name(OsStr::new(value))?,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NONBLOCK,
                0o600,
            )?;
            validate_file(&file)?;
            Ok(file)
        }

        pub(super) fn lock(
            &self,
            value: &str,
            create: bool,
            wait: &LockWait<'_>,
        ) -> Result<Option<File>, StoreError> {
            let flags = libc::O_RDWR | libc::O_NONBLOCK | if create { libc::O_CREAT } else { 0 };
            let file = loop {
                check_wait(wait)?;
                match open_at(&self.0, &name(OsStr::new(value))?, flags, 0o600) {
                    Ok(file) => break file,
                    Err(StoreError::NotFound) if !create => return Ok(None),
                    // Concurrent first creation can transiently report ENOENT.
                    // Reopen the same stable inode, never truncate or unlink it.
                    Err(StoreError::NotFound) => std::thread::sleep(
                        Duration::from_millis(5)
                            .min(wait.deadline.saturating_duration_since(Instant::now())),
                    ),
                    Err(error) => return Err(error),
                }
            };
            validate_file(&file)?;
            loop {
                check_wait(wait)?;
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    return Ok(Some(file));
                }
                let error = std::io::Error::last_os_error().raw_os_error();
                if error != Some(libc::EWOULDBLOCK) && error != Some(libc::EINTR) {
                    return Err(StoreError::StorageFailed);
                }
                std::thread::sleep(
                    Duration::from_millis(5)
                        .min(wait.deadline.saturating_duration_since(Instant::now())),
                );
            }
        }

        pub(super) fn replace(&self, from: &str, to: &str) -> Result<(), StoreError> {
            // Validate the existing destination without following it before replacement.
            let _existing = self.open_file(to)?;
            let from = name(OsStr::new(from))?;
            let to = name(OsStr::new(to))?;
            let result = unsafe {
                libc::renameat(
                    self.0.as_raw_fd(),
                    from.as_ptr(),
                    self.0.as_raw_fd(),
                    to.as_ptr(),
                )
            };
            // Some filesystems can report a failed rename after applying it.
            // Once rename was attempted, callers must reload, never blindly retry.
            if result == 0 {
                Ok(())
            } else {
                Err(StoreError::CommitUncertain)
            }
        }

        pub(super) fn remove(&self, value: &str) -> Result<(), StoreError> {
            let value = name(OsStr::new(value))?;
            if unsafe { libc::unlinkat(self.0.as_raw_fd(), value.as_ptr(), 0) } == 0 {
                Ok(())
            } else {
                Err(StoreError::StorageFailed)
            }
        }

        pub(super) fn sync(&self) -> Result<(), StoreError> {
            self.0.sync_all().map_err(|_| StoreError::StorageFailed)
        }
    }
}

#[cfg(not(unix))]
mod secure_fs {
    use super::*;
    pub(super) struct Directory;
    impl Directory {
        pub(super) fn open(_: &Path, _: bool) -> Result<Self, StoreError> {
            Err(StoreError::Unsupported)
        }
        pub(super) fn open_file(&self, _: &str) -> Result<Option<File>, StoreError> {
            Err(StoreError::Unsupported)
        }
        pub(super) fn create_new(&self, _: &str) -> Result<File, StoreError> {
            Err(StoreError::Unsupported)
        }
        pub(super) fn lock(
            &self,
            _: &str,
            _: bool,
            _: &LockWait<'_>,
        ) -> Result<Option<File>, StoreError> {
            Err(StoreError::Unsupported)
        }
        pub(super) fn replace(&self, _: &str, _: &str) -> Result<(), StoreError> {
            Err(StoreError::Unsupported)
        }
        pub(super) fn remove(&self, _: &str) -> Result<(), StoreError> {
            Err(StoreError::Unsupported)
        }
        pub(super) fn sync(&self) -> Result<(), StoreError> {
            Err(StoreError::Unsupported)
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteFault {
    BeforeWrite,
    FileSync,
    BeforeRename,
    AfterRename,
    RenameResultUncertain,
    ReadBack,
    ReadBackAtRevision(u64),
    ReadBackMismatch,
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt, symlink};
    use std::process::{Command, Stdio};

    fn fixture() -> (tempfile::TempDir, CredentialStore) {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().canonicalize().unwrap().join("auth.json");
        (directory, CredentialStore::new(path).unwrap())
    }

    fn destinations() -> Vec<AllowedDestination> {
        vec![AllowedDestination {
            origin: "https://example.test".into(),
            path_prefix: "/v1".into(),
            protocols: vec!["responses".into()],
            headers: vec!["authorization".into()],
        }]
    }

    fn pending(oauth: bool) -> PendingCredential {
        PendingCredential {
            kind: if oauth {
                CredentialKind::Oauth
            } else {
                CredentialKind::ApiKey
            },
            provider: "synthetic-provider".into(),
            definition_version: 1,
            issuer: if oauth {
                "https://issuer.example.test".into()
            } else {
                String::new()
            },
            client_id: if oauth {
                "synthetic-client".into()
            } else {
                String::new()
            },
            account_id: oauth.then(|| "synthetic-account".into()),
            user_id: oauth.then(|| "synthetic-user".into()),
            allowed_destinations: destinations(),
            api_key: (!oauth).then(|| "synthetic-api-key".into()),
            access_token: oauth.then(|| "synthetic-access-token".into()),
            refresh_token: oauth.then(|| "synthetic-refresh-token".into()),
            expires_at_ms: oauth.then_some(1),
        }
    }

    fn tokens() -> RefreshTokens {
        RefreshTokens {
            issuer: "https://issuer.example.test".into(),
            client_id: "synthetic-client".into(),
            account_id: "synthetic-account".into(),
            user_id: Some("synthetic-user".into()),
            access_token: "synthetic-new-access-token".into(),
            refresh_token: Some("synthetic-new-refresh-token".into()),
            expires_at_ms: 1000,
        }
    }

    fn insert(store: &CredentialStore, oauth: bool) -> AuthIdentitySnapshot {
        store
            .modify(
                "credential",
                None,
                Mutation::Replace(Replacement::Login(pending(oauth))),
                &LockWait::default(),
            )
            .unwrap()
            .identity()
            .unwrap()
    }

    fn write_raw(store: &CredentialStore, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&store.path)
            .unwrap();
        file.write_all(bytes).unwrap();
        file.sync_all().unwrap();
    }

    fn revision(store: &CredentialStore) -> u64 {
        store.read("credential").unwrap().revision
    }

    #[test]
    fn missing_reads_do_not_create_files_or_directories() {
        let (_directory, store) = fixture();
        let absent =
            CredentialStore::new(store.path.parent().unwrap().join("missing/auth.json")).unwrap();
        assert!(absent.list_status().unwrap().is_empty());
        assert_eq!(absent.read("credential").err(), Some(StoreError::NotFound));
        assert!(!absent.path.parent().unwrap().exists());
        assert!(store.list_status().unwrap().is_empty());
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
            0
        );
        assert!(matches!(
            CredentialStore::new("relative/auth.json"),
            Err(StoreError::UnsafePath)
        ));
    }

    #[test]
    fn mutations_preserve_legacy_roots_and_keep_is_byte_idempotent() {
        let (_directory, store) = fixture();
        let legacy = serde_json::json!({"OPENAI_API_KEY": "synthetic-legacy", "other": {"nested": [true, 12, null]}});
        write_raw(&store, &serde_json::to_vec(&legacy).unwrap());
        let identity = insert(&store, false);
        let original = fs::read(&store.path).unwrap();
        store
            .modify("credential", Some(1), Mutation::Keep, &LockWait::default())
            .unwrap();
        assert_eq!(fs::read(&store.path).unwrap(), original);
        let parsed: Value = serde_json::from_slice(&original).unwrap();
        assert_eq!(parsed["OPENAI_API_KEY"], legacy["OPENAI_API_KEY"]);
        assert_eq!(parsed["other"], legacy["other"]);
        assert_eq!(parsed[STORE_KEY]["revision"], 1);
        let mut changed = destinations();
        changed[0].path_prefix = "/v2".into();
        let committed = store
            .modify(
                "credential",
                Some(1),
                Mutation::Replace(Replacement::AllowedDestinations(changed.clone())),
                &LockWait::default(),
            )
            .unwrap();
        let next = committed.identity().unwrap();
        assert_eq!(next.identity_generation, identity.identity_generation);
        assert_eq!(next.credential_revision, 2);
        assert_eq!(next.allowed_destinations, changed);
        assert_eq!(fs::metadata(&store.path).unwrap().mode() & 0o7777, 0o600);
        assert_eq!(
            fs::metadata(store.path.with_extension("json.lock"))
                .unwrap()
                .mode()
                & 0o7777,
            0o600
        );
    }

    #[test]
    fn legacy_updates_preserve_fresh_namespace_and_do_not_expose_it() {
        let (_directory, store) = fixture();
        insert(&store, true);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        let ticket = store
            .begin_refresh(&guard, 1, 123, &LockWait::default())
            .unwrap();
        store
            .finish_refresh(
                &guard,
                &ticket,
                RefreshResolution::Tokens(tokens()),
                &LockWait::default(),
            )
            .unwrap();
        let fresh = store.read("credential").unwrap();
        let previous_file_revision = store.read_document().unwrap().namespace.revision;
        let result = store
            .update_legacy(&LockWait::default(), |legacy| {
                assert!(!legacy.contains_key(STORE_KEY));
                legacy.insert(
                    "OPENAI_API_KEY".into(),
                    Value::String("synthetic-legacy".into()),
                );
                legacy.insert("other".into(), serde_json::json!({"retained": true}));
                Ok(17)
            })
            .unwrap();
        assert_eq!(result, (17, true));
        let document = store.read_document().unwrap();
        assert!(document.namespace.accounts["credential"] == fresh);
        assert_eq!(document.namespace.revision, previous_file_revision + 1);
        assert_eq!(document.legacy["OPENAI_API_KEY"], "synthetic-legacy");
        assert_eq!(document.legacy["other"]["retained"], true);
        assert_eq!(fs::metadata(&store.path).unwrap().mode() & 0o7777, 0o600);
    }

    #[test]
    fn legacy_update_noop_and_closure_error_leave_bytes_unchanged() {
        let (_directory, store) = fixture();
        let (_, changed) = store
            .update_legacy(&LockWait::default(), |_| Ok(()))
            .unwrap();
        assert!(!changed);
        assert!(!store.path.exists());
        write_raw(&store, b"{ \"OPENAI_API_KEY\": \"synthetic-existing\" }\n");
        let before = fs::read(&store.path).unwrap();
        let failed_write = CredentialStore {
            fault: Some(WriteFault::BeforeWrite),
            ..store.clone()
        };
        assert_eq!(
            failed_write.update_legacy(&LockWait::default(), |legacy| {
                legacy.insert(
                    "OPENAI_API_KEY".into(),
                    Value::String("synthetic-existing".into()),
                );
                Ok(21)
            }),
            Ok((21, false))
        );
        assert_eq!(fs::read(&store.path).unwrap(), before);
        assert_eq!(
            store.update_legacy::<()>(&LockWait::default(), |legacy| {
                legacy.clear();
                Err(StoreError::InvalidData("synthetic validation failure"))
            }),
            Err(StoreError::InvalidData("synthetic validation failure"))
        );
        assert_eq!(fs::read(&store.path).unwrap(), before);
    }

    #[test]
    fn legacy_update_rejects_reserved_namespace_and_unreadable_values_before_write() {
        let (_directory, store) = fixture();
        insert(&store, false);
        let before = fs::read(&store.path).unwrap();
        assert_eq!(
            store.update_legacy(&LockWait::default(), |legacy| {
                legacy.insert(
                    STORE_KEY.into(),
                    serde_json::json!({"version": 1, "accounts": {}}),
                );
                Ok(())
            }),
            Err(StoreError::InvalidData("reserved credential namespace"))
        );
        assert_eq!(fs::read(&store.path).unwrap(), before);
        for case in 0..3 {
            let value = match case {
                0 => Value::String("x".repeat(MAX_TOKEN_BYTES + 1)),
                1 => Value::Array(vec![
                    Value::String("x".repeat(MAX_TOKEN_BYTES));
                    MAX_FILE_BYTES / MAX_TOKEN_BYTES + 1
                ]),
                _ => (0..130).fold(Value::Null, |value, _| Value::Array(vec![value])),
            };
            assert!(matches!(
                store.update_legacy(&LockWait::default(), |legacy| {
                    legacy.insert("invalid".into(), value);
                    Ok(())
                }),
                Err(StoreError::InvalidData(_))
            ));
            assert_eq!(fs::read(&store.path).unwrap(), before);
        }
        assert_eq!(revision(&store), 1);
    }

    #[test]
    fn legacy_update_interleaves_with_refresh_without_invalidating_credential_cas() {
        let (_directory, store) = fixture();
        let identity = insert(&store, true);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        let ticket = store
            .begin_refresh(&guard, 1, 123, &LockWait::default())
            .unwrap();
        let marked = store.read("credential").unwrap();
        store
            .update_legacy(&LockWait::default(), |legacy| {
                legacy.insert(
                    "OPENAI_API_KEY".into(),
                    Value::String("synthetic-interleaved".into()),
                );
                Ok(())
            })
            .unwrap();
        assert!(store.read("credential").unwrap() == marked);
        let committed = store
            .finish_refresh(
                &guard,
                &ticket,
                RefreshResolution::Tokens(tokens()),
                &LockWait::default(),
            )
            .unwrap();
        assert_eq!(committed.identity().unwrap().credential_revision, 3);
        assert_eq!(
            committed.identity().unwrap().identity_generation,
            identity.identity_generation
        );
        store
            .update_legacy(&LockWait::default(), |legacy| {
                assert_eq!(legacy["OPENAI_API_KEY"], "synthetic-interleaved");
                legacy.insert("other".into(), Value::Bool(true));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store.read("credential").unwrap().access_token.as_deref(),
            Some("synthetic-new-access-token")
        );
        assert_eq!(store.read_document().unwrap().legacy["other"], true);
    }

    #[test]
    fn legacy_update_uncertainty_returns_no_success_and_preserves_credentials() {
        let (_directory, store) = fixture();
        insert(&store, true);
        let original = store.read("credential").unwrap();
        let uncertain = store.with_uncertain_commit_for_test();
        assert_eq!(
            uncertain.update_legacy(&LockWait::default(), |legacy| {
                legacy.insert("other".into(), Value::Bool(true));
                Ok("only-after-readback")
            }),
            Err(StoreError::CommitUncertain)
        );
        assert!(store.read("credential").unwrap() == original);
        assert_eq!(store.read_document().unwrap().legacy["other"], true);
        assert_eq!(
            store.update_legacy(&LockWait::default(), |legacy| {
                legacy.insert("other".into(), Value::Bool(true));
                Ok(())
            }),
            Ok(((), false))
        );
    }

    #[test]
    fn revoke_retains_tombstone_and_cas_prevents_aba() {
        let (_directory, store) = fixture();
        let initial = insert(&store, true);
        let revoked = store
            .modify(
                "credential",
                Some(1),
                Mutation::Revoke,
                &LockWait::default(),
            )
            .unwrap();
        assert_eq!(revoked.identity(), Err(StoreError::Inactive));
        assert_eq!(store.identity("credential"), Err(StoreError::Inactive));
        let bytes = fs::read_to_string(&store.path).unwrap();
        assert!(!bytes.contains("synthetic-access-token"));
        assert!(!bytes.contains("synthetic-refresh-token"));
        assert_eq!(revision(&store), 2);
        assert!(matches!(
            store.modify(
                "credential",
                None,
                Mutation::Replace(Replacement::Login(pending(true))),
                &LockWait::default()
            ),
            Err(StoreError::Conflict)
        ));
        let replacement = store
            .modify(
                "credential",
                Some(2),
                Mutation::Replace(Replacement::Login(pending(true))),
                &LockWait::default(),
            )
            .unwrap()
            .identity()
            .unwrap();
        assert_eq!(replacement.credential_revision, 3);
        assert_ne!(replacement.identity_generation, initial.identity_generation);
        assert!(matches!(
            store.modify(
                "credential",
                Some(1),
                Mutation::Revoke,
                &LockWait::default()
            ),
            Err(StoreError::Conflict)
        ));
    }

    #[test]
    fn key_replacement_gets_new_identity_even_when_secret_is_unchanged() {
        let (_directory, store) = fixture();
        let initial = insert(&store, false);
        let next = store
            .modify(
                "credential",
                Some(1),
                Mutation::Replace(Replacement::Login(pending(false))),
                &LockWait::default(),
            )
            .unwrap()
            .identity()
            .unwrap();
        assert_ne!(initial.identity_generation, next.identity_generation);
    }

    #[test]
    fn corrupt_duplicate_and_unknown_version_are_never_overwritten() {
        let (_directory, store) = fixture();
        for data in [
            r#"{"key":1,"key":2}"#,
            r#"{"nested":{"key":1,"key":2}}"#,
            r#"{"zenpi_auth_v1":{"version":1,"version":1,"revision":0,"accounts":{}}}"#,
            r#"{"zenpi_auth_v1":{"version":2,"revision":0,"accounts":{}}}"#,
            r#"{"zenpi_auth_v1":{"version":1,"revision":0,"accounts":{},"unknown":true}}"#,
            "[1]",
            "{",
            "{} trailing",
        ] {
            write_raw(&store, data.as_bytes());
            assert!(store.list_status().is_err());
            assert!(
                store
                    .modify(
                        "credential",
                        None,
                        Mutation::Replace(Replacement::Login(pending(false))),
                        &LockWait::default()
                    )
                    .is_err()
            );
            assert_eq!(fs::read(&store.path).unwrap(), data.as_bytes());
        }
    }

    #[test]
    fn account_limit_rejects_129th_key_before_reading_its_value() {
        let accounts = (0..MAX_ACCOUNTS)
            .map(|index| format!("\"c{index}\":null"))
            .collect::<Vec<_>>()
            .join(",");
        let input =
            format!("{{\"zenpi_auth_v1\":{{\"accounts\":{{{accounts},\"overflow\":NOT_JSON");
        let error = serde_json::from_slice::<StrictValue>(input.as_bytes())
            .err()
            .unwrap();
        assert!(error.to_string().contains("account limit exceeded"));
        // The bound applies only to credential accounts, not similarly named legacy data.
        let legacy = format!("{{\"legacy\":{{\"accounts\":{{{accounts},\"extra\":null}}}}}}");
        assert!(serde_json::from_str::<StrictValue>(&legacy).is_ok());
    }

    #[test]
    fn file_and_plain_or_escaped_token_limits_are_bounded() {
        let (_directory, store) = fixture();
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&store.path)
            .unwrap();
        file.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
        assert_eq!(
            store.list_status().err(),
            Some(StoreError::InvalidData("file limit exceeded"))
        );
        fs::remove_file(&store.path).unwrap();
        let mut material = pending(false);
        material.api_key = Some("x".repeat(MAX_TOKEN_BYTES));
        store
            .modify(
                "credential",
                None,
                Mutation::Replace(Replacement::Login(material)),
                &LockWait::default(),
            )
            .unwrap();
        let before = fs::read(&store.path).unwrap();
        let mut material = pending(false);
        material.api_key = Some("x".repeat(MAX_TOKEN_BYTES + 1));
        assert!(
            store
                .modify(
                    "credential",
                    Some(1),
                    Mutation::Replace(Replacement::Login(material)),
                    &LockWait::default()
                )
                .is_err()
        );
        assert_eq!(fs::read(&store.path).unwrap(), before);
        for unit in ["x", "\\u0078", "\\n"] {
            let data = format!("{{\"token\":\"{}\"}}", unit.repeat(MAX_TOKEN_BYTES + 1));
            assert_eq!(
                check_json_string_bounds(data.as_bytes()),
                Err(StoreError::InvalidData("string limit exceeded"))
            );
            write_raw(&store, data.as_bytes());
            assert!(store.list_status().is_err());
        }
        let pair = format!("\"{}\"", "\\ud83d\\ude00".repeat(MAX_TOKEN_BYTES / 4));
        assert_eq!(check_json_string_bounds(pair.as_bytes()), Ok(()));
        assert!(serde_json::from_str::<StrictValue>(&pair).is_ok());
    }

    #[test]
    fn account_insert_limit_and_schema_errors_leave_store_unchanged() {
        let (_directory, store) = fixture();
        insert(&store, false);
        let mut document = store.read_document().unwrap();
        let template = document.namespace.accounts.remove("credential").unwrap();
        for index in 0..MAX_ACCOUNTS {
            document
                .namespace
                .accounts
                .insert(format!("c{index}"), template.clone());
        }
        write_raw(&store, &document.bytes().unwrap());
        let before = fs::read(&store.path).unwrap();
        assert_eq!(store.list_status().unwrap().len(), MAX_ACCOUNTS);
        assert!(
            store
                .modify(
                    "extra",
                    None,
                    Mutation::Replace(Replacement::Login(pending(false))),
                    &LockWait::default()
                )
                .is_err()
        );
        assert_eq!(fs::read(&store.path).unwrap(), before);
        let mut bad = pending(true);
        bad.api_key = Some("cannot-use-oauth-as-api-key".into());
        assert!(
            store
                .modify(
                    "c0",
                    Some(1),
                    Mutation::Replace(Replacement::Login(bad)),
                    &LockWait::default()
                )
                .is_err()
        );
    }

    #[test]
    fn unsafe_permissions_links_and_non_regular_files_are_rejected() {
        let (_directory, store) = fixture();
        insert(&store, false);
        fs::set_permissions(&store.path, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
        fs::set_permissions(&store.path, fs::Permissions::from_mode(0o600)).unwrap();
        let target = store.path.with_file_name("target.json");
        fs::rename(&store.path, &target).unwrap();
        symlink(&target, &store.path).unwrap();
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
        fs::remove_file(&store.path).unwrap();
        fs::hard_link(&target, &store.path).unwrap();
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
        fs::remove_file(&store.path).unwrap();
        fs::create_dir(&store.path).unwrap();
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
        fs::remove_dir(&store.path).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let fifo = std::ffi::CString::new(store.path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
    }

    #[test]
    fn unsafe_parent_and_stable_lock_paths_are_rejected() {
        let (_directory, store) = fixture();
        let parent = store.path.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        let actual = parent.join("actual");
        fs::create_dir(&actual).unwrap();
        fs::set_permissions(&actual, fs::Permissions::from_mode(0o700)).unwrap();
        let alias = parent.join("alias");
        symlink(&actual, &alias).unwrap();
        assert_eq!(
            CredentialStore::new(alias.join("auth.json"))
                .unwrap()
                .list_status()
                .err(),
            Some(StoreError::UnsafePath)
        );
        let lock = store.path.with_extension("json.lock");
        symlink(parent.join("absent"), &lock).unwrap();
        assert_eq!(store.list_status().err(), Some(StoreError::UnsafePath));
        assert!(matches!(
            store.modify(
                "credential",
                None,
                Mutation::Replace(Replacement::Login(pending(false))),
                &LockWait::default()
            ),
            Err(StoreError::UnsafePath)
        ));
        fs::remove_file(&lock).unwrap();
        let refresh_name = format!("auth.json.refresh.{:x}.lock", Sha256::digest(b"credential"));
        symlink(parent.join("absent"), parent.join(refresh_name)).unwrap();
        assert!(matches!(
            store.lock_refresh("credential", &LockWait::default()),
            Err(StoreError::UnsafePath)
        ));
    }

    #[test]
    fn known_pre_rename_failures_preserve_previous_bytes() {
        let (_directory, store) = fixture();
        insert(&store, false);
        let before = fs::read(&store.path).unwrap();
        for fault in [
            WriteFault::BeforeWrite,
            WriteFault::FileSync,
            WriteFault::BeforeRename,
        ] {
            let failed = CredentialStore {
                fault: Some(fault),
                ..store.clone()
            };
            assert!(matches!(
                failed.modify(
                    "credential",
                    Some(1),
                    Mutation::Revoke,
                    &LockWait::default()
                ),
                Err(StoreError::StorageFailed)
            ));
            assert_eq!(fs::read(&store.path).unwrap(), before);
            assert_eq!(
                fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
                2
            );
        }
    }

    #[test]
    fn rename_dirsync_and_readback_uncertainty_never_return_a_handle() {
        for fault in [
            WriteFault::RenameResultUncertain,
            WriteFault::AfterRename,
            WriteFault::ReadBack,
            WriteFault::ReadBackMismatch,
        ] {
            let (_directory, store) = fixture();
            insert(&store, false);
            let uncertain = CredentialStore {
                fault: Some(fault),
                ..store.clone()
            };
            assert!(matches!(
                uncertain.modify(
                    "credential",
                    Some(1),
                    Mutation::Revoke,
                    &LockWait::default()
                ),
                Err(StoreError::CommitUncertain)
            ));
            // The rename really succeeded. Reload observes the tombstone; retry
            // with the old revision cannot repeat the mutation or revive it.
            assert_eq!(revision(&store), 2);
            assert_eq!(store.identity("credential"), Err(StoreError::Inactive));
            assert!(matches!(
                store.modify(
                    "credential",
                    Some(1),
                    Mutation::Revoke,
                    &LockWait::default()
                ),
                Err(StoreError::Conflict)
            ));
        }
    }

    #[test]
    fn refresh_marker_and_success_keep_identity_and_optional_refresh_token() {
        let (_directory, store) = fixture();
        let initial = insert(&store, true);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        let ticket = store
            .begin_refresh(&guard, 1, 123, &LockWait::default())
            .unwrap();
        let marked = store.read("credential").unwrap();
        assert_eq!(marked.state, CredentialState::RefreshInFlight);
        assert_eq!(marked.revision, 2);
        assert_eq!(marked.refresh.as_ref().unwrap().previous_revision, 1);
        assert_eq!(marked.refresh.as_ref().unwrap().started_at_ms, 123);
        assert_eq!(store.identity("credential"), Err(StoreError::Inactive));
        let mut refreshed = tokens();
        refreshed.refresh_token = None;
        let committed = store
            .finish_refresh(
                &guard,
                &ticket,
                RefreshResolution::Tokens(refreshed),
                &LockWait::default(),
            )
            .unwrap();
        assert_eq!(
            committed.identity().unwrap().identity_generation,
            initial.identity_generation
        );
        assert_eq!(committed.identity().unwrap().credential_revision, 3);
        assert_eq!(
            store.read("credential").unwrap().refresh_token.as_deref(),
            Some("synthetic-refresh-token")
        );
        assert!(store.read("credential").unwrap().refresh.is_none());
    }

    #[test]
    fn committed_reads_check_revision_state_and_cancellation_before_secret_borrow() {
        let (_directory, store) = fixture();
        insert(&store, false);
        let committed = store
            .read_committed("credential", 1, &LockWait::default())
            .unwrap();
        assert_eq!(committed.metadata().identity.credential_revision, 1);
        assert_eq!(
            committed.with_request_secret(|secret| secret == "synthetic-api-key"),
            Ok(true)
        );
        assert!(matches!(
            store.read_committed("credential", 2, &LockWait::default()),
            Err(StoreError::Conflict)
        ));
        let cancelled = LockWait {
            cancelled: &|| true,
            ..LockWait::default()
        };
        assert!(matches!(
            store.metadata("credential", &cancelled),
            Err(StoreError::Cancelled)
        ));
        store
            .modify(
                "credential",
                Some(1),
                Mutation::Revoke,
                &LockWait::default(),
            )
            .unwrap();
        assert!(matches!(
            store.read_committed("credential", 2, &LockWait::default()),
            Err(StoreError::Inactive)
        ));
    }

    #[test]
    fn refresh_borrow_requires_live_ticket_and_releases_global_lock() {
        let (_directory, store) = fixture();
        insert(&store, true);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        let ticket = store
            .begin_refresh(&guard, 1, 123, &LockWait::default())
            .unwrap();
        store
            .with_refresh_token(&guard, &ticket, &LockWait::default(), |secret| {
                assert_eq!(secret, "synthetic-refresh-token");
                store
                    .modify(
                        "credential",
                        Some(2),
                        Mutation::Revoke,
                        &LockWait::default(),
                    )
                    .unwrap();
            })
            .unwrap();
        assert_eq!(
            store.with_refresh_token(&guard, &ticket, &LockWait::default(), |_| panic!(
                "stale ticket cannot borrow"
            )),
            Err(StoreError::Conflict)
        );
        let (_other_directory, other) = fixture();
        assert_eq!(
            other.with_refresh_token(&guard, &ticket, &LockWait::default(), |_| panic!(
                "wrong store cannot borrow"
            )),
            Err(StoreError::Conflict)
        );
    }

    #[test]
    fn refresh_rejects_identity_change_and_stale_ticket_after_login_or_revoke() {
        for replace in [true, false] {
            let (_directory, store) = fixture();
            insert(&store, true);
            let guard = store
                .lock_refresh("credential", &LockWait::default())
                .unwrap();
            let ticket = store
                .begin_refresh(&guard, 1, 0, &LockWait::default())
                .unwrap();
            for field in 0..4 {
                let mut changed = tokens();
                match field {
                    0 => changed.issuer.push_str("/other"),
                    1 => changed.client_id.push_str("other"),
                    2 => changed.account_id.push_str("other"),
                    _ => changed.user_id = None,
                }
                assert!(matches!(
                    store.finish_refresh(
                        &guard,
                        &ticket,
                        RefreshResolution::Tokens(changed),
                        &LockWait::default()
                    ),
                    Err(StoreError::IdentityMismatch)
                ));
                assert_eq!(revision(&store), 2);
            }
            let mutation = if replace {
                Mutation::Replace(Replacement::Login(pending(true)))
            } else {
                Mutation::Revoke
            };
            store
                .modify("credential", Some(2), mutation, &LockWait::default())
                .unwrap();
            assert!(matches!(
                store.finish_refresh(
                    &guard,
                    &ticket,
                    RefreshResolution::Tokens(tokens()),
                    &LockWait::default()
                ),
                Err(StoreError::Conflict)
            ));
            assert_eq!(revision(&store), 3);
        }
    }

    #[test]
    fn refresh_outcomes_fail_closed_except_proven_not_sent() {
        for state in [
            CredentialState::Active,
            CredentialState::RefreshUncertain,
            CredentialState::LoginRequired,
        ] {
            let (_directory, store) = fixture();
            insert(&store, true);
            let guard = store
                .lock_refresh("credential", &LockWait::default())
                .unwrap();
            let ticket = store
                .begin_refresh(&guard, 1, 0, &LockWait::default())
                .unwrap();
            let resolution = match state {
                CredentialState::Active => RefreshResolution::NotSent,
                CredentialState::RefreshUncertain => RefreshResolution::Uncertain,
                _ => RefreshResolution::LoginRequired,
            };
            let committed = store
                .finish_refresh(&guard, &ticket, resolution, &LockWait::default())
                .unwrap();
            let current = store.read("credential").unwrap();
            assert_eq!(current.state, state);
            assert_eq!(
                committed.identity().is_ok(),
                state == CredentialState::Active
            );
            if state == CredentialState::LoginRequired {
                assert!(current.refresh_token.is_none());
            }
            if state != CredentialState::Active {
                assert!(matches!(
                    store.begin_refresh(&guard, 3, 0, &LockWait::default()),
                    Err(StoreError::Inactive)
                ));
            }
        }
    }

    #[test]
    fn failed_token_commit_keeps_marker_and_abandoned_recovery_blocks_reuse() {
        let (_directory, store) = fixture();
        insert(&store, true);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        let ticket = store
            .begin_refresh(&guard, 1, 0, &LockWait::default())
            .unwrap();
        let failed = CredentialStore {
            fault: Some(WriteFault::BeforeRename),
            ..store.clone()
        };
        assert!(matches!(
            failed.finish_refresh(
                &guard,
                &ticket,
                RefreshResolution::Tokens(tokens()),
                &LockWait::default()
            ),
            Err(StoreError::StorageFailed)
        ));
        assert_eq!(
            store.read("credential").unwrap().state,
            CredentialState::RefreshInFlight
        );
        assert_eq!(store.identity("credential"), Err(StoreError::Inactive));
        drop(guard);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        store
            .recover_abandoned_refresh(&guard, &LockWait::default())
            .unwrap();
        assert_eq!(
            store.read("credential").unwrap().state,
            CredentialState::RefreshUncertain
        );
        assert!(matches!(
            store.begin_refresh(&guard, 3, 0, &LockWait::default()),
            Err(StoreError::Inactive)
        ));
        let before = fs::read(&store.path).unwrap();
        store
            .recover_abandoned_refresh(&guard, &LockWait::default())
            .unwrap();
        assert_eq!(fs::read(&store.path).unwrap(), before);
    }

    #[test]
    fn cancellation_and_deadline_do_not_mutate() {
        let (_directory, store) = fixture();
        let cancelled = LockWait {
            deadline: Instant::now() + Duration::from_secs(1),
            cancelled: &|| true,
        };
        assert!(matches!(
            store.modify(
                "credential",
                None,
                Mutation::Replace(Replacement::Login(pending(false))),
                &cancelled
            ),
            Err(StoreError::Cancelled)
        ));
        let expired = LockWait {
            deadline: Instant::now(),
            cancelled: &|| false,
        };
        assert!(matches!(
            store.modify(
                "credential",
                None,
                Mutation::Replace(Replacement::Login(pending(false))),
                &expired
            ),
            Err(StoreError::LockTimeout)
        ));
        assert_eq!(
            fs::read_dir(store.path.parent().unwrap()).unwrap().count(),
            0
        );
    }

    #[test]
    fn runtime_credentials_have_no_serialize_or_debug_implementation() {
        macro_rules! assert_not_impl {
            ($ty:ty, $bound:path) => {{
                trait Ambiguous<A> {
                    fn check() {}
                }
                impl<T: ?Sized> Ambiguous<()> for T {}
                struct HasBound;
                impl<T: ?Sized + $bound> Ambiguous<HasBound> for T {}
                let _ = <$ty as Ambiguous<_>>::check;
            }};
        }
        assert_not_impl!(CommittedCredential, Serialize);
        assert_not_impl!(CommittedCredential, std::fmt::Debug);
        assert_not_impl!(PendingCredential, Serialize);
        assert_not_impl!(PendingCredential, std::fmt::Debug);
        assert_not_impl!(RefreshTokens, Serialize);
        assert_not_impl!(RefreshTokens, std::fmt::Debug);
        assert_not_impl!(PrivateCredential, std::fmt::Debug);
    }

    fn child(store: &CredentialStore, mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "auth::store::tests::subprocess_worker",
                "--nocapture",
            ])
            .env_clear()
            .env("TMPDIR", std::env::temp_dir())
            .env("ZENPI_PA03_CHILD_MODE", mode)
            .env("ZENPI_PA03_CHILD_PATH", &store.path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    #[test]
    fn subprocess_locks_are_stable_and_released_on_process_exit() {
        let (_directory, store) = fixture();
        insert(&store, true);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        let other = store
            .lock_refresh("different", &LockWait::default())
            .unwrap();
        let inode = guard._lock.metadata().unwrap().ino();
        assert!(
            child(&store, "lock_timeout")
                .output()
                .unwrap()
                .status
                .success()
        );
        drop(other);
        drop(guard);
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        assert_eq!(guard._lock.metadata().unwrap().ino(), inode);
        drop(guard);
        assert_eq!(
            child(&store, "crash_after_marker")
                .output()
                .unwrap()
                .status
                .code(),
            Some(17)
        );
        let guard = store
            .lock_refresh("credential", &LockWait::default())
            .unwrap();
        assert_eq!(guard._lock.metadata().unwrap().ino(), inode);
        store
            .recover_abandoned_refresh(&guard, &LockWait::default())
            .unwrap();
        assert_eq!(
            store.read("credential").unwrap().state,
            CredentialState::RefreshUncertain
        );
    }

    #[test]
    fn subprocess_cas_allows_exactly_one_writer() {
        let (_directory, store) = fixture();
        insert(&store, false);
        let lock_path = store.path.with_extension("json.lock");
        let inode = fs::metadata(&lock_path).unwrap().ino();
        let first = child(&store, "cas").spawn().unwrap();
        let second = child(&store, "cas").spawn().unwrap();
        let mut codes = [
            first.wait_with_output().unwrap().status.code(),
            second.wait_with_output().unwrap().status.code(),
        ];
        codes.sort();
        assert_eq!(codes, [Some(0), Some(3)]);
        assert_eq!(revision(&store), 2);
        assert_eq!(fs::metadata(lock_path).unwrap().ino(), inode);
    }

    #[test]
    fn subprocess_peer_refresh_rechecks_after_lock_and_rotates_once() {
        let (_directory, store) = fixture();
        let identity = insert(&store, true);
        let first = child(&store, "refresh").spawn().unwrap();
        let second = child(&store, "refresh").spawn().unwrap();
        let outputs = [
            first.wait_with_output().unwrap(),
            second.wait_with_output().unwrap(),
        ];
        for output in outputs {
            assert!(
                output.status.success(),
                "synthetic child failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        assert_eq!(
            fs::read_to_string(store.path.with_file_name("dispatches")).unwrap(),
            "dispatch\n"
        );
        assert_eq!(revision(&store), 3);
        assert_eq!(
            store.identity("credential").unwrap().identity_generation,
            identity.identity_generation
        );
    }

    #[test]
    fn subprocess_legacy_updates_serialize_with_each_other_and_refresh() {
        let (_directory, store) = fixture();
        let identity = insert(&store, true);
        let lock_path = store.path.with_extension("json.lock");
        let inode = fs::metadata(&lock_path).unwrap().ino();
        let first = child(&store, "legacy_increment").spawn().unwrap();
        let second = child(&store, "legacy_increment").spawn().unwrap();
        let refresh = child(&store, "refresh").spawn().unwrap();
        for worker in [first, second, refresh] {
            let output = worker.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "synthetic child failed: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let document = store.read_document().unwrap();
        assert_eq!(document.legacy["counter"], 20);
        assert_eq!(document.namespace.revision, 23);
        assert_eq!(revision(&store), 3);
        assert_eq!(
            store.identity("credential").unwrap().identity_generation,
            identity.identity_generation
        );
        assert_eq!(
            store.read("credential").unwrap().refresh_token.as_deref(),
            Some("synthetic-new-refresh-token")
        );
        assert_eq!(fs::metadata(lock_path).unwrap().ino(), inode);
    }

    #[test]
    fn subprocess_worker() {
        let Ok(mode) = std::env::var("ZENPI_PA03_CHILD_MODE") else {
            return;
        };
        let path = PathBuf::from(std::env::var_os("ZENPI_PA03_CHILD_PATH").unwrap());
        assert!(path.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        assert_eq!(path.file_name().unwrap(), "auth.json");
        let store = CredentialStore::new(path).unwrap();
        match mode.as_str() {
            "legacy_increment" => {
                for _ in 0..10 {
                    store
                        .update_legacy(&LockWait::default(), |legacy| {
                            let current =
                                legacy.get("counter").and_then(Value::as_u64).unwrap_or(0);
                            legacy.insert("counter".into(), Value::from(current + 1));
                            Ok(())
                        })
                        .unwrap();
                }
            }
            "lock_timeout" => {
                let wait = LockWait {
                    deadline: Instant::now() + Duration::from_millis(40),
                    cancelled: &|| false,
                };
                assert!(matches!(
                    store.lock_refresh("credential", &wait),
                    Err(StoreError::LockTimeout)
                ));
            }
            "cas" => match store.modify(
                "credential",
                Some(1),
                Mutation::Replace(Replacement::Login(pending(false))),
                &LockWait::default(),
            ) {
                Ok(_) => (),
                Err(StoreError::Conflict) => std::process::exit(3),
                Err(error) => panic!("{error}"),
            },
            "crash_after_marker" => {
                let guard = store
                    .lock_refresh("credential", &LockWait::default())
                    .unwrap();
                store
                    .begin_refresh(&guard, 1, 0, &LockWait::default())
                    .unwrap();
                std::process::exit(17);
            }
            "refresh" => {
                let guard = store
                    .lock_refresh("credential", &LockWait::default())
                    .unwrap();
                let current = store.read("credential").unwrap();
                if current.expires_at_ms == Some(1) {
                    let ticket = store
                        .begin_refresh(&guard, current.revision, 0, &LockWait::default())
                        .unwrap();
                    let mut count = OpenOptions::new()
                        .append(true)
                        .create(true)
                        .mode(0o600)
                        .open(store.path.with_file_name("dispatches"))
                        .unwrap();
                    count.write_all(b"dispatch\n").unwrap();
                    std::thread::sleep(Duration::from_millis(30));
                    store
                        .finish_refresh(
                            &guard,
                            &ticket,
                            RefreshResolution::Tokens(tokens()),
                            &LockWait::default(),
                        )
                        .unwrap();
                }
            }
            _ => panic!("unknown synthetic test mode"),
        }
    }
}
