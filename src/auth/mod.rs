//! Provider credentials and their non-secret identity boundary.

use serde::{Deserialize, Serialize};

pub(crate) mod callback;
pub(crate) mod codex;
pub(crate) mod resolve;
pub mod store;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthBinding {
    LegacyApiKey,
    StoredApiKey { credential_id: String },
    CodexOAuth { credential_id: String },
    Anonymous,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllowedDestination {
    pub origin: String,
    pub path_prefix: String,
    pub protocols: Vec<String>,
    pub headers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthIdentitySnapshot {
    pub provider: String,
    pub credential_id: String,
    pub account_id: Option<String>,
    pub identity_generation: String,
    pub credential_revision: u64,
    pub allowed_destinations: Vec<AllowedDestination>,
}

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum AuthError {
    #[error("authentication cancelled")]
    Cancelled,
    #[error("authentication deadline exceeded")]
    DeadlineExceeded,
    #[error("another login is active")]
    LoginBusy,
    #[error("authentication send budget exhausted")]
    BudgetExceeded,
    #[error("authentication send was denied")]
    SendDenied,
    #[error("authentication transport failed")]
    Transport,
    #[error("authentication response is invalid")]
    InvalidResponse,
    #[error("authentication response exceeds its size limit")]
    ResponseTooLarge,
    #[error("authentication is unavailable")]
    Unavailable,
    #[error("authentication was denied")]
    AccessDenied,
    #[error("authentication grant is invalid or expired")]
    LoginRequired,
    #[error("authentication failed")]
    LoginFailed,
    #[error("authentication callback is invalid")]
    InvalidCallback,
    #[error("authentication callback state does not match")]
    StateMismatch,
    #[error("authentication prompt is no longer active")]
    StalePrompt,
    #[error("loopback callback port is occupied; use device login")]
    CallbackPortInUse,
    #[error("loopback callback is unavailable; use device login")]
    CallbackUnavailable,
    #[error("authentication randomness unavailable")]
    RandomnessUnavailable,
    #[error("authentication host interaction failed")]
    InteractionFailed,
    #[error("credential revision changed")]
    Conflict,
    #[error("credential persistence failed")]
    StorageFailed,
    #[error("credential commit is uncertain; reload without exchanging again")]
    CommitUncertain,
    #[error("credential is not configured")]
    NotConfigured,
    #[error("credential has been revoked")]
    Revoked,
    #[error("credential refresh result is uncertain; sign in again")]
    RefreshUncertain,
    #[error("credential type does not match the authentication mode")]
    ModeMismatch,
    #[error("credential identity changed")]
    IdentityMismatch,
    #[error("credential does not authorize this destination")]
    DestinationDenied,
    #[error("authentication request scope does not match")]
    ScopeMismatch,
    #[error("authentication context expired; resolve again")]
    ContextExpired,
    #[error("credential lock deadline exceeded")]
    LockTimeout,
}

impl AuthError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Cancelled => "auth_cancelled",
            Self::DeadlineExceeded => "auth_deadline_exceeded",
            Self::LoginBusy => "auth_login_busy",
            Self::BudgetExceeded => "auth_budget_exceeded",
            Self::SendDenied => "auth_send_denied",
            Self::Transport => "auth_transport_failed",
            Self::InvalidResponse => "auth_invalid_response",
            Self::ResponseTooLarge => "auth_response_too_large",
            Self::Unavailable => "auth_unavailable",
            Self::AccessDenied => "auth_access_denied",
            Self::LoginRequired => "auth_login_required",
            Self::LoginFailed => "auth_login_failed",
            Self::InvalidCallback => "auth_invalid_callback",
            Self::StateMismatch => "auth_state_mismatch",
            Self::StalePrompt => "auth_stale_prompt",
            Self::CallbackPortInUse => "auth_callback_port_in_use",
            Self::CallbackUnavailable => "auth_callback_unavailable",
            Self::RandomnessUnavailable => "auth_randomness_unavailable",
            Self::InteractionFailed => "auth_interaction_failed",
            Self::Conflict => "auth_conflict",
            Self::StorageFailed => "auth_storage_failed",
            Self::CommitUncertain => "auth_commit_uncertain",
            Self::NotConfigured => "auth_not_configured",
            Self::Revoked => "auth_revoked",
            Self::RefreshUncertain => "auth_refresh_uncertain",
            Self::ModeMismatch => "auth_mode_mismatch",
            Self::IdentityMismatch => "auth_identity_mismatch",
            Self::DestinationDenied => "auth_destination_denied",
            Self::ScopeMismatch => "auth_scope_mismatch",
            Self::ContextExpired => "auth_context_expired",
            Self::LockTimeout => "auth_lock_timeout",
        }
    }
}

impl From<store::StoreError> for AuthError {
    fn from(error: store::StoreError) -> Self {
        match error {
            store::StoreError::Cancelled => Self::Cancelled,
            store::StoreError::LockTimeout => Self::LockTimeout,
            store::StoreError::Conflict => Self::Conflict,
            store::StoreError::CommitUncertain => Self::CommitUncertain,
            store::StoreError::NotFound => Self::NotConfigured,
            store::StoreError::Inactive => Self::LoginRequired,
            store::StoreError::IdentityMismatch => Self::IdentityMismatch,
            _ => Self::StorageFailed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LoginFlowId(String);
impl LoginFlowId {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoginState {
    Preparing,
    AwaitingAuthorization,
    Exchanging,
    Committing,
    Succeeded,
    Cancelled,
    Failed,
    CommitUncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AuthHttpKind {
    DeviceBegin,
    DevicePoll,
    CodeExchange,
    Refresh,
}

// These events are exclusively for a trusted host, never ProviderEvent/log JSON.
pub(crate) enum AuthPrompt {
    Browser {
        authorization_url: String,
        manual: callback::ManualSubmission,
    },
    Device {
        verification_uri: &'static str,
        user_code: String,
    },
}
pub(crate) enum AuthInteraction {
    Notify {
        flow_id: LoginFlowId,
        state: LoginState,
    },
    Prompt {
        flow_id: LoginFlowId,
        prompt_id: u64,
        prompt: AuthPrompt,
    },
    CancelPrompt {
        flow_id: LoginFlowId,
        prompt_id: u64,
    },
}

pub(crate) struct LoginControl<'a> {
    cancelled: &'a dyn Fn() -> bool,
    deadline: Instant,
    sends_remaining: u32,
    before_send: &'a mut dyn FnMut(AuthHttpKind) -> Result<(), AuthError>,
    interaction: &'a mut dyn FnMut(AuthInteraction) -> Result<(), AuthError>,
    flow_cancelled: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl<'a> LoginControl<'a> {
    pub(crate) fn new(
        cancelled: &'a dyn Fn() -> bool,
        deadline: Instant,
        sends_remaining: u32,
        before_send: &'a mut dyn FnMut(AuthHttpKind) -> Result<(), AuthError>,
        interaction: &'a mut dyn FnMut(AuthInteraction) -> Result<(), AuthError>,
    ) -> Self {
        Self {
            cancelled,
            deadline: deadline.min(Instant::now() + Duration::from_secs(15 * 60)),
            sends_remaining,
            before_send,
            interaction,
            flow_cancelled: None,
        }
    }
    pub(crate) fn check(&self) -> Result<(), AuthError> {
        if self.is_cancelled() {
            Err(AuthError::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(AuthError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }
    fn is_cancelled(&self) -> bool {
        (self.cancelled)()
            || self
                .flow_cancelled
                .as_ref()
                .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire))
    }
    fn admit(&mut self, kind: AuthHttpKind) -> Result<(), AuthError> {
        self.check()?;
        self.sends_remaining = self
            .sends_remaining
            .checked_sub(1)
            .ok_or(AuthError::BudgetExceeded)?;
        let admission = (self.before_send)(kind);
        self.check()?;
        admission.map_err(|_| AuthError::SendDenied)
    }
    fn emit(&mut self, event: AuthInteraction) -> Result<(), AuthError> {
        (self.interaction)(event).map_err(|_| AuthError::InteractionFailed)
    }
}

pub(crate) struct LoginRequest<'a> {
    pub(crate) store: &'a store::CredentialStore,
    pub(crate) credential_id: &'a str,
    pub(crate) expected_revision: Option<u64>,
}
pub(crate) struct LoginSuccess {
    pub(crate) flow_id: LoginFlowId,
    pub(crate) credential: store::CommittedCredential,
}
pub(crate) type LoginOutcome = Result<LoginSuccess, AuthError>;

/// Credential shape for a provider API key.
///
/// The allowed destinations are handed in rather than derived here: they come
/// from the same service policy the request path uses
/// (`providers::connection::api_key_destinations`), and this module owns
/// credential shape, not routing.  An API key has no issuer, session, or
/// expiry; those belong to OAuth bindings.
pub(crate) fn api_key_credential(
    provider: &str,
    definition_version: u32,
    key: String,
    grants: Vec<AllowedDestination>,
) -> store::PendingCredential {
    store::PendingCredential {
        kind: store::CredentialKind::ApiKey,
        provider: provider.to_owned(),
        definition_version,
        issuer: String::new(),
        client_id: String::new(),
        account_id: None,
        user_id: None,
        allowed_destinations: grants,
        api_key: Some(key),
        access_token: None,
        refresh_token: None,
        expires_at_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_credentials_carry_no_oauth_identity_and_only_their_grants() {
        let destination = crate::providers::connection::api_key_destination(
            "openai",
            None,
            Some("responses"),
            None,
        )
        .unwrap();
        let pending = api_key_credential(
            "openai",
            destination.definition_version,
            "synthetic-key".into(),
            vec![destination.grant.clone()],
        );
        assert_eq!(pending.kind, store::CredentialKind::ApiKey);
        assert_eq!(pending.provider, "openai");
        assert_eq!(pending.definition_version, destination.definition_version);
        assert!(pending.issuer.is_empty());
        assert!(pending.client_id.is_empty());
        assert!(pending.account_id.is_none());
        assert!(pending.user_id.is_none());
        assert!(pending.access_token.is_none());
        assert!(pending.refresh_token.is_none());
        assert!(pending.expires_at_ms.is_none());
        assert_eq!(pending.api_key.as_deref(), Some("synthetic-key"));
        assert_eq!(pending.allowed_destinations, [destination.grant]);
    }

    /// A provider with several wires authorizes all of them at once, so a key
    /// added for one protocol is not silently unusable on the others.
    #[test]
    fn unnamed_wires_authorize_every_route_of_a_builtin_provider() {
        let destinations =
            crate::providers::connection::api_key_destinations("deepseek", None, None, None)
                .unwrap();
        let definition = crate::providers::get_provider_definition("deepseek").unwrap();
        assert_eq!(destinations.len(), definition.routes.len());
        assert_eq!(
            destinations.iter().map(|d| d.protocol).collect::<Vec<_>>(),
            definition
                .routes
                .iter()
                .map(|route| route.protocol)
                .collect::<Vec<_>>()
        );
        for destination in &destinations {
            assert_eq!(destination.grant.origin, "https://api.deepseek.com");
        }

        // A named wire still selects exactly one route.
        let named = crate::providers::connection::api_key_destinations(
            "deepseek",
            None,
            Some("chat_completions"),
            None,
        )
        .unwrap();
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].protocol, destinations[0].protocol);
    }

    /// The whole point of deriving a login grant from the service policy is
    /// that the credential can immediately reach the routes it authorizes.
    #[test]
    fn every_grant_of_a_multi_route_provider_admits_its_request_route() {
        use crate::providers::connection::{ProviderConnection, resolve_connection};
        use crate::providers::registry::ModelRegistry;

        let destinations =
            crate::providers::connection::api_key_destinations("deepseek", None, None, None)
                .unwrap();
        let identity = AuthIdentitySnapshot {
            provider: "deepseek".into(),
            credential_id: "cred_test".into(),
            account_id: None,
            identity_generation: "generation_1".into(),
            credential_revision: 1,
            allowed_destinations: destinations
                .iter()
                .map(|destination| destination.grant.clone())
                .collect(),
        };
        for destination in &destinations {
            let connection = ProviderConnection {
                profile: "test-profile".into(),
                provider: "deepseek".into(),
                protocol: destination.protocol,
                base_url: None,
                auth: AuthBinding::StoredApiKey {
                    credential_id: "cred_test".into(),
                },
                header_policy: None,
                config_revision: 1,
                model_routes: vec![],
            };
            resolve_connection(
                &connection,
                "test-model",
                &ModelRegistry::default(),
                Some(&identity),
                true,
            )
            .unwrap_or_else(|error| {
                panic!(
                    "{:?} grant rejects its own route: {error}",
                    destination.protocol
                )
            });
        }
    }
}
