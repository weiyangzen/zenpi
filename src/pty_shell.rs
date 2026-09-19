//! A real interactive shell bound to a working directory (ZS1-166).
//!
//! The Shell pane is not a projection: it owns a pseudo-terminal running the
//! user's login shell with the pane's working directory, forwards keystrokes,
//! and renders a bounded, ANSI-stripped scrollback. The implementation is
//! deliberately small and read/`write` only: it never interprets the terminal
//! for the child and never mangles the master fd across threads.
//!
//! `PtyShell` is cheaply cloneable (`Arc<Mutex<_>>`) because `TuiState` derives
//! `Clone`; every handle shares one child, and the child is reaped when the
//! last handle drops. On non-unix targets the type exists but
//! [`PtyShell::spawn`] always fails so callers fall back to the last snapshot.

use std::path::PathBuf;

#[cfg(unix)]
mod imp {
    use super::PathBuf;
    use std::{
        ffi::CString,
        fmt, io,
        os::{fd::RawFd, unix::ffi::OsStrExt},
        sync::{
            Arc, Mutex,
            mpsc::{self, Receiver, TryRecvError},
        },
        thread::JoinHandle,
    };

    /// Upper bound on retained raw output. Older bytes are dropped so a chatty
    /// shell cannot grow the TUI without bound.
    const MAX_OUTPUT_BYTES: usize = 200_000;

    pub struct PtyShell {
        inner: Arc<Mutex<Inner>>,
    }

    struct Inner {
        master: RawFd,
        child: libc::pid_t,
        cwd: PathBuf,
        output: Vec<u8>,
        display: String,
        /// Bytes accepted before the slave could read (early keystrokes, or a
        /// full kernel buffer). Flushed opportunistically, never blocking.
        pending: Vec<u8>,
        rx: Receiver<Vec<u8>>,
        reader: Option<JoinHandle<()>>,
    }

    /// Best-effort flush of buffered input. EAGAIN keeps the remainder queued.
    fn flush_pending(inner: &mut Inner) {
        while !inner.pending.is_empty() {
            // SAFETY: `master` is an open fd for the lifetime of `Inner`.
            let written = unsafe {
                libc::write(
                    inner.master,
                    inner.pending.as_ptr() as *const libc::c_void,
                    inner.pending.len(),
                )
            };
            if written > 0 {
                inner.pending.drain(..written as usize);
            } else {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => break,
                    _ => {
                        inner.pending.clear();
                        break;
                    }
                }
            }
        }
    }

    impl Clone for PtyShell {
        fn clone(&self) -> Self {
            Self {
                inner: Arc::clone(&self.inner),
            }
        }
    }

    impl fmt::Debug for PtyShell {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self.inner.lock() {
                Ok(inner) => formatter
                    .debug_struct("PtyShell")
                    .field("pid", &inner.child)
                    .field("cwd", &inner.cwd)
                    .finish(),
                Err(_) => formatter
                    .debug_struct("PtyShell")
                    .field("pid", &"<poisoned>")
                    .finish(),
            }
        }
    }

    impl PtyShell {
        /// Spawn `$SHELL` (or `/bin/sh`) in a fresh PTY rooted at `cwd`.
        pub fn spawn(cwd: PathBuf) -> io::Result<Self> {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
            let shell_c = CString::new(shell.clone())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid $SHELL"))?;
            let cwd_c = CString::new(cwd.as_os_str().as_bytes())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cwd"))?;
            let term_c = CString::new("TERM=xterm-256color").expect("static TERM");

            // SAFETY: `posix_openpt` returns an owned fd or -1.
            let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
            if master < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `master` is an open PTY master.
            if unsafe { libc::grantpt(master) } != 0 || unsafe { libc::unlockpt(master) } != 0 {
                let error = io::Error::last_os_error();
                unsafe { libc::close(master) };
                return Err(error);
            }
            // SAFETY: `master` is unlocked; `ptsname` returns a libc-owned buffer
            // that stays valid until the next `ptsname` call.
            let slave_name = unsafe { libc::ptsname(master) };
            if slave_name.is_null() {
                let error = io::Error::last_os_error();
                unsafe { libc::close(master) };
                return Err(error);
            }

            // Fork before allocating further state so the child only uses
            // async-signal-safe calls before `execvp`.
            // SAFETY: called in a normal multi-threaded process, not mid-signal.
            let pid = unsafe { libc::fork() };
            if pid < 0 {
                let error = io::Error::last_os_error();
                unsafe { libc::close(master) };
                return Err(error);
            }
            if pid == 0 {
                // Child: attach the controlling terminal and replace the image.
                unsafe {
                    libc::setsid();
                    let slave = libc::open(slave_name, libc::O_RDWR);
                    if slave < 0 {
                        libc::_exit(127);
                    }
                    libc::ioctl(slave, libc::TIOCSCTTY as libc::c_ulong, 0);
                    libc::dup2(slave, 0);
                    libc::dup2(slave, 1);
                    libc::dup2(slave, 2);
                    if slave > 2 {
                        libc::close(slave);
                    }
                    libc::close(master);
                    libc::chdir(cwd_c.as_ptr());
                    libc::putenv(term_c.into_raw());
                    let argument = shell_c.as_ptr() as *const libc::c_char;
                    let argv = [argument, std::ptr::null()];
                    libc::execvp(shell_c.as_ptr(), argv.as_ptr());
                    libc::_exit(127);
                }
            }

            // SAFETY: setting O_NONBLOCK on a master fd this process owns.
            unsafe {
                let flags = libc::fcntl(master, libc::F_GETFL);
                if flags >= 0 {
                    libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK);
                }
            }

            let (tx, rx) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                loop {
                    // SAFETY: `master` stays open until this thread is joined in
                    // `Inner::drop`, so it is never reused underneath us.
                    let read = unsafe {
                        libc::read(
                            master,
                            buffer.as_mut_ptr() as *mut libc::c_void,
                            buffer.len(),
                        )
                    };
                    if read > 0 {
                        if tx.send(buffer[..read as usize].to_vec()).is_err() {
                            break;
                        }
                    } else if read == 0 {
                        break;
                    } else {
                        let error = io::Error::last_os_error();
                        match error.kind() {
                            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => continue,
                            _ => break,
                        }
                    }
                }
            });

            Ok(Self {
                inner: Arc::new(Mutex::new(Inner {
                    master,
                    child: pid,
                    cwd,
                    output: Vec::new(),
                    display: String::new(),
                    pending: Vec::new(),
                    rx,
                    reader: Some(reader),
                })),
            })
        }

        pub fn cwd(&self) -> PathBuf {
            self.inner
                .lock()
                .map(|inner| inner.cwd.clone())
                .unwrap_or_default()
        }

        /// Drain pending output into the bounded scrollback. Returns whether
        /// anything changed so the caller can mark the frame dirty.
        pub fn pump(&mut self) -> bool {
            let Ok(mut inner) = self.inner.lock() else {
                return false;
            };
            let mut changed = false;
            loop {
                match inner.rx.try_recv() {
                    Ok(chunk) => {
                        inner.output.extend_from_slice(&chunk);
                        if inner.output.len() > MAX_OUTPUT_BYTES {
                            let excess = inner.output.len() - MAX_OUTPUT_BYTES;
                            inner.output.drain(..excess);
                        }
                        changed = true;
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
            flush_pending(&mut inner);
            if changed {
                inner.display = render(&inner.output);
            }
            changed
        }

        pub fn write_input(&mut self, bytes: &[u8]) {
            if bytes.is_empty() {
                return;
            }
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.pending.extend_from_slice(bytes);
            flush_pending(&mut inner);
        }

        pub fn resize(&mut self, rows: u16, cols: u16) {
            if rows == 0 || cols == 0 {
                return;
            }
            let Ok(inner) = self.inner.lock() else {
                return;
            };
            let size = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            // SAFETY: `master` is an open PTY master; `size` is a valid winsize.
            unsafe {
                libc::ioctl(inner.master, libc::TIOCSWINSZ as libc::c_ulong, &size);
            }
        }

        /// Bounded last `rows` lines of the stripped scrollback.
        pub fn content(&self, rows: usize) -> String {
            let Ok(inner) = self.inner.lock() else {
                return String::new();
            };
            if inner.display.is_empty() {
                return String::new();
            }
            let lines: Vec<&str> = inner.display.split('\n').collect();
            let start = lines.len().saturating_sub(rows.max(1));
            lines[start..].join("\n")
        }

        /// Display column after the last emitted character, so the IME anchor
        /// sits at the shell prompt rather than at another pane (ZS1-172).
        pub fn cursor_column(&self) -> usize {
            let Ok(inner) = self.inner.lock() else {
                return 0;
            };
            inner
                .display
                .split('\n')
                .next_back()
                .map(|line| line.chars().count())
                .unwrap_or(0)
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            // SAFETY: `child` is this shell. SIGHUP then SIGKILL guarantees the
            // reader's `read` returns before we close the master.
            unsafe {
                libc::kill(self.child, libc::SIGHUP);
                libc::kill(self.child, libc::SIGKILL);
                let mut status = 0;
                libc::waitpid(self.child, &mut status, 0);
            }
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
            // SAFETY: closing a master fd owned exactly once.
            unsafe {
                libc::close(self.master);
            }
        }
    }

    /// Strip terminal control sequences into displayable text. Pure and
    /// fixture-testable. Handles CSI/OSC/2-byte escapes and a minimal cursor
    /// model: `\r` returns to column 0 and overwrites in place (progress bars),
    /// `\n` starts a new line, and backspace deletes. Cursor-addressing escapes
    /// are dropped, so full-screen programs degrade to their printed text.
    pub fn render(bytes: &[u8]) -> String {
        let mut lines: Vec<Vec<char>> = vec![Vec::new()];
        let mut column = 0usize;
        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == 0x1b {
                index += 1;
                match bytes.get(index) {
                    Some(b'[') => {
                        index += 1;
                        while index < bytes.len() {
                            let c = bytes[index];
                            index += 1;
                            if (0x40..=0x7e).contains(&c) {
                                break;
                            }
                        }
                    }
                    Some(b']') => {
                        index += 1;
                        while index < bytes.len() {
                            let c = bytes[index];
                            if c == 0x07 {
                                index += 1;
                                break;
                            }
                            if c == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                                index += 2;
                                break;
                            }
                            index += 1;
                        }
                    }
                    Some(_) => index += 1,
                    None => break,
                }
                continue;
            }
            match byte {
                b'\n' => {
                    lines.push(Vec::new());
                    column = 0;
                    index += 1;
                }
                b'\r' => {
                    column = 0;
                    index += 1;
                }
                0x08 => {
                    if column > 0 {
                        column -= 1;
                        if let Some(line) = lines.last_mut() {
                            if column < line.len() {
                                line.remove(column);
                            }
                        }
                    }
                    index += 1;
                }
                b'\t' => {
                    for _ in 0..4 {
                        push_char(&mut lines, &mut column, ' ');
                    }
                    index += 1;
                }
                0x00..=0x1f | 0x7f => index += 1,
                _ => {
                    let start = index;
                    let width = utf8_width(byte);
                    index = (index + width).min(bytes.len());
                    match std::str::from_utf8(&bytes[start..index]) {
                        Ok(text) => {
                            for character in text.chars() {
                                push_char(&mut lines, &mut column, character);
                            }
                        }
                        Err(_) => push_char(&mut lines, &mut column, '\u{fffd}'),
                    }
                }
            }
        }
        lines
            .into_iter()
            .map(|line| line.into_iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Write one character at the current column, overwriting if the cursor has
    /// moved back over existing text.
    fn push_char(lines: &mut [Vec<char>], column: &mut usize, character: char) {
        let Some(line) = lines.last_mut() else {
            return;
        };
        if *column < line.len() {
            line[*column] = character;
        } else {
            line.push(character);
        }
        *column += 1;
    }

    fn utf8_width(byte: u8) -> usize {
        match byte {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => 1,
        }
    }

    /// Map a crossterm key to the bytes a terminal would send. Returns `None`
    /// for chords the TUI keeps for itself.
    pub fn key_bytes(
        code: &crossterm::event::KeyCode,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        if modifiers.contains(KeyModifiers::ALT) {
            return None;
        }
        let bytes = match code {
            KeyCode::Char(c) => {
                if ctrl {
                    // Only forward control chords a shell actually uses; the
                    // rest belong to the TUI (tabs, close pane, editor, ...).
                    let lower = c.to_ascii_lowercase();
                    match lower {
                        'c' | 'd' | 'z' | 'a' | 'e' | 'l' => vec![(lower as u8) - b'a' + 1],
                        _ => return None,
                    }
                } else {
                    c.to_string().into_bytes()
                }
            }
            KeyCode::Enter => vec![b'\r'],
            KeyCode::Backspace => vec![0x7f],
            KeyCode::Delete => b"\x1b[3~".to_vec(),
            KeyCode::Esc => vec![0x1b],
            KeyCode::Up => b"\x1b[A".to_vec(),
            KeyCode::Down => b"\x1b[B".to_vec(),
            KeyCode::Right => b"\x1b[C".to_vec(),
            KeyCode::Left => b"\x1b[D".to_vec(),
            KeyCode::Home => b"\x1b[H".to_vec(),
            KeyCode::End => b"\x1b[F".to_vec(),
            KeyCode::PageUp => b"\x1b[5~".to_vec(),
            KeyCode::PageDown => b"\x1b[6~".to_vec(),
            KeyCode::Insert => b"\x1b[2~".to_vec(),
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => return None,
        };
        Some(bytes)
    }
}

#[cfg(unix)]
pub use imp::{PtyShell, key_bytes, render};

#[cfg(not(unix))]
pub use fallback::{PtyShell, key_bytes, render};

#[cfg(not(unix))]
mod fallback {
    use std::io;
    use std::path::PathBuf;

    #[derive(Clone, Debug)]
    pub struct PtyShell;

    impl PtyShell {
        pub fn spawn(_cwd: PathBuf) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "interactive shell requires a unix pseudo-terminal",
            ))
        }

        pub fn cwd(&self) -> PathBuf {
            PathBuf::new()
        }

        pub fn pump(&mut self) -> bool {
            false
        }

        pub fn write_input(&mut self, _bytes: &[u8]) {}

        pub fn resize(&mut self, _rows: u16, _cols: u16) {}

        pub fn content(&self, _rows: usize) -> String {
            String::new()
        }

        pub fn cursor_column(&self) -> usize {
            0
        }
    }

    pub fn render(_bytes: &[u8]) -> String {
        String::new()
    }

    pub fn key_bytes(
        _code: &crossterm::event::KeyCode,
        _modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        None
    }
}

/// Resolve the shell working directory from an ordered candidate list.
pub fn resolve_cwd(candidates: impl IntoIterator<Item = PathBuf>) -> PathBuf {
    for candidate in candidates {
        if candidate.is_dir() {
            return candidate;
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_strips_csi_and_applies_carriage_returns() {
        let raw = b"\x1b[32mgreen\x1b[0m plain\nprogress 10%\rprogress 100%\n";
        assert_eq!(render(raw), "green plain\nprogress 100%\n");
    }

    #[test]
    fn render_handles_osc_and_backspace() {
        let raw = b"\x1b]0;title\x07abc\x08d\n";
        assert_eq!(render(raw), "abd\n");
    }

    #[test]
    fn resolve_cwd_prefers_the_first_existing_directory() {
        let dir = std::env::temp_dir();
        let missing = dir.join("zenpi-definitely-missing-dir");
        assert_eq!(resolve_cwd([missing.clone(), dir.clone()]), dir);
        assert_eq!(
            resolve_cwd([missing, PathBuf::from(".")]),
            PathBuf::from(".")
        );
    }

    #[cfg(unix)]
    #[test]
    fn spawn_runs_a_real_command_in_the_requested_cwd() {
        let dir = std::env::temp_dir();
        let mut shell = PtyShell::spawn(dir.clone()).expect("spawn shell");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        // Wait for the shell prompt before typing, exactly like a user.
        while std::time::Instant::now() < deadline && shell.content(200).trim().is_empty() {
            shell.pump();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        shell.write_input(b"pwd; echo ZENPI_PTY_OK\r");
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            shell.pump();
            text = shell.content(200);
            if text.contains("ZENPI_PTY_OK") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(text.contains("ZENPI_PTY_OK"), "no shell output: {text:?}");
        assert_eq!(shell.cwd(), dir);
    }
}
