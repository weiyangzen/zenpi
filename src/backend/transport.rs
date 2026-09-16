//! Request-owned cancellation during DNS, TCP, TLS and response headers.
//! The HTTP parser and TLS verification remain ureq/rustls responsibilities.
use super::{BackendError, map_ureq_error};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use ureq::{
    config::Config,
    unversioned::{
        resolver::{ResolvedSocketAddrs, Resolver},
        transport::{
            Buffers, ConnectProxyConnector, ConnectionDetails, Connector, Either, LazyBuffers,
            NextTimeout, RustlsConnector, Transport,
        },
    },
};
const SLICE: Duration = Duration::from_millis(25);
#[derive(Clone, Debug, Default)]
pub(super) struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    fn check(&self) -> Result<(), ureq::Error> {
        if self.0.load(Ordering::Acquire) {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "provider request cancelled",
            )
            .into())
        } else {
            Ok(())
        }
    }
}
pub(super) fn client(config: Config) -> (ureq::Agent, Cancellation) {
    let cancel = Cancellation::default();
    let connector =
        ().chain(ConnectProxyConnector::default())
            .chain(CancelConnector(cancel.clone()))
            .chain(RustlsConnector::default());
    (
        ureq::Agent::with_parts(config, connector, CancelResolver(cancel.clone())),
        cancel,
    )
}
/// Only the send/header stage moves to an owned scoped worker. The borrowed
/// host callback stays on the calling thread; no lifetime erasure or detached
/// job is needed. The scope always joins, including unwinding/error paths.
pub(super) fn send_json(
    builder: ureq::RequestBuilder<ureq::typestate::WithBody>,
    body: &Value,
    cancelled: &dyn Fn() -> bool,
    cancel: Cancellation,
) -> Result<ureq::http::Response<ureq::Body>, BackendError> {
    if cancelled() {
        return Err(BackendError::Cancelled);
    }
    thread::scope(|scope| {
        struct Guard {
            cancel: Cancellation,
            armed: bool,
        }
        impl Drop for Guard {
            fn drop(&mut self) {
                if self.armed {
                    self.cancel.cancel();
                }
            }
        }
        let mut guard = Guard {
            cancel: cancel.clone(),
            armed: true,
        };
        let worker = thread::Builder::new()
            .name("zenpi-provider-send".into())
            .spawn_scoped(scope, move || builder.send_json(body))
            .map_err(|error| BackendError::Transport(error.to_string()))?;
        while !worker.is_finished() {
            if cancelled() {
                cancel.cancel();
            }
            thread::park_timeout(Duration::from_millis(5));
        }
        let response = worker
            .join()
            .map_err(|_| BackendError::Transport("provider send worker panicked".into()))?;
        if cancelled() || cancel.0.load(Ordering::Acquire) {
            return Err(BackendError::Cancelled);
        }
        // The response body moves to the caller, which retains its existing
        // bounded body reads and checks the original cancellation callback.
        guard.armed = false;
        response.map_err(map_ureq_error)
    })
}
fn deadline(timeout: NextTimeout) -> Option<Instant> {
    if timeout.after.is_not_happening() {
        None
    } else {
        Instant::now().checked_add(*timeout.after)
    }
}
fn remaining(end: Option<Instant>, reason: ureq::Timeout) -> Result<Duration, ureq::Error> {
    match end {
        Some(end) => {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(ureq::Error::Timeout(reason))
            } else {
                Ok(left.min(SLICE))
            }
        }
        None => Ok(SLICE),
    }
}
#[cfg(unix)]
fn ready(
    fd: std::os::fd::RawFd,
    events: libc::c_short,
    end: Option<Instant>,
    reason: ureq::Timeout,
    cancel: &Cancellation,
) -> Result<(), ureq::Error> {
    loop {
        cancel.check()?;
        let left = remaining(end, reason)?;
        let mut poll = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: poll points to one initialized record and borrows an owned fd.
        let result = unsafe { libc::poll(&mut poll, 1, left.as_millis().max(1) as i32) };
        if result > 0 {
            if poll.revents & libc::POLLNVAL != 0 {
                return Err(
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid provider socket").into(),
                );
            }
            cancel.check()?;
            return Ok(());
        }
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
    }
}
#[derive(Debug)]
struct CancelConnector(Cancellation);
impl<In: Transport> Connector<In> for CancelConnector {
    type Out = Either<In, CancelTransport>;
    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        if let Some(chained) = chained {
            return Ok(Some(Either::A(chained)));
        }
        self.0.check()?;
        let end = deadline(details.timeout);
        let mut last = None;
        for (index, addr) in details.addrs.iter().enumerate() {
            // Preserve a bounded share for later DNS addresses; one blackhole
            // address must not consume the entire connection deadline.
            let address_end = end.map(|end| {
                let now = Instant::now();
                let left = end.saturating_duration_since(now);
                let remaining_addresses = details.addrs.len() - index;
                let fraction = 1.0 / (2.0 - 0.5f64.powi(remaining_addresses as i32 - 1));
                now + left
                    .mul_f64(fraction)
                    .max(Duration::from_millis(10))
                    .min(left)
            });
            match connect_socket(*addr, address_end, details.timeout.reason, &self.0) {
                Ok(stream) => {
                    stream.set_nodelay(details.config.no_delay())?;
                    return Ok(Some(Either::B(CancelTransport {
                        stream,
                        buffers: LazyBuffers::new(
                            details.config.input_buffer_size(),
                            details.config.output_buffer_size(),
                        ),
                        cancel: self.0.clone(),
                    })));
                }
                Err(e) => {
                    self.0.check()?;
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or(ureq::Error::HostNotFound))
    }
}
#[cfg(unix)]
fn connect_socket(
    addr: SocketAddr,
    end: Option<Instant>,
    reason: ureq::Timeout,
    cancel: &Cancellation,
) -> Result<TcpStream, ureq::Error> {
    use std::os::fd::{AsRawFd, FromRawFd};
    cancel.check()?;
    remaining(end, reason)?;
    // SAFETY: socket returns a fresh descriptor; immediately owned by TcpStream
    // on success so every cancellation/error path closes it exactly once.
    #[cfg(target_os = "linux")]
    let socket_kind = libc::SOCK_STREAM | libc::SOCK_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let socket_kind = libc::SOCK_STREAM;
    let raw = unsafe {
        libc::socket(
            if addr.is_ipv4() {
                libc::AF_INET
            } else {
                libc::AF_INET6
            },
            socket_kind,
            0,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let stream = unsafe { TcpStream::from_raw_fd(raw) };
    stream.set_nonblocking(true)?;
    // Match std socket inheritance guarantees before any concurrent exec.
    if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error().into());
    }
    let result = match addr {
        SocketAddr::V4(a) => {
            let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sa.sin_family = libc::AF_INET as _;
            sa.sin_port = a.port().to_be();
            sa.sin_addr.s_addr = u32::from_ne_bytes(a.ip().octets());
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd"
            ))]
            {
                sa.sin_len = std::mem::size_of_val(&sa) as _;
            }
            unsafe {
                libc::connect(
                    raw,
                    (&sa as *const libc::sockaddr_in).cast(),
                    std::mem::size_of_val(&sa) as _,
                )
            }
        }
        SocketAddr::V6(a) => {
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as _;
            sa.sin6_port = a.port().to_be();
            sa.sin6_addr.s6_addr = a.ip().octets();
            sa.sin6_flowinfo = a.flowinfo();
            sa.sin6_scope_id = a.scope_id();
            #[cfg(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd"
            ))]
            {
                sa.sin6_len = std::mem::size_of_val(&sa) as _;
            }
            unsafe {
                libc::connect(
                    raw,
                    (&sa as *const libc::sockaddr_in6).cast(),
                    std::mem::size_of_val(&sa) as _,
                )
            }
        }
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(libc::EINPROGRESS) | Some(libc::EWOULDBLOCK) | Some(libc::EINTR)
        ) {
            return Err(error.into());
        }
        ready(stream.as_raw_fd(), libc::POLLOUT, end, reason, cancel)?;
        if let Some(error) = stream.take_error()? {
            return Err(error.into());
        }
    }
    cancel.check()?;
    Ok(stream)
}
// Non-Unix builds preserve a finite connection timeout. They do not claim
// Unix poll cancellation; those platforms require their native event adapter.
#[cfg(not(unix))]
fn connect_socket(
    addr: SocketAddr,
    end: Option<Instant>,
    reason: ureq::Timeout,
    cancel: &Cancellation,
) -> Result<TcpStream, ureq::Error> {
    cancel.check()?;
    remaining(end, reason)?;
    let stream = TcpStream::connect_timeout(
        &addr,
        end.map(|e| e.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(120)),
    )?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}
#[derive(Debug)]
struct CancelTransport {
    stream: TcpStream,
    buffers: LazyBuffers,
    cancel: Cancellation,
}
impl CancelTransport {
    fn wait(
        &self,
        write: bool,
        end: Option<Instant>,
        reason: ureq::Timeout,
    ) -> Result<(), ureq::Error> {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            ready(
                self.stream.as_raw_fd(),
                if write { libc::POLLOUT } else { libc::POLLIN },
                end,
                reason,
                &self.cancel,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = write;
            self.cancel.check()?;
            thread::sleep(remaining(end, reason)?);
            Ok(())
        }
    }
}
impl Transport for CancelTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        let end = deadline(timeout);
        let mut sent = 0;
        while sent < amount {
            self.cancel.check()?;
            match self.stream.write(&self.buffers.output()[sent..amount]) {
                Ok(0) => {
                    return Err(
                        io::Error::new(io::ErrorKind::WriteZero, "provider socket closed").into(),
                    );
                }
                Ok(n) => sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.wait(true, end, timeout.reason)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let end = deadline(timeout);
        loop {
            self.cancel.check()?;
            match self.stream.read(self.buffers.input_append_buf()) {
                Ok(n) => {
                    self.buffers.input_appended(n);
                    return Ok(n > 0);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    self.wait(false, end, timeout.reason)?
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn is_open(&mut self) -> bool {
        let mut b = [0];
        matches!(self.stream.peek(&mut b),Err(error)if error.kind()==io::ErrorKind::WouldBlock)
    }
}
#[derive(Debug)]
struct CancelResolver(Cancellation);
impl Resolver for CancelResolver {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        self.0.check()?;
        let host = uri
            .host()
            .ok_or(ureq::Error::HostNotFound)?
            .trim_matches(['[', ']']);
        let port = uri
            .port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            });
        let mut result = self.empty();
        if let Ok(ip) = host.parse::<IpAddr>() {
            for addr in config
                .ip_family()
                .keep_wanted([SocketAddr::new(ip, port)].into_iter())
            {
                result.push(addr);
            }
            return if result.is_empty() {
                Err(ureq::Error::HostNotFound)
            } else {
                Ok(result)
            };
        }
        #[cfg(target_os = "macos")]
        {
            for addr in config
                .ip_family()
                .keep_wanted(dns::resolve(host, port, timeout, &self.0)?.into_iter())
                .take(16)
            {
                result.push(addr);
            }
            if result.is_empty() {
                Err(ureq::Error::HostNotFound)
            } else {
                Ok(result)
            }
        }
        // DefaultResolver's DNS timeout uses a detached thread on these
        // platforms; this limitation is explicit pending native resolver work.
        #[cfg(not(target_os = "macos"))]
        {
            ureq::unversioned::resolver::DefaultResolver::default().resolve(uri, config, timeout)
        }
    }
}
#[cfg(target_os = "macos")]
mod dns {
    use super::*;
    use std::{
        ffi::{CString, c_char, c_void},
        net::{Ipv4Addr, Ipv6Addr, SocketAddrV6},
        ptr,
    };
    type Ref = *mut c_void;
    type Reply = unsafe extern "C" fn(
        Ref,
        u32,
        u32,
        i32,
        *const c_char,
        *const libc::sockaddr,
        u32,
        *mut c_void,
    );
    #[link(name = "System")]
    unsafe extern "C" {
        fn DNSServiceGetAddrInfo(
            sd: *mut Ref,
            flags: u32,
            interface: u32,
            protocol: u32,
            host: *const c_char,
            reply: Reply,
            context: *mut c_void,
        ) -> i32;
        fn DNSServiceRefSockFD(sd: Ref) -> i32;
        fn DNSServiceProcessResult(sd: Ref) -> i32;
        fn DNSServiceRefDeallocate(sd: Ref);
    }
    struct Query(Ref);
    impl Drop for Query {
        fn drop(&mut self) {
            unsafe { DNSServiceRefDeallocate(self.0) }
        }
    }
    #[derive(Default)]
    struct State {
        addresses: Vec<SocketAddr>,
        done: bool,
        error: bool,
        port: u16,
    }
    unsafe extern "C" fn reply(
        _: Ref,
        flags: u32,
        _: u32,
        error: i32,
        _: *const c_char,
        address: *const libc::sockaddr,
        _: u32,
        context: *mut c_void,
    ) {
        // SAFETY: each query borrows its stable boxed State; callbacks are only
        // invoked synchronously by ProcessResult before Query is deallocated.
        let state = unsafe { &mut *context.cast::<State>() };
        if error != 0 {
            state.error = true;
            state.done = true;
            return;
        }
        if address.is_null() {
            state.error = true;
            state.done = true;
            return;
        }
        let addr = match unsafe { (*address).sa_family } as i32 {
            libc::AF_INET => {
                let a = unsafe { &*address.cast::<libc::sockaddr_in>() };
                SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::from(a.sin_addr.s_addr.to_ne_bytes())),
                    state.port,
                )
            }
            libc::AF_INET6 => {
                let a = unsafe { &*address.cast::<libc::sockaddr_in6>() };
                SocketAddr::V6(SocketAddrV6::new(
                    Ipv6Addr::from(a.sin6_addr.s6_addr),
                    state.port,
                    a.sin6_flowinfo,
                    a.sin6_scope_id,
                ))
            }
            _ => {
                state.error = true;
                state.done = true;
                return;
            }
        };
        if flags & 2 != 0 && state.addresses.len() < 16 && !state.addresses.contains(&addr) {
            state.addresses.push(addr);
        }
        state.done = flags & 1 == 0;
    }
    pub(super) fn resolve(
        host: &str,
        port: u16,
        timeout: NextTimeout,
        cancel: &Cancellation,
    ) -> Result<Vec<SocketAddr>, ureq::Error> {
        let host = CString::new(host).map_err(|_| ureq::Error::HostNotFound)?;
        let end = deadline(timeout);
        let mut states = [
            Box::new(State {
                port,
                ..State::default()
            }),
            Box::new(State {
                port,
                ..State::default()
            }),
        ];
        let mut queries = vec![];
        // Separate families provide a finite initial-result boundary for each
        // query, including NoSuchRecord, while preserving both address types.
        for (i, state) in states.iter_mut().enumerate() {
            cancel.check()?;
            let mut raw = ptr::null_mut();
            let status = unsafe {
                DNSServiceGetAddrInfo(
                    &mut raw,
                    0,
                    0,
                    if i == 0 { 1 } else { 2 },
                    host.as_ptr(),
                    reply,
                    (&mut **state as *mut State).cast(),
                )
            };
            if status != 0 || raw.is_null() {
                return Err(ureq::Error::HostNotFound);
            }
            queries.push(Query(raw));
        }
        while states.iter().any(|s| !s.done) {
            cancel.check()?;
            let wait = remaining(end, timeout.reason)?;
            let mut fds = [libc::pollfd {
                fd: -1,
                events: libc::POLLIN,
                revents: 0,
            }; 2];
            for (i, q) in queries.iter().enumerate() {
                if !states[i].done {
                    fds[i].fd = unsafe { DNSServiceRefSockFD(q.0) };
                    if fds[i].fd < 0 {
                        return Err(ureq::Error::HostNotFound);
                    }
                }
            }
            let result = unsafe {
                libc::poll(
                    fds.as_mut_ptr(),
                    fds.len() as _,
                    wait.as_millis().max(1) as i32,
                )
            };
            if result < 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::Interrupted {
                    return Err(e.into());
                }
                continue;
            }
            for (i, fd) in fds.iter().enumerate() {
                if fd.revents != 0
                    && (fd.revents & libc::POLLIN == 0
                        || unsafe { DNSServiceProcessResult(queries[i].0) } != 0)
                {
                    return Err(ureq::Error::HostNotFound);
                }
            }
        }
        cancel.check()?;
        let addresses = states
            .iter_mut()
            .flat_map(|s| std::mem::take(&mut s.addresses))
            .take(16)
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            Err(ureq::Error::HostNotFound)
        } else {
            Ok(addresses)
        }
    }
}
