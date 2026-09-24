//! Fixed-endpoint Codex OAuth. No credential discovery, browser shell, or API-key exchange.
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use super::callback::{CallbackServer, ManualSubmission};
use super::store::{
    CredentialKind, LockWait, Mutation, PendingCredential, RefreshTokens, Replacement,
};
use super::*;
use crate::backend::{BackendError, transport};
use crate::providers::codex as definition;

const MAX_RESPONSE: usize = 256 * 1024;
const MAX_TOKEN: usize = 16 * 1024;
const PROMPT_ID: u64 = 1;

pub(crate) struct BrowserLogin {
    flow_id: LoginFlowId,
    verifier: String,
    authorization_url: String,
    submission: ManualSubmission,
}
impl BrowserLogin {
    pub(crate) fn flow_id(&self) -> &LoginFlowId {
        &self.flow_id
    }
}
impl Drop for BrowserLogin {
    fn drop(&mut self) {
        self.submission.close();
    }
}
pub(crate) struct DeviceLogin {
    flow_id: LoginFlowId,
}
impl DeviceLogin {
    pub(crate) fn flow_id(&self) -> &LoginFlowId {
        &self.flow_id
    }
}

fn random_value() -> Result<String, AuthError> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|_| AuthError::RandomnessUnavailable)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn begin_browser_login() -> Result<BrowserLogin, AuthError> {
    let verifier = random_value()?;
    let state = random_value()?;
    let flow_id = LoginFlowId(random_value()?);
    let mut url = Url::parse(definition::AUTHORIZE_ENDPOINT).map_err(|_| AuthError::LoginFailed)?;
    url.query_pairs_mut().extend_pairs([
        ("response_type", "code"),
        ("client_id", definition::CLIENT_ID),
        ("scope", definition::SCOPE),
        ("redirect_uri", definition::REDIRECT_URI),
        ("code_challenge_method", "S256"),
        (
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        ),
        ("state", &state),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("originator", definition::ORIGINATOR),
    ]);
    let submission = ManualSubmission::new(flow_id.clone(), PROMPT_ID, state);
    Ok(BrowserLogin {
        flow_id,
        verifier,
        authorization_url: url.into(),
        submission,
    })
}

pub(crate) fn begin_device_login() -> Result<DeviceLogin, AuthError> {
    Ok(DeviceLogin {
        flow_id: LoginFlowId(random_value()?),
    })
}

struct ActiveEntry {
    id: LoginFlowId,
    cancelled: Weak<AtomicBool>,
}
static ACTIVE: Mutex<Option<ActiveEntry>> = Mutex::new(None);
struct ActiveLogin {
    id: LoginFlowId,
    cancelled: Arc<AtomicBool>,
}
impl ActiveLogin {
    fn acquire(id: &LoginFlowId) -> Result<Self, AuthError> {
        let mut active = ACTIVE.lock().map_err(|_| AuthError::LoginFailed)?;
        if active
            .as_ref()
            .is_some_and(|entry| entry.cancelled.upgrade().is_some())
        {
            return Err(AuthError::LoginBusy);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveEntry {
            id: id.clone(),
            cancelled: Arc::downgrade(&cancelled),
        });
        Ok(Self {
            id: id.clone(),
            cancelled,
        })
    }
}
impl Drop for ActiveLogin {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock()
            && active.as_ref().is_some_and(|entry| entry.id == self.id) {
                *active = None;
            }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CancelDisposition {
    Requested,
    NotActive,
}
pub(crate) fn cancel_login(flow_id: &LoginFlowId) -> CancelDisposition {
    if let Ok(active) = ACTIVE.lock()
        && let Some(cancelled) = active
            .as_ref()
            .filter(|entry| entry.id == *flow_id)
            .and_then(|entry| entry.cancelled.upgrade())
        {
            cancelled.store(true, Ordering::Release);
            return CancelDisposition::Requested;
        }
    CancelDisposition::NotActive
}

fn notice(
    control: &mut LoginControl<'_>,
    id: &LoginFlowId,
    state: LoginState,
) -> Result<(), AuthError> {
    control.emit(AuthInteraction::Notify {
        flow_id: id.clone(),
        state,
    })
}
fn dismiss(control: &mut LoginControl<'_>, id: &LoginFlowId, open: &mut bool) {
    if std::mem::take(open) {
        let _ = control.emit(AuthInteraction::CancelPrompt {
            flow_id: id.clone(),
            prompt_id: PROMPT_ID,
        });
    }
}
fn finish(
    control: &mut LoginControl<'_>,
    id: &LoginFlowId,
    open: &mut bool,
    outcome: &LoginOutcome,
) {
    dismiss(control, id, open);
    let state = match outcome {
        Ok(_) => LoginState::Succeeded,
        Err(AuthError::Cancelled) => LoginState::Cancelled,
        Err(AuthError::CommitUncertain) => LoginState::CommitUncertain,
        Err(_) => LoginState::Failed,
    };
    let _ = notice(control, id, state);
    control.flow_cancelled = None;
}

pub(crate) fn login_browser(
    flow: BrowserLogin,
    request: LoginRequest<'_>,
    control: &mut LoginControl<'_>,
) -> LoginOutcome {
    let active = ActiveLogin::acquire(&flow.flow_id)?;
    control.flow_cancelled = Some(active.cancelled.clone());
    let mut prompt_open = false;
    let outcome = (|| {
        control.check()?;
        notice(control, &flow.flow_id, LoginState::Preparing)?;
        let mut server = CallbackServer::bind()?;
        notice(control, &flow.flow_id, LoginState::AwaitingAuthorization)?;
        prompt_open = true;
        control.emit(AuthInteraction::Prompt {
            flow_id: flow.flow_id.clone(),
            prompt_id: PROMPT_ID,
            prompt: AuthPrompt::Browser {
                authorization_url: flow.authorization_url.clone(),
                manual: flow.submission.clone(),
            },
        })?;
        let code = loop {
            control.check()?;
            server.pump(&flow.submission)?;
            if let Some(result) = flow.submission.take()? {
                break result?;
            }
            sleep(control, Duration::from_millis(5))?;
        };
        // Close the listener and losing manual input before exchanging the code.
        drop(server);
        flow.submission.close();
        dismiss(control, &flow.flow_id, &mut prompt_open);
        notice(control, &flow.flow_id, LoginState::Exchanging)?;
        let body = code_request(&code, &flow.verifier, definition::REDIRECT_URI)?;
        let response = dispatch(AuthHttpKind::CodeExchange, &body, control, &mut post)?;
        require_success(&response, false)?;
        let pending =
            parse_token_response(&response.body, now_ms()?, TokenGrant::Initial)?.into_pending();
        commit(request, &flow.flow_id, pending, control)
    })();
    finish(control, &flow.flow_id, &mut prompt_open, &outcome);
    outcome
}

pub(crate) fn login_device(
    flow: DeviceLogin,
    request: LoginRequest<'_>,
    control: &mut LoginControl<'_>,
) -> LoginOutcome {
    device_login_with(flow, request, control, &mut post)
}

fn device_login_with(
    flow: DeviceLogin,
    request: LoginRequest<'_>,
    control: &mut LoginControl<'_>,
    send: &mut dyn FnMut(
        AuthHttpKind,
        &[u8],
        &mut LoginControl<'_>,
    ) -> Result<HttpResponse, AuthError>,
) -> LoginOutcome {
    let active = ActiveLogin::acquire(&flow.flow_id)?;
    control.flow_cancelled = Some(active.cancelled.clone());
    let mut prompt_open = false;
    let outcome = (|| {
        control.check()?;
        notice(control, &flow.flow_id, LoginState::Preparing)?;
        let body = serde_json::to_vec(&serde_json::json!({"client_id": definition::CLIENT_ID}))
            .map_err(|_| AuthError::LoginFailed)?;
        let response = dispatch(AuthHttpKind::DeviceBegin, &body, control, send)?;
        require_success(&response, true)?;
        let mut challenge = parse_device_begin(&response.body)?;
        notice(control, &flow.flow_id, LoginState::AwaitingAuthorization)?;
        prompt_open = true;
        control.emit(AuthInteraction::Prompt {
            flow_id: flow.flow_id.clone(),
            prompt_id: PROMPT_ID,
            prompt: AuthPrompt::Device {
                verification_uri: definition::DEVICE_VERIFICATION_URI,
                user_code: challenge.user_code.clone(),
            },
        })?;
        let body = serde_json::to_vec(&serde_json::json!({"device_auth_id": challenge.device_auth_id, "user_code": challenge.user_code})).map_err(|_| AuthError::LoginFailed)?;
        // The first poll is immediate; only pending responses cause a wait.
        let (code, verifier) = loop {
            let response = dispatch(AuthHttpKind::DevicePoll, &body, control, send)?;
            match parse_device_poll(response.status, &response.body)? {
                DevicePoll::Authorized { code, verifier } => break (code, verifier),
                DevicePoll::Pending => (),
                DevicePoll::SlowDown => {
                    challenge.interval = challenge
                        .interval
                        .checked_add(Duration::from_secs(5))
                        .ok_or(AuthError::InvalidResponse)?
                }
            }
            sleep(control, challenge.interval)?;
        };
        dismiss(control, &flow.flow_id, &mut prompt_open);
        notice(control, &flow.flow_id, LoginState::Exchanging)?;
        let body = code_request(&code, &verifier, definition::DEVICE_REDIRECT_URI)?;
        let response = dispatch(AuthHttpKind::CodeExchange, &body, control, send)?;
        require_success(&response, false)?;
        let pending =
            parse_token_response(&response.body, now_ms()?, TokenGrant::Initial)?.into_pending();
        commit(request, &flow.flow_id, pending, control)
    })();
    finish(control, &flow.flow_id, &mut prompt_open, &outcome);
    outcome
}

fn commit(
    request: LoginRequest<'_>,
    id: &LoginFlowId,
    pending: PendingCredential,
    control: &mut LoginControl<'_>,
) -> LoginOutcome {
    control.check()?;
    notice(control, id, LoginState::Committing)?;
    let cancelled = || control.is_cancelled();
    let wait = LockWait {
        deadline: control.deadline,
        cancelled: &cancelled,
    };
    let credential = request.store.modify(
        request.credential_id,
        request.expected_revision,
        Mutation::Replace(Replacement::Login(pending)),
        &wait,
    )?;
    // No cancellation check after durable success: do not misreport a saved login.
    Ok(LoginSuccess {
        flow_id: id.clone(),
        credential,
    })
}

fn sleep(control: &LoginControl<'_>, duration: Duration) -> Result<(), AuthError> {
    let until = Instant::now()
        .checked_add(duration)
        .unwrap_or(control.deadline)
        .min(control.deadline);
    loop {
        control.check()?;
        let remaining = until.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}
fn now_ms() -> Result<u64, AuthError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| u64::try_from(value.as_millis()).ok())
        .ok_or(AuthError::InvalidResponse)
}

struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}
fn dispatch(
    kind: AuthHttpKind,
    body: &[u8],
    control: &mut LoginControl<'_>,
    send: &mut dyn FnMut(
        AuthHttpKind,
        &[u8],
        &mut LoginControl<'_>,
    ) -> Result<HttpResponse, AuthError>,
) -> Result<HttpResponse, AuthError> {
    control.admit(kind)?;
    send(kind, body, control)
}
fn post(
    kind: AuthHttpKind,
    body: &[u8],
    control: &mut LoginControl<'_>,
) -> Result<HttpResponse, AuthError> {
    let endpoint = match kind {
        AuthHttpKind::DeviceBegin => definition::DEVICE_BEGIN_ENDPOINT,
        AuthHttpKind::DevicePoll => definition::DEVICE_POLL_ENDPOINT,
        AuthHttpKind::CodeExchange | AuthHttpKind::Refresh => definition::TOKEN_ENDPOINT,
    };
    send_http(
        endpoint,
        matches!(kind, AuthHttpKind::DeviceBegin | AuthHttpKind::DevicePoll),
        body,
        control,
    )
}
fn send_http(
    endpoint: &str,
    json: bool,
    body: &[u8],
    control: &LoginControl<'_>,
) -> Result<HttpResponse, AuthError> {
    control.check()?;
    send_http_with_control(endpoint, json, body, control.deadline, &|| {
        control.is_cancelled()
    })
}

fn send_http_with_control(
    endpoint: &str,
    json: bool,
    body: &[u8],
    deadline: Instant,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<HttpResponse, AuthError> {
    let deadline = deadline.min(Instant::now() + Duration::from_secs(15));
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(deadline.saturating_duration_since(Instant::now())))
        .timeout_recv_body(Some(Duration::from_millis(100)))
        .max_redirects(0)
        .max_redirects_will_error(false)
        .http_status_as_error(false)
        .build();
    let (client, cancel) = transport::client(config);
    let user_agent = format!(
        "zenpi/{} ({}; {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let builder = client
        .post(endpoint)
        .header(
            "Content-Type",
            if json {
                "application/json"
            } else {
                "application/x-www-form-urlencoded"
            },
        )
        .header("Accept", "application/json")
        .header("originator", definition::ORIGINATOR)
        .header("User-Agent", &user_agent);
    let cancelled = || is_cancelled() || Instant::now() >= deadline;
    let map_error = |error| {
        if is_cancelled() {
            AuthError::Cancelled
        } else if Instant::now() >= deadline {
            AuthError::DeadlineExceeded
        } else if matches!(error, BackendError::InvalidResponse(_)) {
            AuthError::ResponseTooLarge
        } else {
            AuthError::Transport
        }
    };
    let mut response =
        transport::send_bytes(builder, body, &cancelled, cancel).map_err(&map_error)?;
    let status = response.status().as_u16();
    let body =
        transport::read_bytes(response.body_mut(), MAX_RESPONSE, &cancelled).map_err(map_error)?;
    Ok(HttpResponse { status, body })
}

fn code_request(code: &str, verifier: &str, redirect: &'static str) -> Result<Vec<u8>, AuthError> {
    text(code, 8 * 1024)?;
    text(verifier, MAX_TOKEN)?;
    Ok(url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("client_id", definition::CLIENT_ID),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
        ])
        .finish()
        .into_bytes())
}

pub(crate) struct RefreshRequest {
    body: Vec<u8>,
}
pub(crate) fn refresh_request(refresh_token: &str) -> Result<RefreshRequest, AuthError> {
    text(refresh_token, MAX_TOKEN)?;
    Ok(RefreshRequest {
        body: url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", definition::CLIENT_ID),
            ])
            .finish()
            .into_bytes(),
    })
}

// The resolver durably records the attempt and admits AuthRefresh before calling
// this function. Once invoked, any transport failure has an unknown remote result.
pub(crate) fn send_refresh(
    request: &RefreshRequest,
    control: &crate::backend::RequestControl<'_>,
) -> Result<RefreshTokens, AuthError> {
    let deadline = control
        .deadline
        .unwrap_or_else(|| Instant::now() + Duration::from_secs(15));
    let response = send_http_with_control(
        definition::TOKEN_ENDPOINT,
        false,
        &request.body,
        deadline,
        control.cancelled,
    )?;
    require_success(&response, false)?;
    Ok(parse_token_response(&response.body, now_ms()?, TokenGrant::Refresh)?.into_refresh())
}

fn text(value: &str, max: usize) -> Result<(), AuthError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(AuthError::InvalidResponse)
    } else {
        Ok(())
    }
}
fn json<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, AuthError> {
    if body.len() > MAX_RESPONSE {
        return Err(AuthError::ResponseTooLarge);
    }
    serde_json::from_slice(body).map_err(|_| AuthError::InvalidResponse)
}

#[derive(Deserialize)]
struct DeviceBeginWire {
    device_auth_id: String,
    user_code: String,
    interval: Value,
}
struct DeviceChallenge {
    device_auth_id: String,
    user_code: String,
    interval: Duration,
}
fn parse_device_begin(body: &[u8]) -> Result<DeviceChallenge, AuthError> {
    let wire: DeviceBeginWire = json(body)?;
    text(&wire.device_auth_id, MAX_TOKEN)?;
    text(&wire.user_code, 1024)?;
    let interval = wire
        .interval
        .as_f64()
        .or_else(|| wire.interval.as_str()?.parse().ok())
        .ok_or(AuthError::InvalidResponse)?;
    if !interval.is_finite() || interval <= 0.0 {
        return Err(AuthError::InvalidResponse);
    }
    let interval =
        Duration::try_from_secs_f64(interval.max(1.0)).map_err(|_| AuthError::InvalidResponse)?;
    Ok(DeviceChallenge {
        device_auth_id: wire.device_auth_id,
        user_code: wire.user_code,
        interval,
    })
}
enum DevicePoll {
    Authorized { code: String, verifier: String },
    Pending,
    SlowDown,
}
#[derive(Deserialize)]
struct DevicePollWire {
    authorization_code: String,
    code_verifier: String,
}
fn parse_device_poll(status: u16, body: &[u8]) -> Result<DevicePoll, AuthError> {
    if body.len() > MAX_RESPONSE {
        return Err(AuthError::ResponseTooLarge);
    }
    if (300..400).contains(&status) {
        return Err(AuthError::LoginFailed);
    }
    if matches!(status, 403 | 404) {
        return Ok(DevicePoll::Pending);
    }
    if let Some(code) = error_code(body)? {
        return match code.as_str() {
            "deviceauth_authorization_pending" => Ok(DevicePoll::Pending),
            "slow_down" => Ok(DevicePoll::SlowDown),
            _ => Err(error_kind(&code)),
        };
    }
    if !(200..300).contains(&status) {
        return Err(AuthError::LoginFailed);
    }
    let wire: DevicePollWire = json(body)?;
    text(&wire.authorization_code, 8 * 1024)?;
    text(&wire.code_verifier, MAX_TOKEN)?;
    Ok(DevicePoll::Authorized {
        code: wire.authorization_code,
        verifier: wire.code_verifier,
    })
}
#[derive(Deserialize)]
struct ErrorWire {
    error: Option<OAuthError>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum OAuthError {
    Code(String),
    Object { code: String },
}
fn error_code(body: &[u8]) -> Result<Option<String>, AuthError> {
    let error: ErrorWire = json(body)?;
    let Some(value) = error.error else {
        return Ok(None);
    };
    let code = match value {
        OAuthError::Code(code) | OAuthError::Object { code } => code,
    };
    text(&code, 256)?;
    Ok(Some(code))
}
fn error_kind(code: &str) -> AuthError {
    match code {
        "access_denied" => AuthError::AccessDenied,
        "invalid_grant"
        | "expired_token"
        | "refresh_token_expired"
        | "refresh_token_reused"
        | "refresh_token_invalidated" => AuthError::LoginRequired,
        _ => AuthError::LoginFailed,
    }
}
fn require_success(response: &HttpResponse, begin: bool) -> Result<(), AuthError> {
    if begin && response.status == 404 {
        return Err(AuthError::Unavailable);
    }
    if let Some(code) = error_code(&response.body)? {
        return Err(error_kind(&code));
    }
    if !(200..300).contains(&response.status) {
        return Err(AuthError::LoginFailed);
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum TokenGrant {
    Initial,
    Refresh,
}
#[derive(Deserialize)]
struct TokenWire {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Value,
}
#[derive(Deserialize)]
struct Claims {
    #[serde(rename = "https://api.openai.com/auth")]
    auth: AuthClaims,
    exp: Option<Value>,
}
#[derive(Deserialize)]
struct AuthClaims {
    chatgpt_account_id: String,
    chatgpt_user_id: Option<String>,
    user_id: Option<String>,
}
pub(crate) struct ParsedTokens {
    access_token: String,
    refresh_token: Option<String>,
    account_id: String,
    user_id: Option<String>,
    expires_at_ms: u64,
}

pub(crate) fn parse_token_response(
    body: &[u8],
    now_ms: u64,
    grant: TokenGrant,
) -> Result<ParsedTokens, AuthError> {
    if let Some(code) = error_code(body)? {
        return Err(error_kind(&code));
    }
    let wire: TokenWire = json(body)?;
    text(&wire.access_token, MAX_TOKEN)?;
    match &wire.refresh_token {
        Some(token) => text(token, MAX_TOKEN)?,
        None if matches!(grant, TokenGrant::Initial) => return Err(AuthError::InvalidResponse),
        None => (),
    }
    let duration = seconds_to_ms(&wire.expires_in)
        .filter(|value| *value > 0)
        .ok_or(AuthError::InvalidResponse)?;
    let mut expires_at_ms = now_ms
        .checked_add(duration)
        .ok_or(AuthError::InvalidResponse)?;
    let parts: Vec<_> = wire.access_token.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::InvalidResponse);
    }
    let payload = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AuthError::InvalidResponse)?;
    // This only extracts claims. It does not verify a JWT or assert authenticity;
    // the production source is the fixed HTTPS token endpoint above.
    let claims: Claims = json(&payload)?;
    text(&claims.auth.chatgpt_account_id, 2048)?;
    let user_id = claims.auth.chatgpt_user_id.or(claims.auth.user_id);
    if let Some(user) = &user_id {
        text(user, 2048)?;
    }
    if claims
        .exp
        .as_ref()
        .and_then(Value::as_f64)
        .is_some_and(|value| value < 0.0)
    {
        return Err(AuthError::LoginRequired);
    }
    if let Some(exp) = claims.exp.as_ref().and_then(seconds_to_ms) {
        expires_at_ms = expires_at_ms.min(exp);
    }
    if expires_at_ms <= now_ms {
        return Err(AuthError::LoginRequired);
    }
    Ok(ParsedTokens {
        access_token: wire.access_token,
        refresh_token: wire.refresh_token,
        account_id: claims.auth.chatgpt_account_id,
        user_id,
        expires_at_ms,
    })
}
fn seconds_to_ms(value: &Value) -> Option<u64> {
    if let Some(seconds) = value.as_u64() {
        return seconds.checked_mul(1000);
    }
    let millis = (value.as_f64()? * 1000.0).floor();
    (millis.is_finite() && millis >= 0.0 && millis < u64::MAX as f64).then_some(millis as u64)
}
impl ParsedTokens {
    fn into_pending(self) -> PendingCredential {
        PendingCredential {
            kind: CredentialKind::Oauth,
            provider: definition::definition().id.to_owned(),
            definition_version: definition::definition().definition_version,
            issuer: definition::ISSUER.into(),
            client_id: definition::CLIENT_ID.into(),
            account_id: Some(self.account_id),
            user_id: self.user_id,
            allowed_destinations: vec![AllowedDestination {
                origin: "https://chatgpt.com".into(),
                path_prefix: "/backend-api/codex".into(),
                protocols: vec!["openai_codex_responses".into()],
                headers: vec!["authorization".into(), "chatgpt-account-id".into()],
            }],
            api_key: None,
            access_token: Some(self.access_token),
            refresh_token: self.refresh_token,
            expires_at_ms: Some(self.expires_at_ms),
        }
    }
    pub(crate) fn into_refresh(self) -> RefreshTokens {
        RefreshTokens {
            issuer: definition::ISSUER.into(),
            client_id: definition::CLIENT_ID.into(),
            account_id: self.account_id,
            user_id: self.user_id,
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at_ms: self.expires_at_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    static ACTIVE_FLOW_TEST: Mutex<()> = Mutex::new(());

    fn jwt(auth: Value, exp: Value) -> String {
        format!(
            "e30.{}.synthetic-signature",
            URL_SAFE_NO_PAD.encode(
                serde_json::to_vec(
                    &serde_json::json!({"https://api.openai.com/auth":auth,"exp":exp})
                )
                .unwrap()
            )
        )
    }
    fn token_response(expires_in: Value, exp: Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"access_token":jwt(serde_json::json!({"chatgpt_account_id":"synthetic-account","chatgpt_user_id":"synthetic-user"}), exp),
            "refresh_token":"synthetic-refresh", "expires_in":expires_in})).unwrap()
    }

    #[test]
    fn browser_preparation_has_frozen_literals_and_independent_pkce_state() {
        let flow = begin_browser_login().unwrap();
        let other = begin_browser_login().unwrap();
        assert_eq!(flow.verifier.len(), 43);
        assert!(
            flow.verifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
        );
        assert_ne!(flow.verifier, other.verifier);
        let url = Url::parse(&flow.authorization_url).unwrap();
        assert_eq!(url.origin().ascii_serialization(), definition::ISSUER);
        assert_eq!(url.path(), "/oauth/authorize");
        let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(query["client_id"], definition::CLIENT_ID);
        assert_eq!(query["scope"], "openid profile email offline_access");
        assert_eq!(query["redirect_uri"], "http://localhost:1455/auth/callback");
        assert_eq!(query["originator"], "zenpi");
        assert_eq!(query["response_type"], "code");
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(query["id_token_add_organizations"], "true");
        assert_eq!(query["codex_cli_simplified_flow"], "true");
        assert_eq!(
            query["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(flow.verifier.as_bytes()))
        );
        assert_eq!(query["state"].len(), 43);
        assert_ne!(query["state"], flow.verifier);
        assert_ne!(query["state"], flow.flow_id.as_str());
    }

    #[test]
    fn code_and_refresh_forms_have_no_secret_client_or_api_key_exchange() {
        for redirect in [definition::REDIRECT_URI, definition::DEVICE_REDIRECT_URI] {
            let form = code_request("synthetic+a&b", "synthetic-verifier", redirect).unwrap();
            let pairs: BTreeMap<_, _> = url::form_urlencoded::parse(&form).into_owned().collect();
            assert_eq!(pairs.len(), 5);
            assert_eq!(pairs["code"], "synthetic+a&b");
            assert_eq!(pairs["redirect_uri"], redirect);
            assert_eq!(pairs["grant_type"], "authorization_code");
            assert_eq!(pairs["client_id"], definition::CLIENT_ID);
        }
        let request = refresh_request("synthetic+refresh&token").unwrap();
        let pairs: BTreeMap<_, _> = url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs["grant_type"], "refresh_token");
        assert_eq!(pairs["refresh_token"], "synthetic+refresh&token");
        assert!(!pairs.contains_key("redirect_uri"));
        assert!(refresh_request("\n").is_err());
    }

    #[test]
    fn token_expiry_uses_checked_millisecond_floor_and_earlier_jwt_exp() {
        let parsed = parse_token_response(
            &token_response(serde_json::json!(1.2349), Value::Null),
            1000,
            TokenGrant::Initial,
        )
        .unwrap();
        assert_eq!(parsed.expires_at_ms, 2234);
        assert_eq!(parsed.account_id, "synthetic-account");
        let parsed = parse_token_response(
            &token_response(serde_json::json!(60), serde_json::json!(2.5)),
            1000,
            TokenGrant::Initial,
        )
        .unwrap();
        assert_eq!(parsed.expires_at_ms, 2500);
        assert_eq!(parsed.user_id.as_deref(), Some("synthetic-user"));
        for expires in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!(0.0009),
            serde_json::json!("12"),
            serde_json::json!(1e300),
            Value::Null,
        ] {
            assert!(
                parse_token_response(
                    &token_response(expires, Value::Null),
                    1000,
                    TokenGrant::Initial
                )
                .is_err()
            );
        }
        assert!(
            parse_token_response(
                &token_response(serde_json::json!(1), Value::Null),
                u64::MAX,
                TokenGrant::Initial
            )
            .is_err()
        );
        assert!(matches!(
            parse_token_response(
                &token_response(serde_json::json!(60), serde_json::json!(1)),
                1000,
                TokenGrant::Initial
            ),
            Err(AuthError::LoginRequired)
        ));
        assert!(matches!(
            parse_token_response(
                &token_response(serde_json::json!(60), serde_json::json!(-1)),
                1000,
                TokenGrant::Initial
            ),
            Err(AuthError::LoginRequired)
        ));
        // An invalid exp is not a verified claim and cannot replace expires_in.
        assert_eq!(
            parse_token_response(
                &token_response(serde_json::json!(1), serde_json::json!("invalid")),
                1000,
                TokenGrant::Initial
            )
            .unwrap()
            .expires_at_ms,
            2000
        );
    }

    #[test]
    fn token_identity_is_required_and_refresh_may_omit_only_refresh_token() {
        for auth in [
            serde_json::json!({}),
            serde_json::json!({"chatgpt_account_id":""}),
            serde_json::json!({"chatgpt_account_id":"\n"}),
        ] {
            let body = serde_json::to_vec(&serde_json::json!({"access_token":jwt(auth, Value::Null), "refresh_token":"synthetic", "expires_in":10})).unwrap();
            assert!(parse_token_response(&body, 1, TokenGrant::Initial).is_err());
        }
        let body = serde_json::to_vec(&serde_json::json!({"access_token":jwt(serde_json::json!({"chatgpt_account_id":"account","user_id":"fallback-user"}), Value::Null), "expires_in":10})).unwrap();
        assert!(parse_token_response(&body, 1, TokenGrant::Initial).is_err());
        let refreshed = parse_token_response(&body, 1, TokenGrant::Refresh)
            .unwrap()
            .into_refresh();
        assert!(refreshed.refresh_token.is_none());
        assert_eq!(refreshed.user_id.as_deref(), Some("fallback-user"));
        assert_eq!(refreshed.issuer, definition::ISSUER);
        assert_eq!(refreshed.client_id, definition::CLIENT_ID);
        for body in [
            br#"{"access_token":"not-a-jwt","refresh_token":"secret","expires_in":10}"#.as_slice(),
            br#"{"access_token":"a.@@@@.b","refresh_token":"secret","expires_in":10}"#,
        ] {
            assert!(parse_token_response(body, 1, TokenGrant::Initial).is_err());
        }
    }

    #[test]
    fn device_interval_and_pending_are_specific_and_bounded() {
        for interval in [
            serde_json::json!(0.1),
            serde_json::json!(1),
            serde_json::json!("2.5"),
        ] {
            let body = serde_json::to_vec(&serde_json::json!({"device_auth_id":"synthetic-device","user_code":"synthetic-code","interval":interval})).unwrap();
            assert!(parse_device_begin(&body).unwrap().interval >= Duration::from_secs(1));
        }
        for interval in [
            serde_json::json!(0),
            serde_json::json!(-1),
            serde_json::json!("NaN"),
            serde_json::json!("inf"),
            Value::Null,
        ] {
            let body = serde_json::to_vec(
                &serde_json::json!({"device_auth_id":"d","user_code":"c","interval":interval}),
            )
            .unwrap();
            assert!(parse_device_begin(&body).is_err());
        }
        assert!(parse_device_begin(br#"{"device_auth_id":"d","user_code":"c"}"#).is_err());
        assert!(matches!(
            parse_device_poll(403, b""),
            Ok(DevicePoll::Pending)
        ));
        assert!(matches!(
            parse_device_poll(404, b"not JSON"),
            Ok(DevicePoll::Pending)
        ));
        assert!(matches!(
            parse_device_poll(
                400,
                br#"{"error":{"code":"deviceauth_authorization_pending"}}"#
            ),
            Ok(DevicePoll::Pending)
        ));
        assert!(matches!(
            parse_device_poll(429, br#"{"error":"slow_down"}"#),
            Ok(DevicePoll::SlowDown)
        ));
        assert!(parse_device_poll(400, br#"{"error":"authorization_pending"}"#).is_err());
        assert!(matches!(
            parse_device_poll(200, br#"{"authorization_code":"c","code_verifier":"v"}"#),
            Ok(DevicePoll::Authorized { .. })
        ));
        assert!(parse_device_poll(200, br#"{"authorization_code":"c"}"#).is_err());
        assert_eq!(
            require_success(
                &HttpResponse {
                    status: 404,
                    body: Vec::new()
                },
                true
            ),
            Err(AuthError::Unavailable)
        );
    }

    #[test]
    fn errors_and_private_challenges_never_derive_secret_diagnostics() {
        assert!(matches!(parse_token_response(br#"{"error":{"code":"refresh_token_reused"},"error_description":"synthetic-secret"}"#, 1, TokenGrant::Refresh), Err(AuthError::LoginRequired)));
        let error = require_success(&HttpResponse { status: 400, body: br#"{"error":"unknown-synthetic-secret","error_description":"synthetic-secret"}"#.to_vec() }, false).unwrap_err();
        assert_eq!(error, AuthError::LoginFailed);
        assert!(!format!("{error:?} {error}").contains("synthetic-secret"));
        macro_rules! no_trait {
            ($ty:ty, $bound:path) => {{
                trait Ambiguous<A> {
                    fn check() {}
                }
                impl<T: ?Sized> Ambiguous<()> for T {}
                struct Bound;
                impl<T: ?Sized + $bound> Ambiguous<Bound> for T {}
                let _ = <$ty as Ambiguous<_>>::check;
            }};
        }
        no_trait!(BrowserLogin, std::fmt::Debug);
        no_trait!(BrowserLogin, serde::Serialize);
        no_trait!(AuthInteraction, std::fmt::Debug);
        no_trait!(AuthInteraction, serde::Serialize);
        no_trait!(ParsedTokens, std::fmt::Debug);
        no_trait!(ParsedTokens, serde::Serialize);
        no_trait!(RefreshRequest, std::fmt::Debug);
        no_trait!(RefreshRequest, serde::Serialize);
    }

    #[test]
    fn control_enforces_budget_cancel_deadline_and_denial_before_network() {
        let count = Cell::new(0);
        let cancelled = Cell::new(false);
        let cancellation = || cancelled.get();
        let mut admission = |_| {
            count.set(count.get() + 1);
            Ok(())
        };
        let mut interaction = |_| Ok(());
        let mut control = LoginControl::new(
            &cancellation,
            Instant::now() + Duration::from_secs(3600),
            1,
            &mut admission,
            &mut interaction,
        );
        assert!(control.deadline <= Instant::now() + Duration::from_secs(900));
        assert_eq!(control.admit(AuthHttpKind::DeviceBegin), Ok(()));
        assert_eq!(
            control.admit(AuthHttpKind::DevicePoll),
            Err(AuthError::BudgetExceeded)
        );
        assert_eq!(count.get(), 1);
        cancelled.set(true);
        assert_eq!(control.check(), Err(AuthError::Cancelled));
        cancelled.set(false);
        control.deadline = Instant::now();
        assert_eq!(control.check(), Err(AuthError::DeadlineExceeded));
        let mut deny = |_| Err(AuthError::SendDenied);
        let mut control = LoginControl::new(
            &|| false,
            Instant::now() + Duration::from_secs(1),
            1,
            &mut deny,
            &mut interaction,
        );
        assert!(matches!(
            dispatch(AuthHttpKind::DeviceBegin, b"{}", &mut control, &mut post),
            Err(AuthError::SendDenied)
        ));
    }

    #[test]
    fn one_host_flow_and_flow_specific_cancel_are_released_by_drop() {
        let _serial = ACTIVE_FLOW_TEST.lock().unwrap();
        let flow = begin_device_login().unwrap();
        let active = ActiveLogin::acquire(flow.flow_id()).unwrap();
        let other = begin_device_login().unwrap();
        assert!(matches!(
            ActiveLogin::acquire(other.flow_id()),
            Err(AuthError::LoginBusy)
        ));
        assert_eq!(cancel_login(other.flow_id()), CancelDisposition::NotActive);
        assert!(!active.cancelled.load(Ordering::Acquire));
        assert_eq!(cancel_login(flow.flow_id()), CancelDisposition::Requested);
        assert!(active.cancelled.load(Ordering::Acquire));
        drop(active);
        assert_eq!(cancel_login(flow.flow_id()), CancelDisposition::NotActive);
        assert!(ActiveLogin::acquire(other.flow_id()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn complete_device_flow_orders_begin_poll_exchange_then_durable_commit() {
        use std::os::unix::fs::PermissionsExt;
        let _serial = ACTIVE_FLOW_TEST.lock().unwrap();
        for outcome_case in 0..3 {
            let directory = tempfile::tempdir().unwrap();
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let store = store::CredentialStore::new(
                directory.path().canonicalize().unwrap().join("auth.json"),
            )
            .unwrap();
            let store = if outcome_case == 2 {
                store.with_uncertain_commit_for_test()
            } else {
                store
            };
            let cancelled = Cell::new(false);
            let cancellation = || cancelled.get();
            let states = RefCell::new(Vec::new());
            let prompts = Cell::new(0);
            let dismissals = Cell::new(0);
            let mut admission = |_| Ok(());
            let mut interaction = |event| {
                match event {
                    AuthInteraction::Notify { state, .. } => {
                        states.borrow_mut().push(state);
                        if outcome_case == 1 && state == LoginState::Committing {
                            cancelled.set(true);
                        }
                    }
                    AuthInteraction::Prompt {
                        prompt:
                            AuthPrompt::Device {
                                verification_uri,
                                user_code,
                            },
                        ..
                    } => {
                        assert_eq!(verification_uri, definition::DEVICE_VERIFICATION_URI);
                        assert_eq!(user_code, "synthetic-user-code");
                        prompts.set(prompts.get() + 1);
                    }
                    AuthInteraction::CancelPrompt { .. } => dismissals.set(dismissals.get() + 1),
                    _ => panic!("unexpected synthetic prompt"),
                }
                Ok(())
            };
            let mut control = LoginControl::new(
                &cancellation,
                Instant::now() + Duration::from_secs(2),
                3,
                &mut admission,
                &mut interaction,
            );
            let mut calls = Vec::new();
            let mut send = |kind, body: &[u8], _control: &mut LoginControl<'_>| {
                calls.push(kind);
                let body = match kind {
                    AuthHttpKind::DeviceBegin => {
                        assert_eq!(
                            serde_json::from_slice::<Value>(body).unwrap(),
                            serde_json::json!({"client_id":definition::CLIENT_ID})
                        );
                        br#"{"device_auth_id":"synthetic-private-device","user_code":"synthetic-user-code","interval":"1"}"#.to_vec()
                    }
                    AuthHttpKind::DevicePoll => {
                        assert_eq!(
                            serde_json::from_slice::<Value>(body).unwrap(),
                            serde_json::json!({"device_auth_id":"synthetic-private-device","user_code":"synthetic-user-code"})
                        );
                        br#"{"authorization_code":"synthetic-code","code_verifier":"synthetic-verifier"}"#.to_vec()
                    }
                    AuthHttpKind::CodeExchange => {
                        let fields: BTreeMap<_, _> =
                            url::form_urlencoded::parse(body).into_owned().collect();
                        assert_eq!(fields["redirect_uri"], definition::DEVICE_REDIRECT_URI);
                        token_response(serde_json::json!(60), Value::Null)
                    }
                    _ => panic!("unexpected synthetic send"),
                };
                Ok(HttpResponse { status: 200, body })
            };
            let result = device_login_with(
                begin_device_login().unwrap(),
                LoginRequest {
                    store: &store,
                    credential_id: "credential",
                    expected_revision: None,
                },
                &mut control,
                &mut send,
            );
            assert_eq!(
                calls,
                [
                    AuthHttpKind::DeviceBegin,
                    AuthHttpKind::DevicePoll,
                    AuthHttpKind::CodeExchange
                ]
            );
            assert_eq!(control.sends_remaining, 0);
            assert_eq!(prompts.get(), 1);
            assert_eq!(dismissals.get(), 1);
            let expected = match outcome_case {
                0 => LoginState::Succeeded,
                1 => LoginState::Cancelled,
                _ => LoginState::CommitUncertain,
            };
            assert_eq!(states.borrow().last(), Some(&expected));
            match outcome_case {
                0 => assert_eq!(
                    result
                        .unwrap()
                        .credential
                        .identity()
                        .unwrap()
                        .account_id
                        .as_deref(),
                    Some("synthetic-account")
                ),
                1 => {
                    assert!(matches!(result, Err(AuthError::Cancelled)));
                    assert!(store.list_status().unwrap().is_empty());
                }
                _ => assert!(matches!(result, Err(AuthError::CommitUncertain))),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn invalid_or_redirect_poll_errors_end_driver_without_another_send() {
        let _serial = ACTIVE_FLOW_TEST.lock().unwrap();
        for (status, body, expected) in [
            (
                400,
                br#"{"error":{"code":"access_denied","code":"deviceauth_authorization_pending"}}"#
                    .as_slice(),
                AuthError::InvalidResponse,
            ),
            (
                302,
                br#"{"error":"deviceauth_authorization_pending"}"#.as_slice(),
                AuthError::LoginFailed,
            ),
            (
                307,
                br#"{"error":"slow_down"}"#.as_slice(),
                AuthError::LoginFailed,
            ),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let store = store::CredentialStore::new(
                directory.path().canonicalize().unwrap().join("auth.json"),
            )
            .unwrap();
            let calls = RefCell::new(Vec::new());
            let states = RefCell::new(Vec::new());
            let mut admission = |_| Ok(());
            let mut interaction = |event| {
                if let AuthInteraction::Notify { state, .. } = event {
                    states.borrow_mut().push(state);
                }
                Ok(())
            };
            let mut control = LoginControl::new(
                &|| false,
                Instant::now() + Duration::from_secs(2),
                3,
                &mut admission,
                &mut interaction,
            );
            let mut send = |kind, _: &[u8], _: &mut LoginControl<'_>| {
                calls.borrow_mut().push(kind);
                match kind {
                    AuthHttpKind::DeviceBegin => Ok(HttpResponse { status: 200, body: br#"{"device_auth_id":"synthetic","user_code":"synthetic","interval":1}"#.to_vec() }),
                    AuthHttpKind::DevicePoll => Ok(HttpResponse { status, body: body.to_vec() }),
                    _ => panic!("invalid synthetic poll must not exchange or send again"),
                }
            };
            let result = device_login_with(
                begin_device_login().unwrap(),
                LoginRequest {
                    store: &store,
                    credential_id: "credential",
                    expected_revision: None,
                },
                &mut control,
                &mut send,
            );
            assert!(matches!(result, Err(error) if error == expected));
            assert_eq!(
                *calls.borrow(),
                [AuthHttpKind::DeviceBegin, AuthHttpKind::DevicePoll]
            );
            assert_eq!(control.sends_remaining, 1);
            assert_eq!(states.borrow().last(), Some(&LoginState::Failed));
        }
    }

    #[cfg(unix)]
    #[test]
    fn success_requires_commit_and_cancellation_does_not_relabel_saved_login() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let store =
            store::CredentialStore::new(directory.path().canonicalize().unwrap().join("auth.json"))
                .unwrap();
        let id = LoginFlowId("synthetic-flow".into());
        let cancelled = Cell::new(true);
        let cancellation = || cancelled.get();
        let states = RefCell::new(Vec::new());
        let mut admission = |_| Ok(());
        let mut interaction = |event| {
            if let AuthInteraction::Notify { state, .. } = event {
                states.borrow_mut().push(state);
            }
            Ok(())
        };
        let mut control = LoginControl::new(
            &cancellation,
            Instant::now() + Duration::from_secs(2),
            2,
            &mut admission,
            &mut interaction,
        );
        let pending = || {
            parse_token_response(
                &token_response(serde_json::json!(60), Value::Null),
                1,
                TokenGrant::Initial,
            )
            .unwrap()
            .into_pending()
        };
        let request = || LoginRequest {
            store: &store,
            credential_id: "credential",
            expected_revision: None,
        };
        assert!(matches!(
            commit(request(), &id, pending(), &mut control),
            Err(AuthError::Cancelled)
        ));
        assert!(store.list_status().unwrap().is_empty());
        cancelled.set(false);
        let success = commit(request(), &id, pending(), &mut control);
        assert!(success.is_ok());
        assert_eq!(
            success
                .as_ref()
                .unwrap()
                .credential
                .identity()
                .unwrap()
                .provider,
            "openai-codex"
        );
        cancelled.set(true);
        finish(&mut control, &id, &mut false, &success);
        assert_eq!(states.borrow().last(), Some(&LoginState::Succeeded));
        cancelled.set(false);
        let uncertain = store.with_uncertain_commit_for_test();
        let request = LoginRequest {
            store: &uncertain,
            credential_id: "credential",
            expected_revision: Some(1),
        };
        let outcome = commit(request, &id, pending(), &mut control);
        assert!(matches!(outcome, Err(AuthError::CommitUncertain)));
        finish(&mut control, &id, &mut false, &outcome);
        assert_eq!(states.borrow().last(), Some(&LoginState::CommitUncertain));
    }

    fn loopback_server(
        response: Vec<u8>,
        delay_body: Duration,
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let endpoint = format!("http://{}/synthetic", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("synthetic loopback accept: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() < 32 * 1024);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..end]);
                    let length: usize = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            if delay_body.is_zero() {
                let _ = stream.write_all(&response);
            } else {
                let end = response
                    .windows(4)
                    .position(|bytes| bytes == b"\r\n\r\n")
                    .unwrap()
                    + 4;
                stream.write_all(&response[..end]).unwrap();
                std::thread::sleep(delay_body);
                let _ = stream.write_all(&response[end..]);
            }
            String::from_utf8(request).unwrap()
        });
        (endpoint, worker)
    }
    fn response(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    #[test]
    fn auth_http_uses_controlled_transport_truthful_headers_and_form_bytes() {
        let (endpoint, worker) = loopback_server(response(b"{}"), Duration::ZERO);
        let mut admission = |_| Ok(());
        let mut interaction = |_| Ok(());
        let control = LoginControl::new(
            &|| false,
            Instant::now() + Duration::from_secs(2),
            1,
            &mut admission,
            &mut interaction,
        );
        let body = code_request(
            "synthetic code",
            "synthetic-verifier",
            definition::REDIRECT_URI,
        )
        .unwrap();
        let result = send_http(&endpoint, false, &body, &control);
        let request = worker.join().unwrap();
        assert_eq!(result.unwrap().status, 200);
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains("originator: zenpi\r\n"));
        assert!(lower.contains(&format!("user-agent: zenpi/{}", env!("CARGO_PKG_VERSION"))));
        assert!(lower.contains("content-type: application/x-www-form-urlencoded\r\n"));
        assert!(request.ends_with(std::str::from_utf8(&body).unwrap()));
        assert!(!lower.contains("authorization:"));
    }

    #[test]
    fn auth_http_does_not_follow_redirects_and_bounds_response_bytes() {
        let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        target.set_nonblocking(true).unwrap();
        let redirect = format!("HTTP/1.1 302 Found\r\nLocation: http://{}/not-allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", target.local_addr().unwrap()).into_bytes();
        let (endpoint, worker) = loopback_server(redirect, Duration::ZERO);
        let mut admission = |_| Ok(());
        let mut interaction = |_| Ok(());
        let control = LoginControl::new(
            &|| false,
            Instant::now() + Duration::from_secs(2),
            1,
            &mut admission,
            &mut interaction,
        );
        let result = send_http(&endpoint, true, b"{}", &control);
        worker.join().unwrap();
        assert_eq!(result.unwrap().status, 302);
        assert_eq!(
            target.accept().err().unwrap().kind(),
            std::io::ErrorKind::WouldBlock
        );
        let (endpoint, worker) =
            loopback_server(response(&vec![b'x'; MAX_RESPONSE + 1]), Duration::ZERO);
        let result = send_http(&endpoint, true, b"{}", &control);
        worker.join().unwrap();
        assert!(matches!(result, Err(AuthError::ResponseTooLarge)));
    }

    #[test]
    fn auth_http_body_wait_observes_cancel_and_shorter_deadline() {
        for cancellation in [true, false] {
            let (endpoint, worker) = loopback_server(response(b"{}"), Duration::from_millis(250));
            let start = Instant::now();
            let cancelled = || cancellation && start.elapsed() >= Duration::from_millis(50);
            let mut admission = |_| Ok(());
            let mut interaction = |_| Ok(());
            let deadline = start
                + if cancellation {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(50)
                };
            let control =
                LoginControl::new(&cancelled, deadline, 1, &mut admission, &mut interaction);
            let result = send_http(&endpoint, true, b"{}", &control);
            worker.join().unwrap();
            assert!(
                matches!(result, Err(error) if error == if cancellation { AuthError::Cancelled } else { AuthError::DeadlineExceeded })
            );
        }
    }

    #[test]
    fn refresh_form_and_shared_http_honor_request_control_after_admission() {
        use crate::backend::{HttpRequestKind, RequestControl, RequestPurpose, RequestScope};
        let payload = token_response(serde_json::json!(60), Value::Null);
        let (endpoint, worker) = loopback_server(response(&payload), Duration::ZERO);
        let count = Cell::new(0);
        let mut admission = |kind, _: &RequestScope| {
            assert_eq!(kind, HttpRequestKind::AuthRefresh);
            count.set(count.get() + 1);
            Ok(())
        };
        let mut control = RequestControl {
            cancelled: &|| false,
            deadline: Some(Instant::now() + Duration::from_secs(2)),
            scope: RequestScope {
                owner_id: "synthetic-owner".into(),
                session_id: "synthetic-session".into(),
                operation_id: "synthetic-operation".into(),
                purpose: RequestPurpose::Turn,
                route_digest: "route".into(),
                identity_scope: "identity".into(),
                policy_digest: None,
                lease_id: None,
            },
            before_send: &mut admission,
        };
        control.before_send(HttpRequestKind::AuthRefresh).unwrap();
        let form = refresh_request("synthetic refresh").unwrap();
        // Only this private shared-transport fixture substitutes a loopback URL;
        // send_refresh itself has no configurable endpoint.
        let result = send_http_with_control(
            &endpoint,
            false,
            &form.body,
            control.deadline.unwrap(),
            control.cancelled,
        )
        .unwrap();
        require_success(&result, false).unwrap();
        parse_token_response(&result.body, now_ms().unwrap(), TokenGrant::Refresh).unwrap();
        let wire = worker.join().unwrap();
        assert!(wire.ends_with(std::str::from_utf8(&form.body).unwrap()));
        assert!(wire.contains("grant_type=refresh_token"));
        assert!(wire.contains("refresh_token=synthetic+refresh"));
        assert!(wire.contains(&format!("client_id={}", definition::CLIENT_ID)));
        assert_eq!(count.get(), 1);

        let (endpoint, worker) = loopback_server(response(b"{}"), Duration::from_millis(200));
        control.deadline = Some(Instant::now() + Duration::from_millis(40));
        control.before_send(HttpRequestKind::AuthRefresh).unwrap();
        let result = send_http_with_control(
            &endpoint,
            false,
            &form.body,
            control.deadline.unwrap(),
            control.cancelled,
        );
        worker.join().unwrap();
        assert!(matches!(result, Err(AuthError::DeadlineExceeded)));
        assert_eq!(count.get(), 2);
    }
}
