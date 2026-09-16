use std::{collections::VecDeque, io, time::Duration};

use mio::{Events, Interest, Poll, Token, unix::SourceFd};
use signal_hook_mio::v1_0::Signals;

#[cfg(feature = "event-stream")]
use crate::event::sys::Waker;
use crate::event::{
    Event, InternalEvent, source::EventSource, sys::unix::parse::parse_event, timeout::PollTimeout,
};
use crate::terminal::sys::file_descriptor::{FileDesc, tty_fd};

// Tokens to identify file descriptor
const TTY_TOKEN: Token = Token(0);
const SIGNAL_TOKEN: Token = Token(1);
#[cfg(feature = "event-stream")]
const WAKE_TOKEN: Token = Token(2);

// I (@zrzka) wasn't able to read more than 1_022 bytes when testing
// reading on macOS/Linux -> we don't need bigger buffer and 1k of bytes
// is enough.
const TTY_BUFFER_SIZE: usize = 1_024;

pub(crate) struct UnixInternalEventSource {
    poll: Poll,
    events: Events,
    pending_tokens: VecDeque<Token>,
    tty_read_started: bool,
    parser: Parser,
    tty_buffer: [u8; TTY_BUFFER_SIZE],
    tty_fd: FileDesc<'static>,
    signals: Signals,
    #[cfg(feature = "event-stream")]
    waker: Waker,
}

impl UnixInternalEventSource {
    pub fn new() -> io::Result<Self> {
        UnixInternalEventSource::from_file_descriptor(tty_fd()?)
    }

    pub(crate) fn from_file_descriptor(input_fd: FileDesc<'static>) -> io::Result<Self> {
        let poll = Poll::new()?;
        let registry = poll.registry();

        let tty_raw_fd = input_fd.raw_fd();
        let mut tty_ev = SourceFd(&tty_raw_fd);
        registry.register(&mut tty_ev, TTY_TOKEN, Interest::READABLE)?;

        let mut signals = Signals::new([signal_hook::consts::SIGWINCH])?;
        registry.register(&mut signals, SIGNAL_TOKEN, Interest::READABLE)?;

        #[cfg(feature = "event-stream")]
        let waker = Waker::new(registry, WAKE_TOKEN)?;

        Ok(UnixInternalEventSource {
            poll,
            events: Events::with_capacity(3),
            pending_tokens: VecDeque::with_capacity(3),
            tty_read_started: false,
            parser: Parser::default(),
            tty_buffer: [0u8; TTY_BUFFER_SIZE],
            tty_fd: input_fd,
            signals,
            #[cfg(feature = "event-stream")]
            waker,
        })
    }
}

impl UnixInternalEventSource {
    fn tty_bytes_available(&self) -> io::Result<u64> {
        #[cfg(not(feature = "libc"))]
        {
            Ok(rustix::io::ioctl_fionread(&self.tty_fd)?)
        }
        #[cfg(feature = "libc")]
        {
            let mut count: libc::c_int = 0;
            // FIONREAD only observes this owned/borrowed descriptor. It does
            // not change shared stdin flags or raw-mode settings.
            if unsafe { libc::ioctl(self.tty_fd.raw_fd(), libc::FIONREAD, &mut count) } == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(count.max(0) as u64)
        }
    }
}

impl EventSource for UnixInternalEventSource {
    fn try_read(&mut self, timeout: Option<Duration>) -> io::Result<Option<InternalEvent>> {
        if let Some(event) = self.parser.next() {
            return Ok(Some(event));
        }

        let timeout = PollTimeout::new(timeout);

        loop {
            // A return from one token must not discard the rest of this
            // readiness batch. Edge-triggered poll may not notify us again.
            if self.pending_tokens.is_empty() {
                if let Err(e) = self.poll.poll(&mut self.events, timeout.leftover()) {
                    // Mio will throw an interrupted error in case of cursor position retrieval. We need to retry until it succeeds.
                    // Previous versions of Mio (< 0.7) would automatically retry the poll call if it was interrupted (if EINTR was returned).
                    // https://docs.rs/mio/0.7.0/mio/struct.Poll.html#notes
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    } else {
                        return Err(e);
                    }
                };

                if self.events.is_empty() {
                    // No readiness events = timeout
                    return Ok(None);
                }
                self.pending_tokens
                    .extend(self.events.iter().map(|event| event.token()));
            }

            while let Some(&token) = self.pending_tokens.front() {
                match token {
                    TTY_TOKEN => {
                        loop {
                            // stdin may be blocking. After returning a parsed
                            // event, keep this token until its bytes are drained,
                            // but never perform a speculative blocking read.
                            if self.tty_read_started && self.tty_bytes_available()? == 0 {
                                self.pending_tokens.pop_front();
                                self.tty_read_started = false;
                                break;
                            }
                            match self.tty_fd.read(&mut self.tty_buffer) {
                                Ok(0) => {
                                    self.pending_tokens.pop_front();
                                    self.tty_read_started = false;
                                    return Err(io::Error::new(
                                        io::ErrorKind::UnexpectedEof,
                                        "terminal input closed",
                                    ));
                                }
                                Ok(read_count) => {
                                    self.tty_read_started = true;
                                    if read_count > 0 {
                                        self.parser.advance(
                                            &self.tty_buffer[..read_count],
                                            read_count == TTY_BUFFER_SIZE,
                                        );
                                    }
                                }
                                Err(e) => {
                                    // No more data to read at the moment. We will receive another event
                                    if e.kind() == io::ErrorKind::WouldBlock {
                                        self.pending_tokens.pop_front();
                                        self.tty_read_started = false;
                                        break;
                                    }
                                    // once more data is available to read.
                                    else if e.kind() == io::ErrorKind::Interrupted {
                                        continue;
                                    } else {
                                        self.pending_tokens.pop_front();
                                        self.tty_read_started = false;
                                        return Err(e);
                                    }
                                }
                            };

                            if let Some(event) = self.parser.next() {
                                return Ok(Some(event));
                            }
                        }
                        // A full read can leave a lone Esc undecided. Only
                        // resolve it once this TTY readiness has been drained;
                        // a queued suffix must still be parsed as its sequence.
                        if let Some(event) = self.parser.finish_pending_escape() {
                            return Ok(Some(event));
                        }
                    }
                    SIGNAL_TOKEN => {
                        self.pending_tokens.pop_front();
                        if self.signals.pending().next() == Some(signal_hook::consts::SIGWINCH) {
                            // TODO Should we remove tput?
                            //
                            // This can take a really long time, because terminal::size can
                            // launch new process (tput) and then it parses its output. It's
                            // not a really long time from the absolute time point of view, but
                            // it's a really long time from the mio, async-std/tokio executor, ...
                            // point of view.
                            let new_size = crate::terminal::size()?;
                            return Ok(Some(InternalEvent::Event(Event::Resize(
                                new_size.0, new_size.1,
                            ))));
                        }
                    }
                    #[cfg(feature = "event-stream")]
                    WAKE_TOKEN => {
                        self.pending_tokens.pop_front();
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "Poll operation was woken up by `Waker::wake`",
                        ));
                    }
                    _ => unreachable!("Synchronize Evented handle registration & token handling"),
                }
            }

            // Processing above can take some time, check if timeout expired
            if timeout.elapsed() {
                return Ok(None);
            }
        }
    }

    #[cfg(feature = "event-stream")]
    fn waker(&self) -> Waker {
        self.waker.clone()
    }
}

//
// Following `Parser` structure exists for two reasons:
//
//  * mimic anes Parser interface
//  * move the advancing, parsing, ... stuff out of the `try_read` method
//
#[derive(Debug)]
struct Parser {
    buffer: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            // This buffer is used for -> 1 <- ANSI escape sequence. Are we
            // aware of any ANSI escape sequence that is bigger? Can we make
            // it smaller?
            //
            // Probably not worth spending more time on this as "there's a plan"
            // to use the anes crate parser.
            buffer: Vec::with_capacity(256),
            // TTY_BUFFER_SIZE is 1_024 bytes. How many ANSI escape sequences can
            // fit? What is an average sequence length? Let's guess here
            // and say that the average ANSI escape sequence length is 8 bytes. Thus
            // the buffer size should be 1024/8=128 to avoid additional allocations
            // when processing large amounts of data.
            //
            // There's no need to make it bigger, because when you look at the `try_read`
            // method implementation, all events are consumed before the next TTY_BUFFER
            // is processed -> events pushed.
            internal_events: VecDeque::with_capacity(128),
        }
    }
}

impl Parser {
    fn finish_pending_escape(&mut self) -> Option<InternalEvent> {
        if self.buffer.as_slice() != b"\x1b" {
            return None;
        }
        let event = parse_event(&self.buffer, false).ok().flatten()?;
        self.buffer.clear();
        Some(event)
    }

    fn advance(&mut self, buffer: &[u8], more: bool) {
        for (idx, byte) in buffer.iter().enumerate() {
            let more = idx + 1 < buffer.len() || more;

            self.buffer.push(*byte);

            match parse_event(&self.buffer, more) {
                Ok(Some(ie)) => {
                    self.internal_events.push_back(ie);
                    self.buffer.clear();
                }
                Ok(None) => {
                    // Event can't be parsed, because we don't have enough bytes for
                    // the current sequence. Keep the buffer and process next bytes.
                }
                Err(_) => {
                    // Event can't be parsed (not enough parameters, parameter is not a number, ...).
                    // Clear the buffer and continue with another sequence.
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_events.pop_front()
    }
}

#[cfg(test)]
mod zenpi_ready_tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    fn pair() -> (UnixInternalEventSource, UnixStream) {
        let (input, output) = UnixStream::pair().unwrap();
        input.set_nonblocking(true).unwrap();
        #[cfg(feature = "libc")]
        let fd = {
            use std::os::fd::IntoRawFd;
            FileDesc::new(input.into_raw_fd(), true)
        };
        #[cfg(not(feature = "libc"))]
        let fd = FileDesc::Owned(input.into());
        (
            UnixInternalEventSource::from_file_descriptor(fd).unwrap(),
            output,
        )
    }

    fn key(ch: char) -> InternalEvent {
        InternalEvent::Event(Event::Key(KeyEvent::new(
            KeyCode::Char(ch),
            KeyModifiers::NONE,
        )))
    }

    #[test]
    fn single_edge_over_1024_bytes_delivers_every_key_without_another_write() {
        let (mut source, mut output) = pair();
        output
            .write_all(&vec![b'x'; TTY_BUFFER_SIZE * 2 + 17])
            .unwrap();
        for index in 0..TTY_BUFFER_SIZE * 2 + 17 {
            assert_eq!(
                source.try_read(Some(Duration::from_millis(20))).unwrap(),
                Some(key('x')),
                "key {index}"
            );
        }
        assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
    }

    #[test]
    fn single_edge_keeps_split_utf8_escape_and_bracketed_paste() {
        let (mut source, mut output) = pair();
        let mut bytes = vec![b'x'; TTY_BUFFER_SIZE - 1];
        bytes.extend_from_slice("界\x1b[D\x1b[200~".as_bytes());
        let payload = "e\u{301}👨‍👩‍👧‍👦".repeat(150);
        bytes.extend_from_slice(payload.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~y");
        output.write_all(&bytes).unwrap();
        for _ in 0..TTY_BUFFER_SIZE - 1 {
            assert_eq!(
                source.try_read(Some(Duration::from_millis(20))).unwrap(),
                Some(key('x'))
            );
        }
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(key('界'))
        );
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(InternalEvent::Event(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::NONE
            ))))
        );
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(InternalEvent::Event(Event::Paste(payload)))
        );
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(key('y'))
        );
        assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
    }
    #[test]
    fn blocking_descriptor_drains_then_timeout_and_eof_are_bounded() {
        let (input, mut output) = UnixStream::pair().unwrap();
        #[cfg(feature = "libc")]
        let fd = {
            use std::os::fd::IntoRawFd;
            FileDesc::new(input.into_raw_fd(), true)
        };
        #[cfg(not(feature = "libc"))]
        let fd = FileDesc::Owned(input.into());
        let mut source = UnixInternalEventSource::from_file_descriptor(fd).unwrap();
        output.write_all(b"x").unwrap();
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(key('x'))
        );
        let start = std::time::Instant::now();
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            None
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(output);
        let error = source
            .try_read(Some(Duration::from_millis(20)))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn partial_input_waits_for_next_write_without_discarding_parser() {
        let (mut source, mut output) = pair();
        output.write_all(&[0xe7]).unwrap();
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            None
        );
        output.write_all(&[0x95, 0x8c]).unwrap();
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(key('界'))
        );
        output.write_all(b"\x1b[").unwrap();
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            None
        );
        output.write_all(b"D").unwrap();
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(InternalEvent::Event(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::NONE
            ))))
        );
    }

    #[cfg(feature = "event-stream")]
    #[test]
    fn pending_kernel_wake_and_tty_batch_survives_both_return_orders() {
        for wake_first in [true, false] {
            let (mut source, mut output) = pair();
            output.write_all(b"x").unwrap();
            source.waker().wake().unwrap();
            source
                .poll
                .poll(&mut source.events, Some(Duration::from_millis(20)))
                .unwrap();
            source
                .pending_tokens
                .extend(source.events.iter().map(|event| event.token()));
            assert!(source.pending_tokens.contains(&TTY_TOKEN));
            assert!(source.pending_tokens.contains(&WAKE_TOKEN));
            source
                .pending_tokens
                .make_contiguous()
                .sort_by_key(|token| {
                    if (*token == WAKE_TOKEN) == wake_first {
                        0
                    } else {
                        1
                    }
                });
            source.parser.advance(b"q", false);
            assert_eq!(
                source.try_read(Some(Duration::ZERO)).unwrap(),
                Some(key('q'))
            );
            if wake_first {
                assert_eq!(
                    source.try_read(Some(Duration::ZERO)).unwrap_err().kind(),
                    io::ErrorKind::Interrupted
                );
                assert_eq!(
                    source.try_read(Some(Duration::ZERO)).unwrap(),
                    Some(key('x'))
                );
            } else {
                assert_eq!(
                    source.try_read(Some(Duration::ZERO)).unwrap(),
                    Some(key('x'))
                );
                assert_eq!(
                    source.try_read(Some(Duration::ZERO)).unwrap_err().kind(),
                    io::ErrorKind::Interrupted
                );
            }
            assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
        }
    }
}

#[cfg(test)]
mod zenpi_escape_boundary_tests {
    use super::*;
    use crate::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::{io::Write, os::unix::net::UnixStream};

    fn pair(blocking: bool) -> (UnixInternalEventSource, UnixStream) {
        let (input, output) = UnixStream::pair().unwrap();
        input.set_nonblocking(!blocking).unwrap();
        #[cfg(feature = "libc")]
        let fd = {
            use std::os::fd::IntoRawFd;
            FileDesc::new(input.into_raw_fd(), true)
        };
        #[cfg(not(feature = "libc"))]
        let fd = FileDesc::Owned(input.into());
        (
            UnixInternalEventSource::from_file_descriptor(fd).unwrap(),
            output,
        )
    }

    fn key(code: KeyCode) -> InternalEvent {
        InternalEvent::Event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn drain_x(source: &mut UnixInternalEventSource, count: usize) {
        for index in 0..count {
            assert_eq!(
                source.try_read(Some(Duration::from_millis(20))).unwrap(),
                Some(key(KeyCode::Char('x'))),
                "ASCII index {index}"
            );
        }
    }

    #[test]
    fn one_write_full_buffer_trailing_escape_is_delivered_without_another_event() {
        for blocking in [false, true] {
            let (mut source, mut output) = pair(blocking);
            let mut bytes = vec![b'x'; TTY_BUFFER_SIZE - 1];
            bytes.push(0x1b);
            output.write_all(&bytes).unwrap();
            drain_x(&mut source, TTY_BUFFER_SIZE - 1);
            let start = std::time::Instant::now();
            let event = source.try_read(Some(Duration::from_millis(20))).unwrap();
            assert_eq!(
                event,
                Some(key(KeyCode::Esc)),
                "blocking={blocking}, after exactly one write"
            );
            assert!(start.elapsed() < Duration::from_secs(1));
            assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
            drop(output);
            assert_eq!(
                source
                    .try_read(Some(Duration::from_millis(20)))
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
    }

    #[test]
    fn full_buffer_escape_uses_already_pending_alt_suffix() {
        let (mut source, mut output) = pair(false);
        let mut bytes = vec![b'x'; TTY_BUFFER_SIZE - 1];
        bytes.extend_from_slice(b"\x1ba");
        output.write_all(&bytes).unwrap();
        drain_x(&mut source, TTY_BUFFER_SIZE - 1);
        assert_eq!(
            source.try_read(Some(Duration::from_millis(20))).unwrap(),
            Some(InternalEvent::Event(Event::Key(KeyEvent::new(
                KeyCode::Char('a'),
                KeyModifiers::ALT
            ))))
        );
        assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
    }

    #[test]
    fn full_buffer_partial_utf8_csi_and_paste_survive_idle() {
        let cases: Vec<(&[u8], &[u8], InternalEvent)> = vec![
            (&[0xe7], &[0x95, 0x8c], key(KeyCode::Char('界'))),
            (b"\x1b[", b"D", key(KeyCode::Left)),
            (
                b"\x1b[200~",
                "e\u{301}👨‍👩‍👧‍👦\x1b[201~".as_bytes(),
                InternalEvent::Event(Event::Paste("e\u{301}👨‍👩‍👧‍👦".to_string())),
            ),
        ];
        for (prefix, suffix, expected) in cases {
            let (mut source, mut output) = pair(false);
            let count = TTY_BUFFER_SIZE - prefix.len();
            let mut bytes = vec![b'x'; count];
            bytes.extend_from_slice(prefix);
            output.write_all(&bytes).unwrap();
            drain_x(&mut source, count);
            assert_eq!(
                source.try_read(Some(Duration::from_millis(20))).unwrap(),
                None
            );
            assert_eq!(source.parser.buffer, prefix);
            output.write_all(suffix).unwrap();
            assert_eq!(
                source.try_read(Some(Duration::from_millis(20))).unwrap(),
                Some(expected)
            );
            assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
        }
    }

    #[cfg(feature = "event-stream")]
    #[test]
    fn escape_idle_completion_retains_real_wake_in_both_batch_orders() {
        for wake_first in [true, false] {
            let (mut source, mut output) = pair(false);
            let mut bytes = vec![b'x'; TTY_BUFFER_SIZE - 1];
            bytes.push(0x1b);
            output.write_all(&bytes).unwrap();
            source.waker().wake().unwrap();
            source
                .poll
                .poll(&mut source.events, Some(Duration::from_millis(20)))
                .unwrap();
            source
                .pending_tokens
                .extend(source.events.iter().map(|event| event.token()));
            assert!(source.pending_tokens.contains(&TTY_TOKEN));
            assert!(source.pending_tokens.contains(&WAKE_TOKEN));
            source
                .pending_tokens
                .make_contiguous()
                .sort_by_key(|token| {
                    if (*token == WAKE_TOKEN) == wake_first {
                        0
                    } else {
                        1
                    }
                });
            if wake_first {
                assert_eq!(
                    source.try_read(Some(Duration::ZERO)).unwrap_err().kind(),
                    io::ErrorKind::Interrupted
                );
            }
            drain_x(&mut source, TTY_BUFFER_SIZE - 1);
            assert_eq!(
                source.try_read(Some(Duration::ZERO)).unwrap(),
                Some(key(KeyCode::Esc))
            );
            if !wake_first {
                assert_eq!(
                    source.try_read(Some(Duration::ZERO)).unwrap_err().kind(),
                    io::ErrorKind::Interrupted
                );
            }
            assert_eq!(source.try_read(Some(Duration::ZERO)).unwrap(), None);
        }
    }
}
