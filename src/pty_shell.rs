//! A real interactive shell bound to a working directory (ZS1-166).
//!
//! The Shell pane is not a projection: it owns a pseudo-terminal running the
//! user's login shell with the pane's working directory, forwards keystrokes,
//! and paints a small terminal screen parsed from the PTY byte stream.
//!
//! The screen model is deliberately minimal but *correct* for interactive line
//! editing — notably, `\b` moves the cursor left (it does not erase), CSI
//! cursor movement and erase-to-end-of-line are honored, and East-Asian wide
//! characters occupy two cells. A naive "strip escapes" renderer corrupts CJK
//! editing; this one tracks a cell grid and the live cursor (ZS1-173).
//!
//! On non-unix targets the type exists but [`PtyShell::spawn`] always fails so
//! callers fall back to the last snapshot.

use std::path::PathBuf;

#[cfg(unix)]
pub use imp::{PtyShell, key_bytes};

#[cfg(not(unix))]
pub use fallback::{PtyShell, key_bytes};

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
    use unicode_width::UnicodeWidthChar;

    /// Upper bound on retained screen lines (scrollback) and columns.
    const MAX_LINES: usize = 4_000;
    const MAX_COLS: usize = 512;

    /// One screen cell. `width == 0` marks the second cell of a wide glyph.
    #[derive(Clone, Copy)]
    struct Cell {
        ch: char,
        width: u8,
    }

    impl Cell {
        const BLANK: Self = Self { ch: ' ', width: 1 };
    }

    /// A bounded character grid produced by a minimal VT parser.
    pub struct Screen {
        lines: Vec<Vec<Cell>>,
        row: usize,
        col: usize,
    }

    impl Screen {
        pub fn new() -> Self {
            Self {
                lines: vec![Vec::new()],
                row: 0,
                col: 0,
            }
        }

        fn ensure_row(&mut self) {
            while self.lines.len() <= self.row {
                self.lines.push(Vec::new());
            }
        }

        fn cell_mut(&mut self, row: usize, col: usize) -> &mut Cell {
            self.ensure_row();
            let line = &mut self.lines[row];
            while line.len() <= col {
                line.push(Cell::BLANK);
            }
            &mut line[col]
        }

        fn advance_line(&mut self) {
            self.row += 1;
            self.col = 0;
            self.ensure_row();
            while self.lines.len() > MAX_LINES {
                self.lines.remove(0);
                self.row = self.row.saturating_sub(1);
            }
        }

        fn put(&mut self, ch: char) {
            let width = UnicodeWidthChar::width(ch).unwrap_or(0) as u8;
            if width == 0 {
                // Combining mark / zero-width: attach is out of scope; drop it
                // rather than corrupt the grid.
                return;
            }
            let cells = usize::from(width.min(2));
            if self.col + cells > MAX_COLS {
                self.advance_line();
            }
            self.cell_mut(self.row, self.col).ch = ch;
            self.cell_mut(self.row, self.col).width = width;
            if cells == 2 {
                let continuation = self.cell_mut(self.row, self.col + 1);
                continuation.ch = ' ';
                continuation.width = 0;
            }
            self.col += cells;
        }

        fn carriage_return(&mut self) {
            self.col = 0;
        }

        /// Backspace moves the cursor exactly one cell left; it never erases.
        /// Wide glyphs are crossed by the shell emitting two backspaces, so
        /// this must not skip a continuation cell on its own (ZS1-173).
        fn backspace(&mut self) {
            self.col = self.col.saturating_sub(1);
        }

        fn tab(&mut self) {
            let next = (self.col / 8 + 1) * 8;
            self.col = next.min(MAX_COLS.saturating_sub(1));
        }

        fn erase_line(&mut self, mode: usize) {
            self.ensure_row();
            let col = self.col;
            let line = &mut self.lines[self.row];
            match mode {
                0 => {
                    if col < line.len() {
                        line.truncate(col);
                    }
                }
                1 => {
                    for cell in line.iter_mut().take(col.saturating_add(1)) {
                        *cell = Cell::BLANK;
                    }
                }
                _ => line.clear(),
            }
        }

        fn erase_display(&mut self, mode: usize) {
            match mode {
                0 => {
                    self.erase_line(0);
                    self.lines.truncate(self.row + 1);
                }
                1 => {
                    for row in 0..self.row {
                        self.lines[row].clear();
                    }
                    self.erase_line(1);
                }
                _ => {
                    self.lines.clear();
                    self.lines.push(Vec::new());
                    self.row = 0;
                    self.col = 0;
                }
            }
        }

        fn move_to(&mut self, row: usize, col: usize) {
            self.row = row.min(MAX_LINES.saturating_sub(1));
            self.col = col.min(MAX_COLS.saturating_sub(1));
            self.ensure_row();
        }

        fn csi(&mut self, params: &[u8], final_byte: u8) {
            if params.first() == Some(&b'?') || params.first() == Some(&b'>') {
                return;
            }
            let numbers: Vec<usize> = params
                .split(|byte| *byte == b';')
                .map(|part| {
                    std::str::from_utf8(part)
                        .ok()
                        .and_then(|text| text.parse().ok())
                        .unwrap_or(0)
                })
                .collect();
            let arg = |index: usize| -> usize {
                let value = numbers.get(index).copied().unwrap_or(0);
                if value == 0 { 1 } else { value }
            };
            match final_byte {
                b'A' => self.row = self.row.saturating_sub(arg(0)),
                b'B' => self.row = (self.row + arg(0)).min(MAX_LINES.saturating_sub(1)),
                b'C' => self.col = (self.col + arg(0)).min(MAX_COLS.saturating_sub(1)),
                b'D' => self.col = self.col.saturating_sub(arg(0)),
                b'E' => {
                    self.row = (self.row + arg(0)).min(MAX_LINES.saturating_sub(1));
                    self.col = 0;
                }
                b'F' => {
                    self.row = self.row.saturating_sub(arg(0));
                    self.col = 0;
                }
                b'G' | b'`' => self.col = (arg(0) - 1).min(MAX_COLS.saturating_sub(1)),
                b'd' => self.row = (arg(0) - 1).min(MAX_LINES.saturating_sub(1)),
                b'H' | b'f' => {
                    let row = numbers.first().copied().unwrap_or(0);
                    let col = numbers.get(1).copied().unwrap_or(0);
                    self.move_to(row.saturating_sub(1), col.saturating_sub(1));
                }
                b'J' => self.erase_display(numbers.first().copied().unwrap_or(0)),
                b'K' => self.erase_line(numbers.first().copied().unwrap_or(0)),
                _ => {}
            }
            self.ensure_row();
        }

        pub fn column(&self) -> usize {
            self.col
        }

        fn line_text(&self, row: usize) -> String {
            let mut text = String::new();
            if let Some(line) = self.lines.get(row) {
                for cell in line {
                    if cell.width == 0 {
                        continue;
                    }
                    text.push(cell.ch);
                }
            }
            text.trim_end().to_owned()
        }

        pub fn content(&self, rows: usize) -> String {
            let total = self.lines.len();
            let start = total.saturating_sub(rows.max(1));
            (start..total)
                .map(|row| self.line_text(row))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    fn utf8_len(byte: u8) -> usize {
        match byte {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => 1,
        }
    }

    /// Parse one escape sequence starting at `bytes[0] == ESC`. Returns the
    /// number of bytes consumed, or `None` when the sequence is incomplete.
    fn parse_escape(screen: &mut Screen, bytes: &[u8]) -> Option<usize> {
        let next = *bytes.get(1)?;
        match next {
            b'[' => {
                let mut index = 2;
                while index < bytes.len() && !(0x40..=0x7e).contains(&bytes[index]) {
                    index += 1;
                }
                if index >= bytes.len() {
                    return None;
                }
                screen.csi(&bytes[2..index], bytes[index]);
                Some(index + 1)
            }
            b']' => {
                let mut index = 2;
                while index < bytes.len() {
                    if bytes[index] == 0x07 {
                        return Some(index + 1);
                    }
                    if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
                        return Some(index + 2);
                    }
                    index += 1;
                }
                None
            }
            _ => Some(2),
        }
    }

    /// Consume every complete sequence from `pending`, leaving a trailing
    /// partial UTF-8 or escape sequence for the next chunk.
    pub fn feed(screen: &mut Screen, pending: &mut Vec<u8>) {
        let mut index = 0;
        while index < pending.len() {
            let byte = pending[index];
            match byte {
                0x1b => match parse_escape(screen, &pending[index..]) {
                    Some(used) => index += used,
                    None => break,
                },
                b'\n' => {
                    screen.advance_line();
                    index += 1;
                }
                b'\r' => {
                    screen.carriage_return();
                    index += 1;
                }
                0x08 => {
                    screen.backspace();
                    index += 1;
                }
                b'\t' => {
                    screen.tab();
                    index += 1;
                }
                0x00..=0x1f | 0x7f => index += 1,
                _ => {
                    let width = utf8_len(byte);
                    if index + width > pending.len() {
                        break;
                    }
                    match std::str::from_utf8(&pending[index..index + width]) {
                        Ok(text) => {
                            for character in text.chars() {
                                screen.put(character);
                            }
                            index += width;
                        }
                        Err(_) => index += 1,
                    }
                }
            }
        }
        pending.drain(..index);
    }

    pub struct PtyShell {
        inner: Arc<Mutex<Inner>>,
    }

    struct Inner {
        master: RawFd,
        child: libc::pid_t,
        cwd: PathBuf,
        screen: Screen,
        pending: Vec<u8>,
        /// Bytes accepted before the slave could read (early keystrokes, or a
        /// full kernel buffer). Flushed opportunistically, never blocking.
        input: Vec<u8>,
        rx: Receiver<Vec<u8>>,
        reader: Option<JoinHandle<()>>,
    }

    /// Best-effort flush of buffered input. EAGAIN keeps the remainder queued.
    fn flush_input(inner: &mut Inner) {
        while !inner.input.is_empty() {
            // SAFETY: `master` is an open fd for the lifetime of `Inner`.
            let written = unsafe {
                libc::write(
                    inner.master,
                    inner.input.as_ptr() as *const libc::c_void,
                    inner.input.len(),
                )
            };
            if written > 0 {
                inner.input.drain(..written as usize);
            } else {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => break,
                    _ => {
                        inner.input.clear();
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
                    screen: Screen::new(),
                    pending: Vec::new(),
                    input: Vec::new(),
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

        /// Drain pending output into the screen. Returns whether anything
        /// changed so the caller can mark the frame dirty.
        pub fn pump(&mut self) -> bool {
            let Ok(mut inner) = self.inner.lock() else {
                return false;
            };
            let mut changed = false;
            loop {
                match inner.rx.try_recv() {
                    Ok(chunk) => {
                        // Split the borrow so the parser can touch both fields.
                        let Inner {
                            screen, pending, ..
                        } = &mut *inner;
                        pending.extend_from_slice(&chunk);
                        feed(screen, pending);
                        changed = true;
                    }
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                }
            }
            flush_input(&mut inner);
            changed
        }

        pub fn write_input(&mut self, bytes: &[u8]) {
            if bytes.is_empty() {
                return;
            }
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };
            inner.input.extend_from_slice(bytes);
            flush_input(&mut inner);
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

        /// Bounded last `rows` lines of the screen.
        pub fn content(&self, rows: usize) -> String {
            let Ok(inner) = self.inner.lock() else {
                return String::new();
            };
            inner.screen.content(rows)
        }

        /// Live cursor as `(column, row)` in display cells.
        pub fn cursor(&self) -> (usize, usize) {
            let Ok(inner) = self.inner.lock() else {
                return (0, 0);
            };
            (inner.screen.col, inner.screen.row)
        }

        /// Number of screen lines (scrollback included).
        pub fn line_count(&self) -> usize {
            let Ok(inner) = self.inner.lock() else {
                return 1;
            };
            inner.screen.lines.len().max(1)
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

    /// Parse a full byte stream into plain text (screen contents). Used by the
    /// fixtures; the live pane keeps its `Screen` incrementally.
    pub fn render(bytes: &[u8]) -> String {
        let mut screen = Screen::new();
        let mut pending = bytes.to_vec();
        feed(&mut screen, &mut pending);
        screen.content(screen.lines.len())
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

        pub fn cursor(&self) -> (usize, usize) {
            (0, 0)
        }

        pub fn line_count(&self) -> usize {
            1
        }
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

    #[cfg(unix)]
    fn render(bytes: &[u8]) -> String {
        imp::render(bytes)
    }

    #[cfg(unix)]
    #[test]
    fn backspace_moves_the_cursor_and_never_erases() {
        // Backspace only moves; the glyphs stay until overwritten.
        assert_eq!(render(b"abc\x08\x08"), "abc");
        // Writing after moving left overwrites in place.
        assert_eq!(render(b"ab\x08X"), "aX");
    }

    #[cfg(unix)]
    #[test]
    fn wide_characters_use_two_cells_and_cursor_tracks_them() {
        let mut screen = imp::Screen::new();
        let mut pending = "你好".to_string().into_bytes();
        pending.push(0x08);
        imp::feed(&mut screen, &mut pending);
        assert_eq!(screen.content(10), "你好");
        // Backspace moves exactly one cell (into the second glyph).
        assert_eq!(screen.column(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn zle_wide_erase_sequence_removes_one_glyph() {
        // zsh zle erases a wide glyph with two single-cell backspaces, two
        // spaces, then two more backspaces — exactly as observed on the wire.
        let mut bytes = "echo 你好世界".as_bytes().to_vec();
        bytes.extend_from_slice(b"\x08\x08  \x08\x08");
        assert_eq!(render(&bytes), "echo 你好世");
    }

    #[cfg(unix)]
    #[test]
    fn carriage_return_overwrites_and_csi_erase_line_clears() {
        assert_eq!(render(b"progress 10%\rprogress 100%"), "progress 100%");
        assert_eq!(render(b"stale\r\x1b[Kfresh"), "fresh");
        assert_eq!(render(b"keep\x1b[K"), "keep");
        assert_eq!(render(b"abc\x1b[2K\rxyz"), "xyz");
    }

    #[cfg(unix)]
    #[test]
    fn osc_title_is_ignored() {
        assert_eq!(render(b"\x1b]0;title\x07hello"), "hello");
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
