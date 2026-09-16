//! One explicitly requested Unix foreground editor and its private temporary file.
//! This owner never routes prompts, reads provider credentials, or captures stdio.
use crate::config::EditorCommand;
use std::{
    ffi::{CStr, CString, OsStr},
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, RawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const MAX_BYTES: usize = 256 * 1024;
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn cstring(value: &OsStr) -> io::Result<CString> {
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in editor path"))
}
fn open_at(directory: RawFd, name: &CStr, flags: i32, mode: libc::mode_t) -> io::Result<File> {
    // The borrowed C string is terminated; a successful descriptor becomes owned once.
    let fd = unsafe {
        libc::openat(
            directory,
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}

struct PrivateDirectory {
    parent: File,
    directory: File,
    name: CString,
    path: PathBuf,
    cleanup_attempted: bool,
}
impl PrivateDirectory {
    fn create(base: &Path, seed: &str) -> io::Result<Self> {
        if seed.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Editor seed exceeds 256 KiB",
            ));
        }
        let base = base.canonicalize()?;
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&base)?;
        for _ in 0..16 {
            let tick = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let name = format!(
                "zenpi-editor-{}-{tick:x}-{:x}",
                std::process::id(),
                NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
            );
            let encoded = cstring(OsStr::new(&name))?;
            if unsafe { libc::mkdirat(parent.as_raw_fd(), encoded.as_ptr(), 0o700) } != 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error);
            }
            let directory = match open_at(
                parent.as_raw_fd(),
                &encoded,
                libc::O_RDONLY | libc::O_DIRECTORY,
                0,
            ) {
                Ok(value) => value,
                Err(error) => {
                    if unsafe {
                        libc::unlinkat(parent.as_raw_fd(), encoded.as_ptr(), libc::AT_REMOVEDIR)
                    } != 0
                    {
                        return Err(io::Error::other(format!(
                            "{error}; editor cleanup failed; private files remain at {}: {}",
                            base.join(&name).display(),
                            io::Error::last_os_error()
                        )));
                    }
                    return Err(error);
                }
            };
            let mut owner = Self {
                parent,
                directory,
                name: encoded,
                path: base.join(name),
                cleanup_attempted: false,
            };
            let write_result = (|| -> io::Result<()> {
                let mut file = open_at(
                    owner.directory.as_raw_fd(),
                    c"draft.md",
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                    0o600,
                )?;
                file.write_all(seed.as_bytes())?;
                file.flush()
            })();
            if let Err(error) = write_result {
                return Err(io::Error::other(owner.cleanup_error(error.to_string())));
            }
            return Ok(owner);
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Editor private directory creation exhausted 16 attempts",
        ))
    }
    fn file_path(&self) -> PathBuf {
        self.path.join("draft.md")
    }
    fn read(&self) -> io::Result<String> {
        let directory = self.directory.metadata()?;
        if directory.uid() != unsafe { libc::geteuid() } || directory.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Editor directory is no longer private",
            ));
        }
        let file = open_at(
            self.directory.as_raw_fd(),
            c"draft.md",
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )?;
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
            || metadata.mode() & 0o400 == 0
            || metadata.nlink() != 1
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Editor result must be a private current-user regular file with one link",
            ));
        }
        if metadata.len() > MAX_BYTES as u64 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Editor result exceeds 256 KiB",
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Editor result exceeds 256 KiB",
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Editor result is not UTF-8"))
    }
    fn oversized(&self) -> bool {
        open_at(
            self.directory.as_raw_fd(),
            c"draft.md",
            libc::O_RDONLY | libc::O_NONBLOCK,
            0,
        )
        .and_then(|file| file.metadata())
        .is_ok_and(|m| m.is_file() && m.len() > MAX_BYTES as u64)
    }
    fn cleanup_error(&mut self, cause: String) -> String {
        match self.cleanup() {
            Ok(()) => cause,
            Err(error) => format!(
                "{cause}; editor cleanup failed; private files remain at {}: {error}",
                self.path.display()
            ),
        }
    }
    fn cleanup(&mut self) -> io::Result<()> {
        self.cleanup_attempted = true;
        clean_directory(self.directory.as_raw_fd(), 0, &mut 256)?;
        // Do not remove a replacement entry at the same parent/name.
        let current = open_at(
            self.parent.as_raw_fd(),
            &self.name,
            libc::O_RDONLY | libc::O_DIRECTORY,
            0,
        )?;
        let expected = self.directory.metadata()?;
        let actual = current.metadata()?;
        if expected.ino() != actual.ino() || expected.dev() != actual.dev() {
            return Err(io::Error::other(
                "Editor private directory identity changed",
            ));
        }
        if unsafe {
            libc::unlinkat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
impl Drop for PrivateDirectory {
    fn drop(&mut self) {
        if !self.cleanup_attempted {
            let _ = self.cleanup();
        }
    }
}

fn clean_directory(fd: RawFd, depth: usize, remaining: &mut usize) -> io::Result<()> {
    if depth > 4 {
        return Err(io::Error::other("Editor cleanup exceeds depth 4"));
    }
    // fdopendir owns only this duplicate; traversal and unlink remain dirfd-relative.
    let duplicate = unsafe { libc::dup(fd) };
    if duplicate < 0 {
        return Err(io::Error::last_os_error());
    }
    let stream = unsafe { libc::fdopendir(duplicate) };
    if stream.is_null() {
        unsafe {
            libc::close(duplicate);
        }
        return Err(io::Error::last_os_error());
    }
    struct DirectoryStream(*mut libc::DIR);
    impl Drop for DirectoryStream {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let stream = DirectoryStream(stream);
    loop {
        let entry = unsafe { libc::readdir(stream.0) };
        if entry.is_null() {
            break;
        }
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if *remaining == 0 {
            return Err(io::Error::other("Editor cleanup exceeds 256 entries"));
        }
        *remaining -= 1;
        match open_at(
            fd,
            name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NONBLOCK,
            0,
        ) {
            Ok(child) => {
                clean_directory(child.as_raw_fd(), depth + 1, remaining)?;
                if unsafe { libc::unlinkat(fd, name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Err(error) if matches!(error.raw_os_error(), Some(libc::ENOTDIR | libc::ELOOP)) => {
                if unsafe { libc::unlinkat(fd, name.as_ptr(), 0) } != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) struct PreparedEditor {
    directory: PrivateDirectory,
    command: EditorCommand,
    program: PathBuf,
    cwd: PathBuf,
}
impl PreparedEditor {
    pub(crate) fn reject(mut self, reason: String) -> String {
        self.directory.cleanup_error(reason)
    }
    pub(crate) fn new(seed: &str, command: &EditorCommand, cwd: &Path) -> Result<Self, String> {
        let cwd = cwd
            .canonicalize()
            .map_err(|_| "Editor origin folder is unavailable")?;
        let program = resolve_program(command, &cwd)?;
        let base = command
            .environment
            .get(OsStr::new("TMPDIR"))
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let directory = PrivateDirectory::create(&base, seed)
            .map_err(|error| format!("Editor temporary file failed: {error}"))?;
        Ok(Self {
            directory,
            command: command.clone(),
            program,
            cwd,
        })
    }
}
fn resolve_program(command: &EditorCommand, cwd: &Path) -> Result<PathBuf, String> {
    let name = Path::new(
        command
            .argv
            .first()
            .filter(|s| !s.is_empty())
            .ok_or("Editor command has no program")?,
    );
    let executable = |path: &Path| {
        path.is_file()
            && cstring(path.as_os_str())
                .is_ok_and(|p| unsafe { libc::access(p.as_ptr(), libc::X_OK) } == 0)
    };
    if name.is_absolute() || name.components().count() > 1 {
        let path = if name.is_absolute() {
            name.to_owned()
        } else {
            cwd.join(name)
        };
        return executable(&path)
            .then_some(path)
            .ok_or_else(|| "Editor program is missing or not executable".into());
    }
    if let Some(path) = command.environment.get(OsStr::new("PATH")) {
        for directory in std::env::split_paths(path) {
            let path = if directory.is_absolute() {
                directory
            } else {
                cwd.join(directory)
            }
            .join(name);
            if executable(&path) {
                return Ok(path);
            }
        }
    }
    Err("Editor program was not found in the captured PATH".into())
}

pub(crate) fn set_foreground(fd: RawFd, group: libc::pid_t) -> io::Result<()> {
    let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
    let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut blocked);
        libc::sigaddset(&mut blocked, libc::SIGTTOU);
    }
    let code = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous) };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code));
    }
    let result = if unsafe { libc::tcsetpgrp(fd, group) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    };
    let restore =
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };
    result.and(if restore == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(restore))
    })
}

/// Original controlling-terminal state, captured before crossterm enters raw mode.
pub(crate) struct TerminalState {
    attributes: libc::termios,
    flags: libc::c_int,
    foreground: libc::pid_t,
}
impl TerminalState {
    pub(crate) fn capture() -> io::Result<Self> {
        let mut attributes = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut attributes) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let flags = unsafe { libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL) };
        let foreground = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
        if flags < 0 || foreground < 0 {
            return Err(io::Error::last_os_error());
        }
        if foreground != unsafe { libc::getpgrp() } {
            return Err(io::Error::other(
                "Zenpi does not own the foreground terminal",
            ));
        }
        Ok(Self {
            attributes,
            flags,
            foreground,
        })
    }
    pub(crate) fn reclaim(&self) -> io::Result<()> {
        set_foreground(libc::STDIN_FILENO, self.foreground)
    }
    pub(crate) fn flush(&self) -> io::Result<()> {
        if unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    pub(crate) fn restore(&self, editor: bool) -> io::Result<()> {
        let attributes =
            if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.attributes) } == 0
            {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            };
        let flags = if unsafe {
            libc::fcntl(
                libc::STDIN_FILENO,
                libc::F_SETFL,
                if editor {
                    self.flags & !libc::O_NONBLOCK
                } else {
                    self.flags
                },
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        };
        attributes.and(flags)
    }
}

pub(crate) struct EditorSession {
    prepared: PreparedEditor,
    child: Child,
    group: libc::pid_t,
    previous_foreground: libc::pid_t,
    deadline: Instant,
    exit: Option<ExitStatus>,
    closing: Option<Instant>,
    failure: Option<String>,
    killed: bool,
    done: bool,
    fatal_failure: bool,
}
impl EditorSession {
    pub(crate) fn fatal_failure(&self) -> bool {
        self.fatal_failure
    }
    pub(crate) fn start(mut prepared: PreparedEditor) -> Result<Self, String> {
        let previous = unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) };
        if previous < 0 || previous != unsafe { libc::getpgrp() } {
            return Err(prepared
                .directory
                .cleanup_error("Zenpi does not own the foreground terminal".into()));
        }
        let child = Command::new(&prepared.program)
            .args(&prepared.command.argv[1..])
            .arg(prepared.directory.file_path())
            .current_dir(&prepared.cwd)
            .env_clear()
            .envs(&prepared.command.environment)
            .process_group(0)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn();
        let child = match child {
            Ok(child) => child,
            Err(error) => {
                return Err(prepared
                    .directory
                    .cleanup_error(format!("Editor could not start: {error}")));
            }
        };
        let group = child.id() as libc::pid_t;
        let deadline = Instant::now() + prepared.command.timeout;
        let mut session = Self {
            prepared,
            child,
            group,
            previous_foreground: previous,
            deadline,
            exit: None,
            closing: None,
            failure: None,
            killed: false,
            done: false,
            fatal_failure: false,
        };
        if let Err(error) = set_foreground(libc::STDIN_FILENO, group) {
            session.cancel(&format!("Editor foreground handoff failed: {error}"));
            return Err(session
                .shutdown()
                .err()
                .unwrap_or_else(|| "Editor foreground handoff failed".into()));
        }
        // A fast editor may have reached read() before the foreground transfer.
        unsafe {
            libc::kill(-group, libc::SIGCONT);
        }
        Ok(session)
    }
    pub(crate) fn cancel(&mut self, reason: &str) {
        self.failure.get_or_insert_with(|| reason.to_owned());
        self.begin_close();
    }
    fn begin_close(&mut self) {
        if self.closing.is_none() {
            self.closing = Some(Instant::now());
            unsafe {
                libc::kill(-self.group, libc::SIGCONT);
                libc::kill(-self.group, libc::SIGTERM);
            }
        }
    }
    fn group_exists(&self) -> bool {
        (unsafe { libc::kill(-self.group, 0) }) == 0
            || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    fn observe_child(&mut self) -> io::Result<()> {
        if self.exit.is_some() {
            return Ok(());
        }
        if let Some(exit) = self.child.try_wait()? {
            self.exit = Some(exit);
            return Ok(());
        }
        let mut status = 0;
        let pid =
            unsafe { libc::waitpid(self.group, &mut status, libc::WNOHANG | libc::WUNTRACED) };
        if pid < 0 {
            return Err(io::Error::last_os_error());
        }
        if pid > 0 {
            if libc::WIFSTOPPED(status) {
                self.cancel("Editor stopped; external changes not applied");
            } else if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                self.exit = Some(ExitStatus::from_raw(status));
            }
        }
        Ok(())
    }
    pub(crate) fn poll(&mut self) -> Option<Result<String, String>> {
        if self.done {
            return None;
        }
        if let Err(error) = self.observe_child() {
            self.cancel(&format!("Editor child status failed: {error}"));
        }
        if Instant::now() >= self.deadline {
            self.cancel("Editor deadline exceeded; external changes not applied");
        }
        if self.prepared.directory.oversized() {
            self.cancel("Editor result exceeds 256 KiB; external changes not applied");
        }
        if let Some(exit) = self.exit {
            if !exit.success() {
                self.failure.get_or_insert_with(|| {
                    format!("Editor exited with {exit}; external changes not applied")
                });
            }
            if self.group_exists() {
                self.begin_close();
            }
        }
        if let Some(started) = self.closing {
            if !self.killed && started.elapsed() >= Duration::from_millis(250) {
                unsafe {
                    libc::kill(-self.group, libc::SIGKILL);
                }
                self.killed = true;
            }
            if started.elapsed() >= Duration::from_millis(2250)
                && (self.exit.is_none() || self.group_exists())
            {
                self.fatal_failure = true;
                self.failure =
                    Some("Editor process group did not finish within the cleanup deadline".into());
                return Some(self.finish());
            }
        }
        if self.exit.is_some() && !self.group_exists() {
            return Some(self.finish());
        }
        None
    }
    fn finish(&mut self) -> Result<String, String> {
        self.done = true;
        let mut result = set_foreground(libc::STDIN_FILENO, self.previous_foreground)
            .map_err(|error| format!("Editor foreground restore failed: {error}"))
            .and_then(|()| match self.failure.take() {
                Some(error) => Err(error),
                None => self
                    .prepared
                    .directory
                    .read()
                    .map_err(|error| format!("Editor result rejected: {error}")),
            });
        if let Err(error) = self.prepared.directory.cleanup() {
            result = Err(format!(
                "Editor cleanup failed; private files remain at {}: {error}",
                self.prepared.directory.path.display()
            ));
        }
        result
    }
    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        if self.done {
            return Ok(());
        }
        self.cancel("Editor cancelled for terminal shutdown");
        loop {
            if let Some(result) = self.poll() {
                return result.map(|_| ());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for EditorSession {
    fn drop(&mut self) {
        if !self.done {
            let _ = self.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn private_file_preserves_unicode_and_allows_private_atomic_replacement() {
        let base = tempfile::tempdir().unwrap();
        let seed = "head\n界e\u{301}👨‍👩‍👧‍👦  \n";
        let mut owner = PrivateDirectory::create(base.path(), seed).unwrap();
        assert_eq!(owner.directory.metadata().unwrap().mode() & 0o777, 0o700);
        assert_eq!(
            std::fs::metadata(owner.file_path()).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(owner.read().unwrap(), seed);
        let replacement = owner.path.join("atomic.md");
        std::fs::write(&replacement, "changed  \n").unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::rename(replacement, owner.file_path()).unwrap();
        assert_eq!(owner.read().unwrap(), "changed  \n");
        owner.cleanup().unwrap();
        assert!(!owner.path.exists());
    }

    #[test]
    fn bounded_read_rejects_large_invalid_or_non_private_results() {
        let base = tempfile::tempdir().unwrap();
        let mut owner = PrivateDirectory::create(base.path(), "seed").unwrap();
        std::fs::write(owner.file_path(), vec![b'x'; MAX_BYTES]).unwrap();
        assert_eq!(owner.read().unwrap().len(), MAX_BYTES);
        std::fs::write(owner.file_path(), vec![b'x'; MAX_BYTES + 1]).unwrap();
        assert!(owner.oversized());
        assert!(owner.read().is_err());
        std::fs::write(owner.file_path(), [0xff]).unwrap();
        assert!(owner.read().is_err());
        std::fs::write(owner.file_path(), "private").unwrap();
        std::fs::set_permissions(owner.file_path(), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(owner.read().is_err());
        std::fs::set_permissions(owner.file_path(), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        std::fs::hard_link(owner.file_path(), owner.path.join("other-link")).unwrap();
        assert!(owner.read().is_err());
        owner.cleanup().unwrap();
    }

    #[test]
    fn symlink_fifo_directory_and_missing_result_never_block_or_follow() {
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        std::fs::write(&outside, "unchanged").unwrap();
        for kind in 0..4 {
            let mut owner = PrivateDirectory::create(base.path(), "seed").unwrap();
            let file = owner.file_path();
            std::fs::remove_file(&file).unwrap();
            match kind {
                0 => symlink(&outside, &file).unwrap(),
                1 => assert_eq!(
                    unsafe { libc::mkfifo(cstring(file.as_os_str()).unwrap().as_ptr(), 0o600) },
                    0
                ),
                2 => std::fs::create_dir(&file).unwrap(),
                _ => {}
            }
            let before = Instant::now();
            assert!(owner.read().is_err());
            assert!(before.elapsed() < Duration::from_millis(500));
            owner.cleanup().unwrap();
            assert_eq!(std::fs::read_to_string(&outside).unwrap(), "unchanged");
        }
    }

    #[test]
    fn cleanup_is_dirfd_relative_and_reports_entry_and_depth_limits() {
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep"), "private outside").unwrap();
        let mut owner = PrivateDirectory::create(base.path(), "seed").unwrap();
        symlink(&outside, owner.path.join("link")).unwrap();
        owner.cleanup().unwrap();
        assert!(outside.join("keep").exists());
        let mut owner = PrivateDirectory::create(base.path(), "seed").unwrap();
        for i in 0..257 {
            std::fs::write(owner.path.join(i.to_string()), "").unwrap();
        }
        assert!(owner.cleanup().unwrap_err().to_string().contains("256"));
        assert!(owner.path.exists());
        let mut owner = PrivateDirectory::create(base.path(), "seed").unwrap();
        std::fs::create_dir_all(owner.path.join("1/2/3/4/5")).unwrap();
        assert!(owner.cleanup().unwrap_err().to_string().contains("depth"));
        assert!(owner.path.exists());
    }
}
