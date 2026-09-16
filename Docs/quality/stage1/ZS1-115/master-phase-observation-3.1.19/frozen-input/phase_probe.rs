use std::{fs, path::{Path, PathBuf}, process::{Command, Stdio}, time::{Duration, Instant}};
extern crate libc;
    fn fixture_phase(path: &Path, name: &str) {
        use std::io::Write;
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: clock_gettime initializes the supplied valid timespec.
        assert_eq!(
            unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) },
            0
        );
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}.{:09} pid={} {}",
            now.tv_sec,
            now.tv_nsec,
            std::process::id(),
            name
        )
        .unwrap();
    }

    // Drop before the TempDir on return or unwind. The child may never get to
    // its own println after the outer watchdog, so the parent preserves evidence.
    struct FixturePhaseLog(PathBuf);
    impl Drop for FixturePhaseLog {
        fn drop(&mut self) {
            use std::io::Read;
            use std::os::unix::fs::OpenOptionsExt;
            const MAX_PHASE_BYTES: usize = 8 * 1024;
            let snapshot = (|| -> std::io::Result<_> {
                let file = fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(self.0.join("phases.log"))?;
                if !file.metadata()?.is_file() {
                    return Err(std::io::Error::other("phase log is not a regular file"));
                }
                let mut bytes = Vec::new();
                file.take((MAX_PHASE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
                let truncated = bytes.len() > MAX_PHASE_BYTES;
                bytes.truncate(MAX_PHASE_BYTES);
                Ok((bytes, truncated))
            })();
            match snapshot {
                Ok((bytes, truncated)) => println!(
                    "fixture_phase_snapshot dir={:?} bytes={} truncated={truncated} phases={}",
                    self.0,
                    bytes.len(),
                    String::from_utf8_lossy(&bytes)
                ),
                Err(error) => println!(
                    "fixture_phase_snapshot dir={:?} unavailable={error}",
                    self.0
                ),
            }
        }
    }

    struct FixturePids(PathBuf);
    impl Drop for FixturePids {
        fn drop(&mut self) {
            // Only IDs written by this test's own children in its private directory.
            for name in ["escaped.pid"] {
                if let Ok(text) = fs::read_to_string(self.0.join(name))
                    && let Ok(pid) = text.parse::<i32>()
                    && pid > 1
                {
                    // SAFETY: these are the fixture's recorded process IDs; no broad process scan.
                    unsafe { libc::kill(pid, libc::SIGKILL) };
                }
            }
        }
    }

fn observe(ext: &Path, mode: &str, preserve: bool) -> bool {
    let _phase_log = preserve.then(|| FixturePhaseLog(ext.to_owned()));
    let api = 0;
    let pipe = "diagnostic";
    let stop = mode;
    let cleanup = FixturePids(ext.to_owned());
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .arg("--child").arg(ext).arg(mode)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let start = Instant::now();
        let timed_out = loop {
            if helper.try_wait().unwrap().is_some() {
                break false;
            }
            if start.elapsed() >= Duration::from_secs(2) {
                break true;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let pids = ["parent.pid", "escaped.pid"]
            .map(|p| fs::read_to_string(ext.join(p)).unwrap_or_default());
        // First release only fixture descendants. A broken host's blocked worker
        // can then unwind; kill only this helper if it still cannot finish.
        drop(cleanup);
        if timed_out {
            let grace = Instant::now();
            while helper.try_wait().unwrap().is_none()
                && grace.elapsed() < Duration::from_millis(500)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            if helper.try_wait().unwrap().is_none() {
                helper.kill().unwrap();
            }
        }
        let output = helper.wait_with_output().unwrap();
        println!(
            "api={api} pipe={pipe} stop={stop} outer_timeout={timed_out} elapsed_ms={} fixture_pids={pids:?} status={} stdout={} stderr={}",
            start.elapsed().as_millis(),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        !timed_out && output.status.success()
    }

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let ext = PathBuf::from(&args[2]);
    if args[1] == "--child" {
        fixture_phase(&ext.join("phases.log"), "helper-entry");
        if args[3] == "watchdog" {
            fixture_phase(&ext.join("phases.log"), "intentional-watchdog-stall");
            std::thread::sleep(Duration::from_secs(10));
        }
        std::process::exit(if args[3] == "nonzero" { 17 } else { 0 });
    }
    if args[1] == "--snapshot" {
        let _phase_log = FixturePhaseLog(ext);
        return;
    }
    if args[1] == "--unwind" {
        let outcome = std::panic::catch_unwind(|| {
            let _phase_log = FixturePhaseLog(ext.clone());
            fixture_phase(&ext.join("phases.log"), "before-intentional-unwind");
            panic!("intentional fixture failure stays a failure");
        });
        fs::remove_dir_all(ext).unwrap();
        std::process::exit(if outcome.is_err() { 1 } else { 0 });
    }
    let success = observe(&ext, &args[3], args[1] == "--after");
    println!("observed_fixture_success={success}");
    println!("phase_file_before_cleanup_bytes={}", fs::metadata(ext.join("phases.log")).map(|m| m.len()).unwrap_or(0));
    fs::remove_dir_all(ext).unwrap();
    std::process::exit(if success { 0 } else { 1 });
}
