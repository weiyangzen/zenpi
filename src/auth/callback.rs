//! Bounded loopback reception and a shared, single-use manual/callback winner.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use url::Url;

use super::{AuthError, LoginFlowId};
use crate::providers::codex::REDIRECT_URI;

const MAX_URI: usize = 8 * 1024;
const MAX_REQUEST: usize = 12 * 1024;
const MAX_CONNECTIONS: usize = 8;

struct Winner {
    flow_id: LoginFlowId,
    prompt_id: u64,
    state: String,
    resolved: bool,
    manual_closed: bool,
    result: Option<Result<String, AuthError>>,
}

#[derive(Clone)]
pub(crate) struct ManualSubmission(Arc<Mutex<Winner>>);

impl ManualSubmission {
    pub(super) fn new(flow_id: LoginFlowId, prompt_id: u64, state: String) -> Self {
        Self(Arc::new(Mutex::new(Winner {
            flow_id,
            prompt_id,
            state,
            resolved: false,
            manual_closed: false,
            result: None,
        })))
    }

    pub(crate) fn submit(
        &self,
        flow_id: &LoginFlowId,
        prompt_id: u64,
        uri: &str,
    ) -> Result<(), AuthError> {
        let mut winner = self.0.lock().map_err(|_| AuthError::LoginFailed)?;
        if winner.flow_id != *flow_id
            || winner.prompt_id != prompt_id
            || winner.manual_closed
            || winner.resolved
        {
            return Err(AuthError::StalePrompt);
        }
        accept(&mut winner, uri)
    }

    // Closing manual input is not cancellation of the entire browser login.
    pub(crate) fn cancel_prompt(
        &self,
        flow_id: &LoginFlowId,
        prompt_id: u64,
    ) -> Result<(), AuthError> {
        let mut winner = self.0.lock().map_err(|_| AuthError::LoginFailed)?;
        if winner.flow_id != *flow_id || winner.prompt_id != prompt_id || winner.resolved {
            return Err(AuthError::StalePrompt);
        }
        winner.manual_closed = true;
        Ok(())
    }

    pub(super) fn accept_callback(&self, uri: &str) -> Result<(), AuthError> {
        let mut winner = self.0.lock().map_err(|_| AuthError::LoginFailed)?;
        if winner.resolved {
            return Err(AuthError::StalePrompt);
        }
        accept(&mut winner, uri)
    }

    pub(super) fn take(&self) -> Result<Option<Result<String, AuthError>>, AuthError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| AuthError::LoginFailed)?
            .result
            .take())
    }

    pub(super) fn close(&self) {
        if let Ok(mut winner) = self.0.lock() {
            winner.resolved = true;
            winner.manual_closed = true;
            winner.result = None;
        }
    }
}

fn accept(winner: &mut Winner, uri: &str) -> Result<(), AuthError> {
    let result = parse_callback(uri, &winner.state)?;
    winner.resolved = true;
    winner.result = Some(result);
    Ok(())
}

fn parse_callback(uri: &str, expected_state: &str) -> Result<Result<String, AuthError>, AuthError> {
    if uri.len() > MAX_URI || uri.chars().any(char::is_control) {
        return Err(AuthError::InvalidCallback);
    }
    let url = Url::parse(uri).map_err(|_| AuthError::InvalidCallback)?;
    let expected = Url::parse(REDIRECT_URI).map_err(|_| AuthError::InvalidCallback)?;
    if url.scheme() != expected.scheme()
        || url.host_str() != expected.host_str()
        || url.port_or_known_default() != expected.port_or_known_default()
        || url.path() != expected.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(AuthError::InvalidCallback);
    }
    let (mut state, mut code, mut error) = (None, None, None);
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if slot.is_some() || value.is_empty() || value.chars().any(char::is_control) {
            return Err(AuthError::InvalidCallback);
        }
        *slot = Some(value.into_owned());
    }
    if state.as_deref() != Some(expected_state) {
        return Err(AuthError::StateMismatch);
    }
    match (code, error) {
        (Some(code), None) => Ok(Ok(code)),
        (None, Some(error)) => Ok(Err(match error.as_str() {
            "access_denied" => AuthError::AccessDenied,
            "invalid_grant" | "expired_token" => AuthError::LoginRequired,
            _ => AuthError::LoginFailed,
        })),
        _ => Err(AuthError::InvalidCallback),
    }
}

struct Connection {
    stream: TcpStream,
    bytes: Vec<u8>,
    deadline: Instant,
}
pub(super) struct CallbackServer {
    listeners: Vec<TcpListener>,
    connections: Vec<Connection>,
}

impl CallbackServer {
    pub(super) fn bind() -> Result<Self, AuthError> {
        let mut listeners = Vec::new();
        for address in [
            SocketAddr::from((Ipv4Addr::LOCALHOST, 1455)),
            SocketAddr::from((Ipv6Addr::LOCALHOST, 1455)),
        ] {
            match TcpListener::bind(address) {
                Ok(listener) => {
                    listener
                        .set_nonblocking(true)
                        .map_err(|_| AuthError::CallbackUnavailable)?;
                    listeners.push(listener);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    return Err(AuthError::CallbackPortInUse);
                }
                // Some hosts disable an address family. Never fall back to a
                // wildcard interface or a different, unregistered port.
                Err(error) if error.kind() == std::io::ErrorKind::AddrNotAvailable => (),
                Err(_) => return Err(AuthError::CallbackUnavailable),
            }
        }
        if listeners.is_empty() {
            return Err(AuthError::CallbackUnavailable);
        }
        Ok(Self {
            listeners,
            connections: Vec::new(),
        })
    }

    pub(super) fn pump(&mut self, submission: &ManualSubmission) -> Result<(), AuthError> {
        for listener in &self.listeners {
            // Bound accept work so a local flood cannot starve cancellation.
            for _ in 0..MAX_CONNECTIONS {
                match listener.accept() {
                    Ok((stream, peer))
                        if peer.ip().is_loopback() && self.connections.len() < MAX_CONNECTIONS =>
                    {
                        stream
                            .set_nonblocking(true)
                            .map_err(|_| AuthError::CallbackUnavailable)?;
                        self.connections.push(Connection {
                            stream,
                            bytes: Vec::new(),
                            deadline: Instant::now() + Duration::from_secs(2),
                        });
                    }
                    Ok(_) => (),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => return Err(AuthError::CallbackUnavailable),
                }
            }
        }
        self.connections.retain_mut(|connection| {
            if Instant::now() >= connection.deadline {
                return false;
            }
            let mut buffer = [0_u8; 1024];
            for _ in 0..MAX_REQUEST / 1024 + 1 {
                match connection.stream.read(&mut buffer) {
                    Ok(0) => return false,
                    Ok(length) => {
                        if connection.bytes.len() + length > MAX_REQUEST {
                            respond(&mut connection.stream, 413);
                            return false;
                        }
                        connection.bytes.extend_from_slice(&buffer[..length]);
                        if connection.bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                            let status = handle_http(&connection.bytes, submission);
                            respond(&mut connection.stream, status);
                            return false;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
                    Err(_) => return false,
                }
            }
            true
        });
        Ok(())
    }
}

fn handle_http(bytes: &[u8], submission: &ManualSubmission) -> u16 {
    let Ok(request) = std::str::from_utf8(bytes) else {
        return 400;
    };
    let Some((head, body)) = request.split_once("\r\n\r\n") else {
        return 400;
    };
    if !body.is_empty() {
        return 400;
    }
    let mut lines = head.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or_default().split(' ').collect();
    if parts.len() != 3 || !matches!(parts[2], "HTTP/1.1" | "HTTP/1.0") {
        return 400;
    }
    if parts[0] != "GET" {
        return 405;
    }
    let target = parts[1];
    if target.split('?').next() != Some("/auth/callback") {
        return 404;
    }
    let mut host = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return 400;
        };
        if name.eq_ignore_ascii_case("host")
            && host.replace(value.trim()).is_some() {
                return 400;
            }
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value.trim() != "0")
        {
            return 400;
        }
    }
    if host != Some("localhost:1455") {
        return 400;
    }
    let uri = format!("http://localhost:1455{target}");
    match submission.accept_callback(&uri) {
        Ok(()) => 200,
        Err(AuthError::StalePrompt) => 409,
        Err(_) => 400,
    }
}

fn respond(stream: &mut TcpStream, status: u16) {
    // Never echo the URI, state, authorization code, or OAuth description.
    let text = if status == 200 {
        "Login response received. Return to zenpi."
    } else {
        "Login response rejected."
    };
    let response = format!(
        "HTTP/1.1 {status} Result\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    fn submission() -> ManualSubmission {
        ManualSubmission::new(LoginFlowId("flow".into()), 1, "state".into())
    }
    fn uri(query: &str) -> String {
        format!("{REDIRECT_URI}?{query}")
    }

    #[test]
    fn strict_manual_uri_and_wrong_state_do_not_consume() {
        let input = submission();
        for value in [
            "bare-code".to_owned(),
            uri("state=wrong&code=one"),
            uri("state=state&state=state&code=one"),
            uri("state=state&code=one#fragment"),
            "https://localhost:1455/auth/callback?state=state&code=one".into(),
            "http://evil.test:1455/auth/callback?state=state&code=one".into(),
        ] {
            assert!(
                input
                    .submit(&LoginFlowId("flow".into()), 1, &value)
                    .is_err()
            );
            assert!(input.take().unwrap().is_none());
        }
        input
            .submit(
                &LoginFlowId("flow".into()),
                1,
                &uri("state=state&code=winner"),
            )
            .unwrap();
        assert_eq!(input.take().unwrap(), Some(Ok("winner".into())));
        assert_eq!(
            input.accept_callback(&uri("state=state&code=late")),
            Err(AuthError::StalePrompt)
        );
    }

    #[test]
    fn oauth_error_with_matching_state_is_terminal_and_redacted() {
        let input = submission();
        input
            .accept_callback(&uri(
                "state=state&error=access_denied&error_description=synthetic-secret",
            ))
            .unwrap();
        assert_eq!(input.take().unwrap(), Some(Err(AuthError::AccessDenied)));
        assert_eq!(
            input.accept_callback(&uri("state=state&code=late")),
            Err(AuthError::StalePrompt)
        );
    }

    #[test]
    fn prompt_cancellation_is_not_flow_cancellation_and_late_flow_is_rejected() {
        let input = submission();
        assert_eq!(
            input.submit(
                &LoginFlowId("other".into()),
                1,
                &uri("state=state&code=one")
            ),
            Err(AuthError::StalePrompt)
        );
        input.cancel_prompt(&LoginFlowId("flow".into()), 1).unwrap();
        assert_eq!(
            input.submit(&LoginFlowId("flow".into()), 1, &uri("state=state&code=one")),
            Err(AuthError::StalePrompt)
        );
        input
            .accept_callback(&uri("state=state&code=callback"))
            .unwrap();
        assert_eq!(input.take().unwrap(), Some(Ok("callback".into())));
    }

    #[test]
    fn callback_and_manual_have_one_concurrent_winner() {
        let input = submission();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                input.submit(
                    &LoginFlowId("flow".into()),
                    1,
                    &uri("state=state&code=manual"),
                )
            });
            let second = scope.spawn(|| input.accept_callback(&uri("state=state&code=callback")));
            let successes = usize::from(first.join().unwrap().is_ok())
                + usize::from(second.join().unwrap().is_ok());
            assert_eq!(successes, 1);
        });
        assert!(input.take().unwrap().unwrap().is_ok());
        assert!(input.take().unwrap().is_none());
    }

    #[test]
    fn http_restricts_method_path_host_body_and_does_not_echo_secrets() {
        let input = submission();
        for request in [
            "POST /auth/callback?state=state&code=secret HTTP/1.1\r\nHost: localhost:1455\r\n\r\n",
            "GET /other?state=state&code=secret HTTP/1.1\r\nHost: localhost:1455\r\n\r\n",
            "GET /auth/callback?state=state&code=secret HTTP/1.1\r\nHost: evil.test\r\n\r\n",
            "GET /auth/callback?state=state&code=secret HTTP/1.1\r\nHost: localhost:1455\r\nTransfer-Encoding: chunked\r\n\r\n",
        ] {
            assert_ne!(handle_http(request.as_bytes(), &input), 200);
            assert!(input.take().unwrap().is_none());
        }
    }

    #[test]
    fn nonblocking_loopback_pump_accepts_and_releases_listener() {
        // Only this test-only listener uses a random port; the production bind
        // path above is fixed at the registered port and has no fallback.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut server = CallbackServer {
            listeners: vec![listener],
            connections: Vec::new(),
        };
        let input = submission();
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(b"GET /auth/callback?state=state&code=synthetic-code HTTP/1.1\r\nHost: localhost:1455\r\n\r\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        let result = loop {
            server.pump(&input).unwrap();
            if let Some(result) = input.take().unwrap() {
                break result;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(result, Ok("synthetic-code".into()));
        client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(!response.contains("synthetic-code"));
        drop(server);
        assert!(TcpListener::bind(address).is_ok());
    }

    #[test]
    fn oversized_uri_and_slow_partial_requests_do_not_consume_the_flow() {
        let input = submission();
        assert_eq!(
            input.accept_callback(&uri(&format!("state=state&code={}", "x".repeat(MAX_URI)))),
            Err(AuthError::InvalidCallback)
        );
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut server = CallbackServer {
            listeners: vec![listener],
            connections: Vec::new(),
        };
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(b"GET /auth/callback?").unwrap();
        server.pump(&input).unwrap();
        assert_eq!(server.connections.len(), 1);
        server.connections[0].deadline = Instant::now();
        server.pump(&input).unwrap();
        assert!(server.connections.is_empty());
        assert!(input.take().unwrap().is_none());
        let mut client = TcpStream::connect(address).unwrap();
        client.write_all(&vec![b'x'; MAX_REQUEST + 1]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            server.pump(&input).unwrap();
            if server.connections.is_empty() {
                break;
            }
            assert!(Instant::now() < deadline);
        }
        assert!(input.take().unwrap().is_none());
    }

    #[test]
    fn occupied_registered_port_reports_device_fallback_without_taking_it_over() {
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 1455));
        match occupied {
            Ok(_listener) => assert!(matches!(
                CallbackServer::bind(),
                Err(AuthError::CallbackPortInUse)
            )),
            Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                assert!(matches!(
                    CallbackServer::bind(),
                    Err(AuthError::CallbackPortInUse)
                ));
            }
            Err(error) => panic!("test loopback unavailable: {error}"),
        }
    }
}
