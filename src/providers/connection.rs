//! Pure explicit-connection validation. No credentials, HTTP, or session mutation.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};

use crate::auth::{AllowedDestination, AuthBinding, AuthIdentitySnapshot};
use crate::backend::{BackendError, ProviderCapabilities};

use super::registry::{FieldSource, ModelDescriptor, ModelRegistry, validate_identity};
use super::{
    AuthHeaderPolicy, CUSTOM, Dialect, EndpointOperation, EndpointRule, OptionPolicy, Protocol,
    ProviderDefinition, RouteRule, get_provider_definition,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRoute {
    pub model: String,
    #[serde(default)]
    pub wire_api: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ProviderConnection {
    pub profile: String,
    pub provider: String,
    pub protocol: Protocol,
    pub base_url: Option<String>,
    pub auth: AuthBinding,
    pub header_policy: Option<AuthHeaderPolicy>,
    pub config_revision: u64,
    pub model_routes: Vec<ModelRoute>,
}

#[derive(Debug, Clone)]
pub(crate) struct ApiKeyDestination {
    pub protocol: Protocol,
    pub definition_version: u32,
    /// The one destination this route authorizes, headers included.
    pub grant: AllowedDestination,
}

/// Build a login grant from the same service policy used to prepare requests.
/// Google grants cover only the final `models` subtree, since both the model
/// and generation operation are selected later. Other grants bind one operation.
pub(crate) fn api_key_destination(
    provider: &str,
    base_url: Option<&str>,
    wire: Option<&str>,
    auth_header: Option<&str>,
) -> Result<ApiKeyDestination, BackendError> {
    validate_identity(provider, "credential-scope")
        .map_err(|_| invalid("invalid provider identity"))?;
    let definition = get_provider_definition(provider).unwrap_or(&CUSTOM);
    if definition.id == super::codex::definition().id {
        return Err(invalid("Codex requires its OAuth binding"));
    }
    if definition.id == CUSTOM.id && (wire.is_none() || auth_header.is_none()) {
        return Err(invalid(
            "custom API key providers require explicit protocol and header policy",
        ));
    }
    let protocol = match wire {
        Some(wire) => Protocol::parse(wire)?,
        None => match definition.routes {
            [route] => route.protocol,
            _ => return Err(invalid("provider requires an explicit protocol")),
        },
    };
    let rule = service_route(definition, protocol)?;
    let header_policy = api_key_header(
        rule,
        protocol,
        auth_header.map(AuthHeaderPolicy::parse).transpose()?,
    )?;
    let mut url = final_url(rule.endpoint, base_url, "credential-scope", false)?;
    if url.scheme() != "https" {
        return Err(invalid("credential-bearing destinations require HTTPS"));
    }
    if matches!(
        rule.endpoint,
        EndpointRule::PrefixAndOperation {
            operation: EndpointOperation::GoogleGenerateContent,
            ..
        }
    ) {
        url.path_segments_mut()
            .map_err(|_| invalid("invalid hierarchical API prefix"))?
            .pop();
    }
    let grant = AllowedDestination {
        origin: url.origin().ascii_serialization(),
        path_prefix: url.path().into(),
        protocols: vec![protocol.as_str().into()],
        headers: header_policy
            .headers()
            .iter()
            .map(|s| (*s).into())
            .collect(),
    };
    validate_grant(&grant)?;
    Ok(ApiKeyDestination {
        protocol,
        definition_version: definition.definition_version,
        grant,
    })
}

/// Grants for a login that does not name a wire.
///
/// A provider with several routes cannot be reduced to one destination, and
/// guessing one would hide the others.  The caller gets the whole set and shows
/// it to the user, which is what a built-in multi-protocol provider needs at
/// key-add time.  Naming a wire selects exactly that route.
pub(crate) fn api_key_destinations(
    provider: &str,
    base_url: Option<&str>,
    wire: Option<&str>,
    auth_header: Option<&str>,
) -> Result<Vec<ApiKeyDestination>, BackendError> {
    if wire.is_some() {
        return Ok(vec![api_key_destination(
            provider,
            base_url,
            wire,
            auth_header,
        )?]);
    }
    validate_identity(provider, "credential-scope")
        .map_err(|_| invalid("invalid provider identity"))?;
    let definition = get_provider_definition(provider).unwrap_or(&CUSTOM);
    if definition.id == CUSTOM.id {
        // The custom definition's routes exist to be selected, not inferred.
        return Err(invalid(
            "custom API key providers require explicit protocol and header policy",
        ));
    }
    // Each route keeps its own precise grant: a built-in provider's routes do
    // not all live at one path (DeepSeek's Messages route is under
    // `/anthropic/v1`), and collapsing them into one broader prefix would
    // authorize more of the service than the caller asked for.
    let destinations: Vec<ApiKeyDestination> = definition
        .routes
        .iter()
        .map(|route| {
            api_key_destination(provider, None, Some(route.protocol.as_str()), auth_header)
        })
        .collect::<Result<_, _>>()?;
    // A built-in name identifies a service, so a base URL for one may only
    // name that same service.  The routes themselves stay definition-fixed.
    if let Some(base) = base_url {
        let origin = parse_prefix(base)?.origin().ascii_serialization();
        let expected = destinations
            .first()
            .ok_or_else(|| invalid("provider has no route to authorize"))?
            .grant
            .origin
            .clone();
        if origin != expected {
            return Err(invalid(
                "a builtin provider base URL must address that provider's own service",
            ));
        }
    }
    Ok(destinations)
}

/// Only this module can construct a route. Encoders may inspect, not replace,
/// the destination already checked against the credential's grant.
#[derive(Debug, Clone)]
pub(crate) struct ValidatedRoute {
    profile: String,
    provider: String,
    protocol: Protocol,
    dialect: Dialect,
    url: Url,
    auth: AuthBinding,
    header_policy: AuthHeaderPolicy,
    model: ModelDescriptor,
    capabilities: ProviderCapabilities,
    options: OptionPolicy,
    streaming: bool,
    route_digest: String,
    identity_scope: String,
    config_revision: u64,
    credential_revision: Option<u64>,
    definition_version: u32,
}

impl ValidatedRoute {
    pub(crate) fn profile(&self) -> &str {
        &self.profile
    }
    pub(crate) fn provider(&self) -> &str {
        &self.provider
    }
    pub(crate) fn protocol(&self) -> Protocol {
        self.protocol
    }
    pub(crate) fn dialect(&self) -> Dialect {
        self.dialect
    }
    pub(crate) fn url(&self) -> &str {
        self.url.as_str()
    }
    pub(crate) fn auth(&self) -> &AuthBinding {
        &self.auth
    }
    pub(crate) fn header_policy(&self) -> AuthHeaderPolicy {
        self.header_policy
    }
    pub(crate) fn model(&self) -> &ModelDescriptor {
        &self.model
    }
    pub(crate) fn capabilities(&self) -> ProviderCapabilities {
        self.capabilities
    }
    pub(crate) fn options(&self) -> OptionPolicy {
        self.options
    }
    pub(crate) fn streaming(&self) -> bool {
        self.streaming
    }
    pub(crate) fn route_digest(&self) -> &str {
        &self.route_digest
    }
    pub(crate) fn identity_scope(&self) -> &str {
        &self.identity_scope
    }
    pub(crate) fn config_revision(&self) -> u64 {
        self.config_revision
    }
    pub(crate) fn credential_revision(&self) -> Option<u64> {
        self.credential_revision
    }
    pub(crate) fn definition_version(&self) -> u32 {
        self.definition_version
    }
}

/// Recheck the current credential grant against an immutable prepared route.
pub(crate) fn revalidate_route_auth(
    route: &ValidatedRoute,
    identity: &AuthIdentitySnapshot,
) -> Result<(), BackendError> {
    validate_destination(
        &route.provider,
        route.protocol,
        &route.auth,
        identity,
        &route.url,
        route.header_policy,
    )?;
    if route
        .credential_revision
        .is_none_or(|revision| identity.credential_revision < revision)
        || credential_identity_scope(identity)? != route.identity_scope
    {
        return Err(invalid(
            "credential identity changed after route preparation",
        ));
    }
    Ok(())
}

fn invalid(message: &'static str) -> BackendError {
    BackendError::Configuration(message.into())
}

pub(crate) fn validate_model_routes(routes: &[ModelRoute]) -> Result<(), BackendError> {
    if routes.len() > 128 {
        return Err(invalid("too many model routes"));
    }
    let mut seen = BTreeSet::new();
    for route in routes {
        validate_identity("custom", &route.model)
            .map_err(|_| invalid("invalid model route identity"))?;
        if route.model.contains(['*', '?', '[', ']']) {
            return Err(invalid("model routes do not accept glob patterns"));
        }
        if !seen.insert(&route.model) {
            return Err(invalid("duplicate exact model route"));
        }
        if let Some(wire) = &route.wire_api {
            Protocol::parse(wire)?;
        }
        if let Some(base) = &route.base_url {
            parse_prefix(base)?;
        }
    }
    Ok(())
}

pub(crate) fn resolve_connection(
    connection: &ProviderConnection,
    model_id: &str,
    registry: &ModelRegistry,
    identity: Option<&AuthIdentitySnapshot>,
    streaming: bool,
) -> Result<ValidatedRoute, BackendError> {
    validate_model_routes(&connection.model_routes)?;
    validate_identity(&connection.provider, model_id)
        .map_err(|_| invalid("invalid provider/model identity"))?;
    if connection.profile.is_empty()
        || connection.profile.len() > 128
        || connection.profile.chars().any(char::is_control)
    {
        return Err(invalid("invalid connection profile"));
    }
    let mut selected = connection.clone();
    if let Some(exact) = connection
        .model_routes
        .iter()
        .find(|route| route.model == model_id)
    {
        if let Some(wire) = &exact.wire_api {
            selected.protocol = Protocol::parse(wire)?;
        }
        if let Some(base) = &exact.base_url {
            selected.base_url = Some(base.clone());
        }
    }
    let model = registry
        .resolve(&connection.provider, model_id)
        .map_err(|_| invalid("invalid model metadata"))?;
    let definition = get_provider_definition(&connection.provider).unwrap_or(&CUSTOM);
    resolve_model_route(definition, &selected, &model, identity, streaming)
}

pub(crate) fn resolve_model_route(
    definition: &ProviderDefinition,
    connection: &ProviderConnection,
    model: &ModelDescriptor,
    identity: Option<&AuthIdentitySnapshot>,
    streaming: bool,
) -> Result<ValidatedRoute, BackendError> {
    validate_identity(&connection.provider, &model.id)
        .map_err(|_| invalid("invalid provider/model identity"))?;
    if model.provider != connection.provider
        || (definition.id != CUSTOM.id && definition.id != connection.provider)
    {
        return Err(invalid("model/provider definition mismatch"));
    }
    let rule = service_route(definition, connection.protocol)?;
    let url = final_url(
        rule.endpoint,
        connection.base_url.as_deref(),
        &model.id,
        streaming,
    )?;
    let header = match &connection.auth {
        AuthBinding::Anonymous => {
            if definition.id != CUSTOM.id
                || identity.is_some()
                || connection.header_policy.is_some()
                || url.scheme() != "http"
                || !is_loopback(&url)
            {
                return Err(invalid(
                    "anonymous auth requires an explicit loopback HTTP connection",
                ));
            }
            AuthHeaderPolicy::None
        }
        AuthBinding::LegacyApiKey => {
            return Err(invalid(
                "legacy API key must use the existing legacy backend path",
            ));
        }
        AuthBinding::CodexOAuth { .. } => {
            if definition.id != super::codex::definition().id
                || connection.protocol != Protocol::OpenAiCodexResponses
                || connection.header_policy.is_some()
            {
                return Err(invalid("OAuth is restricted to the fixed Codex service"));
            }
            AuthHeaderPolicy::Codex
        }
        AuthBinding::StoredApiKey { .. } => {
            api_key_header(rule, connection.protocol, connection.header_policy)?
        }
    };
    let identity_scope = if header == AuthHeaderPolicy::None {
        digest(&(connection.provider.as_str(), "anonymous", url.as_str()))?
    } else {
        if url.scheme() != "https" {
            return Err(invalid("credential-bearing destinations require HTTPS"));
        }
        let identity = identity.ok_or_else(|| invalid("credential identity is required"))?;
        validate_destination(
            &connection.provider,
            connection.protocol,
            &connection.auth,
            identity,
            &url,
            header,
        )?;
        credential_identity_scope(identity)?
    };
    let mut capabilities = model.effective_capabilities(rule.capabilities);
    let deepseek_chat =
        rule.dialect == Dialect::DeepSeek && connection.protocol == Protocol::ChatCompletions;
    let mut wire = ProviderCapabilities::for_wire_api(connection.protocol.wire_api());
    if deepseek_chat {
        // DeepSeek extends Chat with reasoning; model and service limits still apply.
        wire.reasoning = rule.capabilities.reasoning;
    }
    capabilities = intersect(capabilities, wire);
    // Legacy catalog fallback is intentionally permissive. Only explicit new
    // connections require per-field catalog or user evidence for rich inputs.
    for (field, supported) in [
        ("images", &mut capabilities.images),
        ("files", &mut capabilities.files),
        ("tools", &mut capabilities.tools),
        ("structured_output", &mut capabilities.structured_output),
        ("reasoning_levels", &mut capabilities.reasoning),
    ] {
        if !matches!(
            model.sources.get(field),
            Some(FieldSource::Builtin { .. } | FieldSource::UserOverride { .. })
        ) {
            *supported = false;
        }
    }
    if streaming && !capabilities.streaming {
        return Err(invalid("selected model does not support streaming"));
    }
    let options = OptionPolicy {
        reasoning_effort: rule.options.reasoning_effort && capabilities.reasoning,
        tool_choice: rule.options.tool_choice && capabilities.tools,
        // JSON mode is independent of JSON Schema support on both DeepSeek APIs.
        response_format: rule.options.response_format
            && (capabilities.structured_output
                || (rule.dialect == Dialect::DeepSeek
                    && matches!(
                        connection.protocol,
                        Protocol::ChatCompletions | Protocol::Responses
                    ))),
        ..rule.options
    };
    let route_digest = digest(&(
        &connection.provider,
        connection.protocol.as_str(),
        url.as_str(),
        &connection.auth,
        header.headers(),
        format!("{:?}", rule.dialect),
        definition.definition_version,
        connection.config_revision,
        model.digest(),
        capabilities,
        streaming,
        (
            options.reasoning_effort,
            options.output_token_limit,
            options.tool_choice,
            options.response_format,
        ),
    ))?;
    Ok(ValidatedRoute {
        profile: connection.profile.clone(),
        provider: connection.provider.clone(),
        protocol: connection.protocol,
        dialect: rule.dialect,
        url,
        auth: connection.auth.clone(),
        header_policy: header,
        model: model.clone(),
        capabilities,
        options,
        streaming,
        route_digest,
        identity_scope,
        config_revision: connection.config_revision,
        credential_revision: identity.map(|identity| identity.credential_revision),
        definition_version: definition.definition_version,
    })
}

fn service_route(
    definition: &ProviderDefinition,
    protocol: Protocol,
) -> Result<&RouteRule, BackendError> {
    let mut rules = definition
        .routes
        .iter()
        .filter(|rule| rule.protocol == protocol);
    let rule = rules
        .next()
        .ok_or_else(|| invalid("unsupported provider protocol"))?;
    if rules.next().is_some() {
        return Err(invalid("ambiguous provider protocol definition"));
    }
    Ok(rule)
}

fn api_key_header(
    rule: &RouteRule,
    protocol: Protocol,
    configured: Option<AuthHeaderPolicy>,
) -> Result<AuthHeaderPolicy, BackendError> {
    if protocol == Protocol::OpenAiCodexResponses {
        return Err(invalid("Codex requires its OAuth binding"));
    }
    let selected = configured.unwrap_or(rule.header_policy);
    if !rule.allowed_headers.contains(&selected) {
        return Err(invalid("header policy is not allowed by the service route"));
    }
    Ok(selected)
}

fn intersect(a: ProviderCapabilities, b: ProviderCapabilities) -> ProviderCapabilities {
    ProviderCapabilities {
        text: a.text && b.text,
        images: a.images && b.images,
        files: a.files && b.files,
        tools: a.tools && b.tools,
        structured_output: a.structured_output && b.structured_output,
        streaming: a.streaming && b.streaming,
        reasoning: a.reasoning && b.reasoning,
    }
}

fn digest(value: &impl Serialize) -> Result<String, BackendError> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid("invalid route metadata"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn credential_identity_scope(identity: &AuthIdentitySnapshot) -> Result<String, BackendError> {
    digest(&(
        &identity.provider,
        &identity.credential_id,
        &identity.account_id,
        &identity.identity_generation,
    ))
}

fn opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn validate_destination(
    provider: &str,
    protocol: Protocol,
    auth: &AuthBinding,
    identity: &AuthIdentitySnapshot,
    url: &Url,
    header: AuthHeaderPolicy,
) -> Result<(), BackendError> {
    let credential_id = match auth {
        AuthBinding::StoredApiKey { credential_id } | AuthBinding::CodexOAuth { credential_id } => {
            credential_id
        }
        _ => return Err(invalid("invalid credential binding")),
    };
    if !opaque_id(credential_id)
        || identity.credential_id != *credential_id
        || identity.provider != provider
        || !opaque_id(&identity.identity_generation)
        || identity.credential_revision == 0
    {
        return Err(invalid("credential identity does not match the connection"));
    }
    if let Some(account) = &identity.account_id
        && (account.is_empty()
            || account.len() > 256
            || account.chars().any(|c| c.is_control() || c.is_whitespace()))
        {
            return Err(invalid("invalid account identity"));
        }
    if matches!(auth, AuthBinding::CodexOAuth { .. }) && identity.account_id.is_none() {
        return Err(invalid("Codex requires an account identity"));
    }
    if identity.allowed_destinations.is_empty() || identity.allowed_destinations.len() > 128 {
        return Err(invalid("credential has no bounded destination grants"));
    }
    let mut allowed = false;
    for grant in &identity.allowed_destinations {
        validate_grant(grant)?;
        allowed |= grant.origin == url.origin().ascii_serialization()
            && path_within(url.path(), &grant.path_prefix)
            && grant.protocols.iter().any(|p| p == protocol.as_str())
            && header
                .headers()
                .iter()
                .all(|name| grant.headers.iter().any(|h| h == name));
    }
    if !allowed {
        return Err(invalid(
            "credential destination/protocol/header scope denied",
        ));
    }
    Ok(())
}

fn validate_grant(grant: &AllowedDestination) -> Result<(), BackendError> {
    let origin = parse_prefix(&grant.origin)?;
    if origin.scheme() != "https"
        || origin.path() != "/"
        || origin.origin().ascii_serialization() != grant.origin
        || !valid_path(&grant.path_prefix)
        || grant.protocols.is_empty()
        || grant.protocols.len() > 5
        || grant.headers.is_empty()
        || grant.headers.len() > 4
    {
        return Err(invalid("invalid credential destination grant"));
    }
    let mut protocols = BTreeSet::new();
    for name in &grant.protocols {
        if Protocol::parse(name)?.as_str() != name || !protocols.insert(name) {
            return Err(invalid("invalid granted protocol"));
        }
    }
    let mut headers = BTreeSet::new();
    for name in &grant.headers {
        if !matches!(
            name.as_str(),
            "authorization" | "x-api-key" | "x-goog-api-key" | "chatgpt-account-id"
        ) || !headers.insert(name)
        {
            return Err(invalid("invalid granted auth header"));
        }
    }
    Ok(())
}

fn path_within(path: &str, prefix: &str) -> bool {
    prefix == "/"
        || path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn valid_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= 2048
        && path.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.' | b'~' | b':')
        })
        && (path == "/"
            || path[1..]
                .split('/')
                .all(|part| !part.is_empty() && part != "." && part != ".."))
}

fn parse_prefix(value: &str) -> Result<Url, BackendError> {
    if value.is_empty()
        || value.len() > 2048
        || !value.is_ascii()
        || value
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        || value.contains(['?', '#', '\\', '%'])
        || !(value.starts_with("https://") || value.starts_with("http://"))
    {
        return Err(invalid("invalid or ambiguous API prefix"));
    }
    let value = value.strip_suffix('/').unwrap_or(value);
    let parsed = Url::parse(value).map_err(|_| invalid("invalid API prefix URL"))?;
    if parsed.host().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !valid_path(parsed.path())
        || parsed.as_str().strip_suffix('/').unwrap_or(parsed.as_str()) != value
    {
        return Err(invalid(
            "API prefix must have canonical authority and unambiguous path",
        ));
    }
    Ok(parsed)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain("localhost")) => true,
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    }
}

fn final_url(
    rule: EndpointRule,
    configured: Option<&str>,
    model: &str,
    streaming: bool,
) -> Result<Url, BackendError> {
    match rule {
        EndpointRule::Fixed(fixed) => {
            // A base URL cannot override a product-specific OAuth destination,
            // even if it happens to contain that product's current URL.
            if configured.is_some() {
                return Err(invalid("fixed service does not accept a base URL override"));
            }
            parse_prefix(fixed)
        }
        EndpointRule::PrefixAndOperation {
            default_prefix,
            canonical_prefix,
            operation,
        } => {
            let mut url = parse_prefix(
                configured
                    .or(default_prefix)
                    .ok_or_else(|| invalid("custom provider requires an explicit API prefix"))?,
            )?;
            if let Some(canonical) = canonical_prefix
                && url != parse_prefix(canonical)? {
                    return Err(invalid(
                        "noncanonical builtin API prefix; use an explicitly scoped custom provider",
                    ));
                }
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| invalid("invalid hierarchical API prefix"))?;
            segments.pop_if_empty();
            match operation {
                EndpointOperation::ChatCompletions => {
                    segments.push("chat").push("completions");
                }
                EndpointOperation::Responses => {
                    segments.push("responses");
                }
                EndpointOperation::Messages => {
                    segments.push("messages");
                }
                EndpointOperation::GoogleGenerateContent => {
                    if model.is_empty()
                        || model.len() > 128
                        || model == "."
                        || model == ".."
                        || !model
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                    {
                        return Err(invalid(
                            "Google requires a bounded model ID without URL components",
                        ));
                    }
                    let operation = if streaming {
                        "streamGenerateContent"
                    } else {
                        "generateContent"
                    };
                    segments
                        .push("models")
                        .push(&format!("{model}:{operation}"));
                }
            }
            drop(segments);
            if matches!(operation, EndpointOperation::GoogleGenerateContent) && streaming {
                url.query_pairs_mut().append_pair("alt", "sse");
            }
            if url.as_str().len() > 2048 {
                return Err(invalid("complete request URL exceeds limit"));
            }
            Ok(url)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::registry::ModelOverride;

    fn connection(provider: &str, protocol: Protocol) -> ProviderConnection {
        ProviderConnection {
            profile: "test-profile".into(),
            provider: provider.into(),
            protocol,
            base_url: None,
            auth: if provider == "openai-codex" {
                AuthBinding::CodexOAuth {
                    credential_id: "cred_test".into(),
                }
            } else {
                AuthBinding::StoredApiKey {
                    credential_id: "cred_test".into(),
                }
            },
            header_policy: None,
            config_revision: 1,
            model_routes: vec![],
        }
    }

    fn identity(provider: &str, origin: &str, path: &str) -> AuthIdentitySnapshot {
        AuthIdentitySnapshot {
            provider: provider.into(),
            credential_id: "cred_test".into(),
            account_id: Some("account_test".into()),
            identity_generation: "generation_1".into(),
            credential_revision: 1,
            allowed_destinations: vec![AllowedDestination {
                origin: origin.into(),
                path_prefix: path.into(),
                protocols: [
                    Protocol::ChatCompletions,
                    Protocol::Responses,
                    Protocol::AnthropicMessages,
                    Protocol::GoogleGenerativeAi,
                    Protocol::OpenAiCodexResponses,
                ]
                .map(|p| p.as_str().into())
                .into(),
                headers: [
                    "authorization",
                    "x-api-key",
                    "x-goog-api-key",
                    "chatgpt-account-id",
                ]
                .map(String::from)
                .into(),
            }],
        }
    }

    fn resolve(
        c: &ProviderConnection,
        id: Option<&AuthIdentitySnapshot>,
    ) -> Result<ValidatedRoute, BackendError> {
        resolve_connection(c, "test-model", &ModelRegistry::default(), id, true)
    }

    fn gateway(base: &str) -> ProviderConnection {
        let mut c = connection("gateway", Protocol::Responses);
        c.base_url = Some(base.into());
        c
    }

    #[test]
    fn builtin_routes_freeze_complete_urls_and_headers() {
        for (provider, protocol, origin, expected, header) in [
            (
                "openai",
                Protocol::ChatCompletions,
                "https://api.openai.com",
                "https://api.openai.com/v1/chat/completions",
                AuthHeaderPolicy::Bearer,
            ),
            (
                "openai",
                Protocol::Responses,
                "https://api.openai.com",
                "https://api.openai.com/v1/responses",
                AuthHeaderPolicy::Bearer,
            ),
            (
                "deepseek",
                Protocol::ChatCompletions,
                "https://api.deepseek.com",
                "https://api.deepseek.com/chat/completions",
                AuthHeaderPolicy::Bearer,
            ),
            (
                "deepseek",
                Protocol::Responses,
                "https://api.deepseek.com",
                "https://api.deepseek.com/responses",
                AuthHeaderPolicy::Bearer,
            ),
            (
                "deepseek",
                Protocol::AnthropicMessages,
                "https://api.deepseek.com",
                "https://api.deepseek.com/anthropic/v1/messages",
                AuthHeaderPolicy::XApiKey,
            ),
            (
                "anthropic",
                Protocol::AnthropicMessages,
                "https://api.anthropic.com",
                "https://api.anthropic.com/v1/messages",
                AuthHeaderPolicy::XApiKey,
            ),
            (
                "google",
                Protocol::GoogleGenerativeAi,
                "https://generativelanguage.googleapis.com",
                "https://generativelanguage.googleapis.com/v1beta/models/test-model:streamGenerateContent?alt=sse",
                AuthHeaderPolicy::GoogleApiKey,
            ),
        ] {
            let c = connection(provider, protocol);
            let route = resolve(&c, Some(&identity(provider, origin, "/"))).unwrap();
            assert_eq!(route.url(), expected);
            assert_eq!(route.header_policy(), header);
            assert_eq!(route.provider(), provider);
            assert_eq!(route.profile(), "test-profile");
            assert_eq!(route.config_revision(), 1);
            assert_eq!(route.definition_version(), 1);
        }
    }

    #[test]
    fn codex_requires_fixed_product_oauth_and_both_headers() {
        let c = connection("openai-codex", Protocol::OpenAiCodexResponses);
        let endpoint = Url::parse(crate::providers::codex::RESPONSES_ENDPOINT).unwrap();
        let mut id = identity(&c.provider, &endpoint.origin().ascii_serialization(), "/");
        let route = resolve(&c, Some(&id)).unwrap();
        assert_eq!(route.url(), endpoint.as_str());
        assert_eq!(route.dialect(), Dialect::Codex);
        assert!(!route.options().output_token_limit);
        assert_eq!(
            route.header_policy().headers(),
            ["authorization", "chatgpt-account-id"]
        );
        id.allowed_destinations[0]
            .headers
            .retain(|h| h != "chatgpt-account-id");
        assert!(resolve(&c, Some(&id)).is_err());
        id.allowed_destinations[0]
            .headers
            .push("chatgpt-account-id".into());
        id.account_id = None;
        assert!(resolve(&c, Some(&id)).is_err());
    }

    #[test]
    fn oauth_cannot_change_endpoint_protocol_or_header_policy() {
        let mut c = connection("openai-codex", Protocol::OpenAiCodexResponses);
        let endpoint = Url::parse(crate::providers::codex::RESPONSES_ENDPOINT).unwrap();
        let id = identity(&c.provider, &endpoint.origin().ascii_serialization(), "/");
        c.base_url = Some(endpoint.into());
        assert!(resolve(&c, Some(&id)).is_err());
        c.base_url = None;
        c.header_policy = Some(AuthHeaderPolicy::Codex);
        assert!(resolve(&c, Some(&id)).is_err());
        c.header_policy = None;
        c.protocol = Protocol::Responses;
        assert!(resolve(&c, Some(&id)).is_err());
        c.provider = "openai".into();
        assert!(resolve(&c, Some(&identity("openai", "https://api.openai.com", "/"))).is_err());
        c = connection("openai-codex", Protocol::OpenAiCodexResponses);
        c.auth = AuthBinding::StoredApiKey {
            credential_id: "cred_test".into(),
        };
        assert!(resolve(&c, Some(&id)).is_err());
    }

    #[test]
    fn deepseek_has_no_implicit_v1_responses_alias() {
        let mut c = connection("deepseek", Protocol::Responses);
        let id = identity("deepseek", "https://api.deepseek.com", "/");
        for base in [
            "https://api.deepseek.com/v1",
            "https://api.deepseek.com/responses",
            "https://api.deepseek.com/v1/responses",
        ] {
            c.base_url = Some(base.into());
            assert!(resolve(&c, Some(&id)).is_err(), "{base}");
        }
        c = gateway("https://gateway.test/v1");
        let route = resolve(
            &c,
            Some(&identity("gateway", "https://gateway.test", "/v1")),
        )
        .unwrap();
        assert_eq!(route.url(), "https://gateway.test/v1/responses");
    }

    #[test]
    fn prefixes_reject_ambiguity_before_parser_normalization() {
        for base in [
            "https://user:secret@gateway.test/v1",
            "https://@gateway.test/v1",
            "https://gateway.test/v1?key=secret",
            "https://gateway.test/v1?",
            "https://gateway.test/v1#fragment",
            "https://gateway.test/a/../v1",
            "https://gateway.test/./v1",
            "https://gateway.test/%2e/v1",
            "https://gateway.test/v1%2fextra",
            "https://gateway.test/v1%5cextra",
            "https://gateway.test/v1%252fextra",
            "https://gateway.test/v1//",
            "https://gateway.test\\evil/v1",
            "https://gateway.test/v1\n",
            " https://gateway.test/v1",
            "https://gateway.test:443/v1",
            "http://127.1/v1",
            "ftp://gateway.test/v1",
            "https:gateway.test/v1",
            "https://gateway.test/路径",
            "https:///gateway.test/v1",
        ] {
            assert!(parse_prefix(base).is_err(), "accepted {base:?}");
        }
    }

    #[test]
    fn only_a_single_trailing_prefix_slash_is_normalized() {
        let id = identity("gateway", "https://gateway.test", "/v1");
        let a = resolve(&gateway("https://gateway.test/v1"), Some(&id)).unwrap();
        let b = resolve(&gateway("https://gateway.test/v1/"), Some(&id)).unwrap();
        assert_eq!(a.url(), b.url());
        assert_eq!(a.route_digest(), b.route_digest());
        assert!(resolve(&gateway("https://gateway.test/v1//"), Some(&id)).is_err());
    }

    #[test]
    fn scoped_paths_match_segment_boundaries_not_string_prefixes() {
        let id = identity("gateway", "https://gateway.test", "/v1");
        assert!(resolve(&gateway("https://gateway.test/v1"), Some(&id)).is_ok());
        assert!(resolve(&gateway("https://gateway.test/v1/nested"), Some(&id)).is_ok());
        for base in [
            "https://gateway.test/v10",
            "https://gateway.test/v1evil",
            "https://gateway.test/other",
        ] {
            assert!(resolve(&gateway(base), Some(&id)).is_err());
        }
    }

    #[test]
    fn destination_origin_and_port_are_exact() {
        let id = identity("gateway", "https://gateway.test", "/");
        for base in [
            "https://gateway.test.evil/v1",
            "https://gateway.test:8443/v1",
            "https://evil.test/v1",
            "http://gateway.test/v1",
        ] {
            assert!(resolve(&gateway(base), Some(&id)).is_err());
        }
    }

    #[test]
    fn protocol_and_header_must_be_granted_together() {
        let c = gateway("https://gateway.test/v1");
        let mut id = identity("gateway", "https://gateway.test", "/v1");
        id.allowed_destinations[0].protocols = vec!["chat_completions".into()];
        assert!(resolve(&c, Some(&id)).is_err());
        id.allowed_destinations[0].protocols = vec!["responses".into()];
        id.allowed_destinations[0].headers = vec!["x-api-key".into()];
        assert!(resolve(&c, Some(&id)).is_err());
        let mut c = c;
        c.header_policy = Some(AuthHeaderPolicy::XApiKey);
        assert_eq!(
            resolve(&c, Some(&id)).unwrap().header_policy(),
            AuthHeaderPolicy::XApiKey
        );
        let mut native = connection("openai", Protocol::Responses);
        native.header_policy = Some(AuthHeaderPolicy::XApiKey);
        assert!(
            resolve(
                &native,
                Some(&identity("openai", "https://api.openai.com", "/"))
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_grants_fail_closed_even_beside_a_valid_grant() {
        let c = gateway("https://gateway.test/v1");
        let original = identity("gateway", "https://gateway.test", "/v1");
        for change in 0..6 {
            let mut id = original.clone();
            let mut invalid = id.allowed_destinations[0].clone();
            match change {
                0 => invalid.origin.push_str("/v1"),
                1 => invalid.path_prefix = "/v1/../other".into(),
                2 => invalid.headers = vec!["Authorization".into()],
                3 => invalid.protocols = vec!["chat".into()],
                4 => invalid.path_prefix = "/v1%2fextra".into(),
                _ => invalid.protocols.clear(),
            }
            id.allowed_destinations.push(invalid);
            assert!(resolve(&c, Some(&id)).is_err());
        }
    }

    #[test]
    fn credential_snapshot_must_match_binding_and_provider() {
        let c = gateway("https://gateway.test/v1");
        let original = identity("gateway", "https://gateway.test", "/v1");
        assert!(resolve(&c, None).is_err());
        for change in 0..5 {
            let mut id = original.clone();
            match change {
                0 => id.provider = "other".into(),
                1 => id.credential_id = "other".into(),
                2 => id.identity_generation.clear(),
                3 => id.credential_revision = 0,
                _ => id.allowed_destinations.clear(),
            }
            assert!(resolve(&c, Some(&id)).is_err());
        }
    }

    #[test]
    fn anonymous_is_explicit_loopback_http_only() {
        for base in [
            "http://localhost:8000/v1",
            "http://127.0.0.1:8000/v1",
            "http://[::1]:8000/v1",
        ] {
            let mut c = gateway(base);
            c.auth = AuthBinding::Anonymous;
            assert_eq!(
                resolve(&c, None).unwrap().header_policy(),
                AuthHeaderPolicy::None
            );
        }
        for base in [
            "http://example.test/v1",
            "http://localhost.evil/v1",
            "https://localhost/v1",
            "http://0.0.0.0:8000/v1",
        ] {
            let mut c = gateway(base);
            c.auth = AuthBinding::Anonymous;
            assert!(resolve(&c, None).is_err());
        }
        let mut c = gateway("http://localhost:8000/v1");
        c.auth = AuthBinding::Anonymous;
        c.header_policy = Some(AuthHeaderPolicy::Bearer);
        assert!(resolve(&c, None).is_err());
        c.header_policy = None;
        assert!(resolve(&c, Some(&identity("gateway", "https://gateway.test", "/"))).is_err());
        c.base_url = None;
        assert!(resolve(&c, None).is_err());
    }

    #[test]
    fn stored_keys_never_use_loopback_http_and_legacy_is_not_reinterpreted() {
        let mut c = gateway("http://localhost:8000/v1");
        assert!(resolve(&c, Some(&identity("gateway", "https://gateway.test", "/"))).is_err());
        c.auth = AuthBinding::LegacyApiKey;
        assert!(resolve(&c, None).is_err());
        assert_eq!(
            Protocol::from(crate::backend::OpenAiWireApi::default()),
            Protocol::ChatCompletions
        );
    }

    #[test]
    fn exact_model_routes_override_wire_without_rebinding_credentials() {
        let mut c = connection("deepseek", Protocol::ChatCompletions);
        c.model_routes.push(ModelRoute {
            model: "selected".into(),
            wire_api: Some("responses".into()),
            base_url: None,
        });
        let id = identity("deepseek", "https://api.deepseek.com", "/");
        for (name, expected) in [
            ("selected", Protocol::Responses),
            ("SELECTED", Protocol::ChatCompletions),
            ("other", Protocol::ChatCompletions),
        ] {
            let route =
                resolve_connection(&c, name, &ModelRegistry::default(), Some(&id), true).unwrap();
            assert_eq!(route.protocol(), expected);
            assert_eq!(route.auth(), &c.auth);
            assert_eq!(route.model().id, name);
        }
    }

    #[test]
    fn model_route_dto_rejects_unknown_fields_and_invalid_or_duplicate_rules() {
        let route: ModelRoute = serde_json::from_str(r#"{"model":"exact"}"#).unwrap();
        assert!(route.wire_api.is_none());
        assert!(route.base_url.is_none());
        assert!(
            serde_json::from_str::<ModelRoute>(r#"{"model":"exact","auth_ref":"other"}"#).is_err()
        );
        assert!(validate_model_routes(&[route.clone(), route.clone()]).is_err());
        assert!(validate_model_routes(&vec![route.clone(); 129]).is_err());
        for name in ["", "white space", "wild*", "wild?", "[models]"] {
            assert!(
                validate_model_routes(&[ModelRoute {
                    model: name.into(),
                    ..route.clone()
                }])
                .is_err()
            );
        }
        assert!(
            validate_model_routes(&[ModelRoute {
                wire_api: Some("unknown".into()),
                ..route.clone()
            }])
            .is_err()
        );
        assert!(
            validate_model_routes(&[ModelRoute {
                base_url: Some("https://host/v1?key=secret".into()),
                ..route
            }])
            .is_err()
        );
    }

    #[test]
    fn duplicate_nonselected_model_routes_are_not_ignored() {
        let mut c = gateway("https://gateway.test/v1");
        c.model_routes = vec![
            ModelRoute {
                model: "other".into(),
                wire_api: None,
                base_url: None
            };
            2
        ];
        assert!(resolve(&c, Some(&identity("gateway", "https://gateway.test", "/"))).is_err());
    }

    #[test]
    fn google_operation_is_frozen_before_destination_authentication() {
        let c = connection("google", Protocol::GoogleGenerativeAi);
        let id = identity(
            "google",
            "https://generativelanguage.googleapis.com",
            "/v1beta/models",
        );
        let route = resolve_connection(
            &c,
            "gemini-2.5-flash",
            &ModelRegistry::default(),
            Some(&id),
            false,
        )
        .unwrap();
        assert_eq!(
            route.url(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent"
        );
        assert!(!route.streaming());
        for model in ["evil/key", "evil?key", "evil#fragment", "..", "%2f"] {
            let error = resolve_connection(&c, model, &ModelRegistry::default(), None, true)
                .unwrap_err()
                .to_string();
            assert!(error.contains("Google"), "{error}");
        }
    }

    #[test]
    fn explicit_unknown_capabilities_are_conservative_without_changing_legacy_catalog() {
        let registry = ModelRegistry::default();
        let c = gateway("https://gateway.test/v1");
        let id = identity("gateway", "https://gateway.test", "/v1");
        assert!(
            registry
                .resolve("gateway", "unknown")
                .unwrap()
                .capabilities
                .images
        );
        let caps = resolve_connection(&c, "unknown", &registry, Some(&id), true)
            .unwrap()
            .capabilities();
        assert!(caps.text && caps.streaming);
        assert!(
            !caps.images
                && !caps.files
                && !caps.tools
                && !caps.structured_output
                && !caps.reasoning
        );
    }

    #[test]
    fn known_or_explicit_model_capabilities_still_intersect_with_wire_and_service() {
        let c = connection("openai", Protocol::Responses);
        let id = identity("openai", "https://api.openai.com", "/v1");
        let registry = ModelRegistry::default();
        let route = resolve_connection(&c, "gpt-4.1", &registry, Some(&id), true).unwrap();
        assert!(
            route.capabilities().images && route.capabilities().files && route.capabilities().tools
        );
        let c = ProviderConnection {
            protocol: Protocol::ChatCompletions,
            ..c
        };
        assert!(
            !resolve_connection(&c, "gpt-4.1", &registry, Some(&id), true)
                .unwrap()
                .capabilities()
                .files
        );
        let model: ModelOverride = serde_json::from_value(serde_json::json!({
            "provider":"deepseek", "id":"synthetic", "version":"test", "images":true,
            "files":true, "tools":true, "structured_output":true
        }))
        .unwrap();
        let registry = ModelRegistry::with_overrides(&[model]).unwrap();
        let c = connection("deepseek", Protocol::Responses);
        let route = resolve_connection(
            &c,
            "synthetic",
            &registry,
            Some(&identity("deepseek", "https://api.deepseek.com", "/")),
            true,
        )
        .unwrap();
        assert!(route.capabilities().images && route.capabilities().tools);
        assert!(!route.capabilities().files);
        assert!(route.capabilities().structured_output);
        assert_eq!(route.dialect(), Dialect::DeepSeek);
    }

    #[test]
    fn deepseek_chat_preserves_reasoning_and_json_mode_without_granting_json_schema() {
        let model: ModelOverride = serde_json::from_value(serde_json::json!({
            "provider":"deepseek", "id":"synthetic", "version":"test",
            "structured_output":true, "reasoning_levels":["high"]
        }))
        .unwrap();
        let registry = ModelRegistry::with_overrides(&[model]).unwrap();
        let c = connection("deepseek", Protocol::ChatCompletions);
        let id = identity("deepseek", "https://api.deepseek.com", "/");
        let route = resolve_connection(&c, "synthetic", &registry, Some(&id), true).unwrap();
        assert!(route.capabilities().reasoning);
        assert!(route.options().reasoning_effort);
        assert!(route.options().response_format);
        assert!(!route.capabilities().structured_output);
        let unknown = resolve_connection(&c, "unknown", &registry, Some(&id), true).unwrap();
        assert!(!unknown.capabilities().reasoning);
        assert!(!unknown.options().reasoning_effort);
        assert!(unknown.options().response_format);
        assert!(!unknown.capabilities().structured_output);
    }

    #[test]
    fn deepseek_chat_extensions_do_not_expand_generic_chat_capabilities() {
        let model: ModelOverride = serde_json::from_value(serde_json::json!({
            "provider":"gateway", "id":"synthetic", "version":"test",
            "structured_output":false, "reasoning_levels":["high"]
        }))
        .unwrap();
        let registry = ModelRegistry::with_overrides(&[model]).unwrap();
        let mut c = gateway("https://gateway.test/v1");
        c.protocol = Protocol::ChatCompletions;
        let id = identity("gateway", "https://gateway.test", "/v1");
        let route = resolve_connection(&c, "synthetic", &registry, Some(&id), true).unwrap();
        assert!(!route.capabilities().reasoning);
        assert!(!route.options().reasoning_effort);
        assert!(!route.options().response_format);
        assert!(!route.capabilities().structured_output);
    }

    #[test]
    fn refresh_revision_does_not_change_route_or_identity_scope() {
        let c = gateway("https://gateway.test/v1");
        let mut id = identity("gateway", "https://gateway.test", "/v1");
        let before = resolve(&c, Some(&id)).unwrap();
        id.credential_revision += 1;
        let refreshed = resolve(&c, Some(&id)).unwrap();
        assert_eq!(before.route_digest(), refreshed.route_digest());
        assert_eq!(before.identity_scope(), refreshed.identity_scope());
        assert_eq!(refreshed.credential_revision(), Some(2));
        id.identity_generation = "generation_2".into();
        let replaced = resolve(&c, Some(&id)).unwrap();
        assert_ne!(before.identity_scope(), replaced.identity_scope());
        id.account_id = Some("other_account".into());
        assert_ne!(
            replaced.identity_scope(),
            resolve(&c, Some(&id)).unwrap().identity_scope()
        );
    }

    #[test]
    fn revalidation_accepts_same_identity_refresh_without_mutating_the_route() {
        let c = gateway("https://gateway.test/v1");
        let mut id = identity("gateway", "https://gateway.test", "/v1");
        let route = resolve(&c, Some(&id)).unwrap();
        let before = format!("{route:?}");
        revalidate_route_auth(&route, &id).unwrap();
        id.credential_revision += 1;
        revalidate_route_auth(&route, &id).unwrap();
        assert_eq!(format!("{route:?}"), before);
        assert_eq!(route.credential_revision(), Some(1));
    }

    #[test]
    fn revalidation_rejects_withdrawn_destination_protocol_and_header_grants() {
        let c = gateway("https://gateway.test/v1");
        let id = identity("gateway", "https://gateway.test", "/v1");
        let route = resolve(&c, Some(&id)).unwrap();
        for change in ["all", "origin", "path", "protocol", "header"] {
            let mut latest = id.clone();
            latest.credential_revision += 1;
            match change {
                "all" => latest.allowed_destinations.clear(),
                "origin" => latest.allowed_destinations[0].origin = "https://other.test".into(),
                "path" => latest.allowed_destinations[0].path_prefix = "/v1/responses-other".into(),
                "protocol" => latest.allowed_destinations[0].protocols = vec!["chat".into()],
                "header" => latest.allowed_destinations[0].headers = vec!["x-api-key".into()],
                _ => unreachable!(),
            }
            assert!(revalidate_route_auth(&route, &latest).is_err(), "{change}");
        }
    }

    #[test]
    fn revalidation_rejects_account_credential_provider_and_login_generation_changes() {
        let c = gateway("https://gateway.test/v1");
        let id = identity("gateway", "https://gateway.test", "/v1");
        let route = resolve(&c, Some(&id)).unwrap();
        for change in [
            "account",
            "no_account",
            "credential",
            "provider",
            "generation",
        ] {
            let mut latest = id.clone();
            latest.credential_revision += 1;
            match change {
                "account" => latest.account_id = Some("other_account".into()),
                "no_account" => latest.account_id = None,
                "credential" => latest.credential_id = "other_credential".into(),
                "provider" => latest.provider = "other_provider".into(),
                "generation" => latest.identity_generation = "new_login".into(),
                _ => unreachable!(),
            }
            assert!(revalidate_route_auth(&route, &latest).is_err(), "{change}");
        }
    }

    #[test]
    fn revalidation_rejects_credential_revision_rollback() {
        let c = gateway("https://gateway.test/v1");
        let mut id = identity("gateway", "https://gateway.test", "/v1");
        id.credential_revision = 4;
        let route = resolve(&c, Some(&id)).unwrap();
        for revision in [0, 1, 3] {
            id.credential_revision = revision;
            assert!(revalidate_route_auth(&route, &id).is_err());
        }
    }

    #[test]
    fn codex_revalidation_keeps_the_fixed_service_and_both_auth_headers() {
        let c = connection("openai-codex", Protocol::OpenAiCodexResponses);
        let endpoint = Url::parse(crate::providers::codex::RESPONSES_ENDPOINT).unwrap();
        let id = identity(
            &c.provider,
            &endpoint.origin().ascii_serialization(),
            endpoint.path(),
        );
        let route = resolve(&c, Some(&id)).unwrap();
        let mut latest = id.clone();
        latest.credential_revision += 1;
        revalidate_route_auth(&route, &latest).unwrap();
        assert_eq!(route.url(), endpoint.as_str());
        latest.allowed_destinations[0]
            .headers
            .retain(|header| header != "chatgpt-account-id");
        assert!(revalidate_route_auth(&route, &latest).is_err());
        latest = id.clone();
        latest.allowed_destinations[0].origin = "https://api.openai.com".into();
        assert!(revalidate_route_auth(&route, &latest).is_err());
        latest = id;
        latest.account_id = None;
        assert!(revalidate_route_auth(&route, &latest).is_err());
    }

    #[test]
    fn anonymous_route_cannot_be_revalidated_with_a_credential_identity() {
        let mut c = gateway("http://localhost:8000/v1");
        c.auth = AuthBinding::Anonymous;
        let route = resolve(&c, None).unwrap();
        let id = identity("gateway", "https://gateway.test", "/v1");
        assert!(revalidate_route_auth(&route, &id).is_err());
    }

    #[test]
    fn destination_and_configuration_changes_change_route_digest() {
        let mut c = gateway("https://gateway.test/v1");
        let id = identity("gateway", "https://gateway.test", "/");
        let first = resolve(&c, Some(&id)).unwrap();
        c.base_url = Some("https://gateway.test/other".into());
        let second = resolve(&c, Some(&id)).unwrap();
        assert_ne!(first.route_digest(), second.route_digest());
        c.config_revision += 1;
        assert_ne!(
            second.route_digest(),
            resolve(&c, Some(&id)).unwrap().route_digest()
        );
    }

    #[test]
    fn google_grants_can_restrict_the_final_model_and_operation() {
        let c = connection("google", Protocol::GoogleGenerativeAi);
        let id = identity(
            "google",
            "https://generativelanguage.googleapis.com",
            "/v1beta/models/test-model:streamGenerateContent",
        );
        assert!(resolve(&c, Some(&id)).is_ok());
        assert!(
            resolve_connection(
                &c,
                "another-model",
                &ModelRegistry::default(),
                Some(&id),
                true
            )
            .is_err()
        );
        assert!(
            resolve_connection(
                &c,
                "test-model",
                &ModelRegistry::default(),
                Some(&id),
                false
            )
            .is_err()
        );
    }

    #[test]
    fn anonymous_endpoint_changes_do_not_reuse_identity_scope() {
        let mut c = gateway("http://localhost:8000/first");
        c.auth = AuthBinding::Anonymous;
        let first = resolve(&c, None).unwrap();
        c.base_url = Some("http://localhost:8000/second".into());
        assert_ne!(
            first.identity_scope(),
            resolve(&c, None).unwrap().identity_scope()
        );
    }

    #[test]
    fn malformed_endpoint_errors_do_not_echo_embedded_secrets() {
        let c = gateway("https://user:synthetic-secret@gateway.test/v1");
        let error = resolve(&c, None).unwrap_err().to_string();
        assert!(!error.contains("synthetic-secret"));
        assert!(!error.contains("user:"));
    }

    /// A grant minted at login and the route built for a request come from the
    /// same service policy.  If they ever drift, a freshly added credential
    /// would be unable to reach the endpoint it was created for.
    #[test]
    fn login_grants_admit_the_request_route_of_every_builtin_definition() {
        let mut checked = 0;
        for provider in ["openai", "deepseek", "anthropic", "google"] {
            let definition = get_provider_definition(provider).unwrap();
            for rule in definition.routes {
                let wire = rule.protocol.as_str();
                let destination = api_key_destination(provider, None, Some(wire), None).unwrap();
                let mut connection = connection(provider, rule.protocol);
                connection.header_policy = None;
                let identity = AuthIdentitySnapshot {
                    provider: provider.into(),
                    credential_id: "cred_test".into(),
                    account_id: Some("account_test".into()),
                    identity_generation: "generation_1".into(),
                    credential_revision: 1,
                    allowed_destinations: vec![destination.grant.clone()],
                };
                let route = resolve_connection(
                    &connection,
                    "test-model",
                    &ModelRegistry::default(),
                    Some(&identity),
                    true,
                )
                .unwrap_or_else(|error| {
                    panic!("{provider}/{wire} login grant rejects its own route: {error}")
                });
                // The grant carries exactly the header the request will use.
                assert_eq!(destination.grant.headers, route.header_policy().headers());
                assert_eq!(
                    destination.definition_version,
                    definition.definition_version
                );
                checked += 1;
            }
            match definition.routes {
                // A single-route provider must not require an explicit wire.
                [only] => {
                    let inferred = api_key_destination(provider, None, None, None).unwrap();
                    assert_eq!(inferred.protocol, only.protocol);
                    let explicit =
                        api_key_destination(provider, None, Some(only.protocol.as_str()), None)
                            .unwrap();
                    assert_eq!(inferred.grant, explicit.grant);
                }
                // A provider with several routes cannot guess which one a key
                // is for, so the caller has to name it.
                routes => {
                    assert!(routes.len() > 1);
                    let error = api_key_destination(provider, None, None, None)
                        .unwrap_err()
                        .to_string();
                    assert!(error.contains("explicit protocol"), "{provider}: {error}");
                }
            }
        }
        assert_eq!(checked, 7, "every built-in route must be covered");
    }

    /// Google selects the model and the generation operation after login, so
    /// its grant stops at the `models` subtree rather than naming one model.
    #[test]
    fn google_grants_stop_at_the_models_subtree() {
        let destination = api_key_destination("google", None, None, None).unwrap();
        assert_eq!(
            destination.grant.origin,
            "https://generativelanguage.googleapis.com"
        );
        // Segment-boundary matching means the subtree covers every model and
        // operation below it without naming one.
        assert_eq!(destination.grant.path_prefix, "/v1beta/models");
        assert_eq!(destination.grant.protocols, ["google_generative_ai"]);
        assert_eq!(destination.grant.headers, ["x-goog-api-key"]);
    }

    #[test]
    fn login_grants_refuse_codex_and_unscoped_custom_providers() {
        let error = api_key_destination("openai-codex", None, None, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("OAuth binding"), "{error}");

        let error = api_key_destination(
            "custom",
            Some("https://gateway.test/v1"),
            Some("responses"),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("explicit protocol and header policy"),
            "{error}"
        );

        let error = api_key_destination(
            "custom",
            Some("https://gateway.test/v1"),
            None,
            Some("bearer"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("explicit protocol and header policy"),
            "{error}"
        );

        // A custom provider has several routes, so the protocol is required.
        let error = api_key_destination("custom", Some("https://gateway.test/v1"), None, None)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("explicit protocol and header policy"),
            "{error}"
        );

        let error = api_key_destination("no such provider!", None, None, None)
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid provider identity"), "{error}");
    }

    #[test]
    fn login_grants_require_https_and_a_route_allowed_header() {
        let error = api_key_destination(
            "custom",
            Some("http://gateway.test/v1"),
            Some("responses"),
            Some("bearer"),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("require HTTPS"), "{error}");

        // openai only allows the bearer header on its routes.
        let error = api_key_destination("openai", None, Some("responses"), Some("x_api_key"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not allowed by the service route"),
            "{error}"
        );

        let error = api_key_destination("openai", None, Some("responses"), Some("google_api_key"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("not allowed by the service route"),
            "{error}"
        );

        // An unknown header name is rejected before any route is consulted.
        let error = api_key_destination("openai", None, Some("responses"), Some("authorization"))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("unsupported API key header policy"),
            "{error}"
        );

        // A custom provider may pick any header its own route allows.
        let destination = api_key_destination(
            "custom",
            Some("https://gateway.test/v1"),
            Some("responses"),
            Some("x_api_key"),
        )
        .unwrap();
        assert_eq!(destination.grant.headers, ["x-api-key"]);
        assert_eq!(destination.grant.origin, "https://gateway.test");
        assert_eq!(destination.grant.path_prefix, "/v1/responses");
    }
}
