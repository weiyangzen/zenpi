use serde_json::json;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use zenpi::{
    backend::{Backend, BackendError, CompletionRequest, OpenAiCompatibleBackend, OpenAiWireApi},
    core::{Turn, TurnRole},
};
fn read_request(stream: &mut TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = vec![];
    let mut chunk = [0; 4096];
    loop {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        assert!(bytes.len() < 1024 * 1024);
        if let Some(i) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&bytes[..i]);
            let length = header
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if bytes.len() >= i + 4 + length {
                break;
            }
        }
    }
}
fn backend(url: String, wire: OpenAiWireApi) -> OpenAiCompatibleBackend {
    OpenAiCompatibleBackend::from_values_with_settings_and_timeout(
        url,
        None,
        match wire {
            OpenAiWireApi::AnthropicMessages => "claude-sonnet-4-6",
            OpenAiWireApi::GoogleGenerativeAi => "gemini-2.5-flash",
            _ => "gpt-4o-mini",
        }
        .into(),
        wire,
        None,
        None,
        Duration::from_secs(4),
    )
    .unwrap()
    .with_max_retries(2)
    .unwrap()
}
fn cancelled_connection(wire: OpenAiWireApi, tls: bool, partial: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "{}://{}",
        if tls { "https" } else { "http" },
        listener.local_addr().unwrap()
    );
    let (started_tx, started_rx) = mpsc::channel();
    let (closed_tx, closed_rx) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        if tls {
            let mut hello = [0; 4096];
            assert!(stream.read(&mut hello).unwrap() > 0);
        } else {
            read_request(&mut stream);
            if partial {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Ty").unwrap();
                stream.flush().unwrap();
            }
        }
        started_tx.send(()).unwrap();
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                ) => {}
            other => panic!("connection survived request cancellation: {other:?}"),
        };
        listener.set_nonblocking(true).unwrap();
        assert!(matches!(listener.accept(),Err(e)if e.kind()==std::io::ErrorKind::WouldBlock));
        closed_tx.send(()).unwrap();
    });
    let flag = Arc::new(AtomicBool::new(false));
    let cancel = flag.clone();
    let worker = thread::spawn(move || {
        let mut events = 0;
        let result = backend(url, wire).complete_with_control(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
            &|| cancel.load(Ordering::Acquire),
            &mut |_| {
                events += 1;
                Ok(())
            },
        );
        (result, events)
    });
    started_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let now = Instant::now();
    flag.store(true, Ordering::Release);
    let (result, events) = worker.join().unwrap();
    assert!(matches!(result, Err(BackendError::Cancelled)), "{result:?}");
    assert_eq!(events, 0);
    assert!(
        now.elapsed() < Duration::from_millis(700),
        "{:?}",
        now.elapsed()
    );
    closed_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    peer.join().unwrap();
}
#[test]
fn cancel_before_response_headers_closes_each_native_and_openai_wire() {
    for wire in [
        OpenAiWireApi::ChatCompletions,
        OpenAiWireApi::Responses,
        OpenAiWireApi::AnthropicMessages,
        OpenAiWireApi::GoogleGenerativeAi,
    ] {
        cancelled_connection(wire, false, false);
    }
}
#[test]
fn cancellation_during_partial_headers_closes_the_same_request() {
    cancelled_connection(OpenAiWireApi::Responses, false, true);
}
#[test]
fn cancellation_during_tls_handshake_closes_tcp_and_joins_send_owner() {
    cancelled_connection(OpenAiWireApi::Responses, true, false);
}
#[test]
fn pre_cancelled_request_opens_no_socket() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let b = backend(
        format!("http://{}", listener.local_addr().unwrap()),
        OpenAiWireApi::ChatCompletions,
    );
    assert!(matches!(
        b.complete_with_control(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
            &|| true,
            &mut |_| Ok(())
        ),
        Err(BackendError::Cancelled)
    ));
    listener.set_nonblocking(true).unwrap();
    assert!(matches!(listener.accept(),Err(e)if e.kind()==std::io::ErrorKind::WouldBlock));
}
fn slow_headers(hostname: bool) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = if hostname {
        format!("http://localhost:{}", listener.local_addr().unwrap().port())
    } else {
        format!("http://{}", listener.local_addr().unwrap())
    };
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        let body=json!({"choices":[{"message":{"content":"slow headers are valid"},"finish_reason":"stop"}]}).to_string();
        stream.write_all(b"HTTP/1.1 200 OK\r\n").unwrap();
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(230));
        write!(stream,"Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    let c = backend(url, OpenAiWireApi::ChatCompletions)
        .complete(CompletionRequest::new(
            "u",
            &[Turn::new("u", TurnRole::User, "go")],
            None,
            &[],
        ))
        .unwrap();
    assert_eq!(c.content, "slow headers are valid");
    peer.join().unwrap();
}
#[test]
fn normal_slow_headers_do_not_become_a_short_timeout() {
    slow_headers(false);
}
#[test]
fn system_hostname_resolution_and_slow_headers_use_real_http() {
    slow_headers(true);
}

#[cfg(target_os = "macos")]
#[test]
fn cancellation_during_system_dns_query_returns_without_a_background_resolver_thread() {
    let host = format!(
        "zenpi-cancel-{}-{}.local",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let b = backend(format!("http://{host}:9"), OpenAiWireApi::Responses);
    let flag = Arc::new(AtomicBool::new(false));
    let cancel = flag.clone();
    let trigger = thread::spawn(move || {
        thread::sleep(Duration::from_millis(60));
        cancel.store(true, Ordering::Release);
    });
    let now = Instant::now();
    let result = b.complete_with_control(
        CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
        &|| flag.load(Ordering::Acquire),
        &mut |_| Ok(()),
    );
    trigger.join().unwrap();
    assert!(matches!(result, Err(BackendError::Cancelled)), "{result:?}");
    assert!(
        now.elapsed() < Duration::from_millis(700),
        "{:?}",
        now.elapsed()
    );
}

#[test]
fn cancellation_callback_unwind_still_closes_connection_and_joins_send_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let ready = Arc::new(AtomicBool::new(false));
    let signal = ready.clone();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
        signal.store(true, Ordering::Release);
        let mut b = [0];
        assert_eq!(stream.read(&mut b).unwrap(), 0);
    });
    let now = Instant::now();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        backend(url, OpenAiWireApi::Responses).complete_with_control(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
            &|| {
                assert!(!ready.load(Ordering::Acquire), "fixture callback unwind");
                false
            },
            &mut |_| Ok(()),
        )
    }));
    assert!(result.is_err());
    assert!(now.elapsed() < Duration::from_millis(700));
    peer.join().unwrap();
}

#[test]
fn cancelled_send_owner_does_not_poison_next_request_on_same_backend() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let b = backend(
        format!("http://{}", listener.local_addr().unwrap()),
        OpenAiWireApi::ChatCompletions,
    );
    let (tx, rx) = mpsc::channel();
    let peer = thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        read_request(&mut first);
        tx.send(()).unwrap();
        let mut byte = [0];
        assert_eq!(first.read(&mut byte).unwrap(), 0);
        let (mut second, _) = listener.accept().unwrap();
        read_request(&mut second);
        let body =
            json!({"choices":[{"message":{"content":"fresh owner"},"finish_reason":"stop"}]})
                .to_string();
        write!(second,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
    });
    let flag = Arc::new(AtomicBool::new(false));
    let signal = flag.clone();
    let trigger = thread::spawn(move || {
        rx.recv_timeout(Duration::from_secs(3)).unwrap();
        signal.store(true, Ordering::Release);
    });
    assert!(matches!(
        b.complete_with_control(
            CompletionRequest::new("u", &[Turn::new("u", TurnRole::User, "go")], None, &[]),
            &|| flag.load(Ordering::Acquire),
            &mut |_| Ok(())
        ),
        Err(BackendError::Cancelled)
    ));
    trigger.join().unwrap();
    assert_eq!(
        b.complete(CompletionRequest::new(
            "v",
            &[Turn::new("v", TurnRole::User, "again")],
            None,
            &[]
        ))
        .unwrap()
        .content,
        "fresh owner"
    );
    peer.join().unwrap();
}
