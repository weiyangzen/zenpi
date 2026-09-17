#!/usr/bin/env python3
"""Rebuild and verify original/patched crossterm with real Unix PTYs.

Owned by ZS1-131. The candidate is ``vendor/crossterm``, pinned to the upstream
crossterm 0.29.0 ``.crate`` whose SHA256 is ``CRATE_SHA256`` below. The helper
extracts the archived 77 files, writes the pristine baseline, and compares every
relative path, byte and SHA256 against the candidate. The only permitted
differences are the two files in ``ALLOWED_CHANGED``:

* ``src/event.rs`` -- ``reset_event_reader`` (Unix, without ``event-stream``)
* ``src/event/source/unix/mio.rs`` -- readiness-token consumption fix

``.cargo-ok`` is a Cargo install marker and must not enter the vendor tree. The
emitted ``inventory.json`` records the crate SHA, the baseline and candidate
file inventories (relative path -> SHA256) and the changed list; ``result.json``
records the command bindings and the real-PTY evidence.

Rollback: remove the ``[patch.crates-io] crossterm`` entry from the root
``Cargo.toml``, delete ``vendor/crossterm`` and this helper, and regenerate
``Cargo.lock``. Callers added for item ZS1-130 are reverted together with it.

Requires cargo, a pristine crossterm 0.29.0 source directory, its official .crate,
and the candidate vendor directory. Only the new evidence directory is written.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import subprocess
import time
import tarfile
import urllib.request

CRATE_SHA256 = 'd8b9f2e4c67f833b660cdb0a3523065869fb35570177239812ed4c905aeff87b'
ALLOWED_CHANGED = {'src/event.rs', 'src/event/source/unix/mio.rs'}
# Embedded programs are independently rebuilt; no archived executable is trusted.
BEFORE_SOURCE = r'''
use crossterm::{event, terminal};
use std::{io::{self, Write}, time::Duration};
fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    println!("READY"); io::stdout().flush()?;
    if !event::poll(Duration::from_secs(3))? { panic!("initial input deadline"); }
    println!("FIRST={:?}", event::read()?);
    let mut drained = Vec::new();
    while event::poll(Duration::ZERO)? { drained.push(event::read()?); }
    assert_eq!(unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH) }, 0);
    println!("DRAINED={drained:?}\nFLUSHED"); io::stdout().flush()?;
    if !event::poll(Duration::from_secs(3))? { panic!("continuation deadline"); }
    println!("AFTER={:?}", event::read()?);
    terminal::disable_raw_mode()?;
    Ok(())
}
'''
AFTER_SOURCE = r'''
use crossterm::{event, terminal};
use std::{io::{self, Write}, sync::atomic::{AtomicUsize, Ordering}, time::Duration};
static SIGNALS: AtomicUsize = AtomicUsize::new(0);
extern "C" fn on_term(_: libc::c_int) { SIGNALS.fetch_add(1, Ordering::Relaxed); }
fn fd_count() -> usize { std::fs::read_dir("/dev/fd").unwrap().count() }
fn main() -> io::Result<()> {
    terminal::enable_raw_mode()?;
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "cycles" {
        unsafe { libc::signal(libc::SIGTERM, on_term as *const () as libc::sighandler_t); }
        event::poll(Duration::ZERO)?;
        let baseline=fd_count(); let mut maximum=baseline;
        for i in 0..100 {
            event::reset_event_reader()?;
            assert!(!event::poll(Duration::ZERO)?);
            maximum=maximum.max(fd_count());
            unsafe { libc::raise(libc::SIGWINCH); libc::raise(libc::SIGTERM); }
            assert!(event::poll(Duration::from_secs(1))?);
            assert!(matches!(event::read()?, event::Event::Resize(_, _)));
            if i % 25 == 0 { println!("CYCLE={i} FD={}",fd_count()); }
        }
        let final_count=fd_count();
        assert_eq!(baseline,final_count); assert_eq!(baseline,maximum);
        assert_eq!(SIGNALS.load(Ordering::Relaxed),100);
        println!("CYCLES=100 FD_BASE={baseline} FD_MAX={maximum} FD_FINAL={final_count} SIGTERM=100 SIGWINCH=100");
    } else {
        println!("READY"); io::stdout().flush()?;
        assert!(event::poll(Duration::from_secs(3))?);
        println!("FIRST={:?}",event::read()?);
        if mode != "queue-reset" {
            let mut drained=Vec::new();
            while event::poll(Duration::ZERO)? { drained.push(event::read()?); }
            println!("DRAINED={drained:?}");
        }
        event::reset_event_reader()?;
        assert_eq!(unsafe {libc::tcflush(libc::STDIN_FILENO,libc::TCIFLUSH)},0);
        assert!(!event::poll(Duration::ZERO)?);
        println!("FLUSHED"); io::stdout().flush()?;
        assert!(event::poll(Duration::from_secs(3))?);
        println!("AFTER={:?}",event::read()?);
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
'''



def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def resolve_cargo(toolchain):
    """Return the cargo argv prefix, tolerating hosts without a rustup proxy."""
    candidates = [['cargo', '+' + toolchain], ['rustup', 'run', toolchain, 'cargo'], ['cargo']]
    for argv in candidates:
        try:
            probe = subprocess.run(argv + ['--version'], capture_output=True, text=True, timeout=60)
        except (OSError, subprocess.SubprocessError):
            continue
        if probe.returncode == 0:
            return argv, probe.stdout.strip()
    return ['cargo'], 'unknown'


def inventory(root):
    return {str(p.relative_to(root)): sha(p) for p in sorted(root.rglob('*'))
            if p.is_file() and not set(p.relative_to(root).parts) & {'target', '.git', '.cargo-ok'}}


def command(argv, cwd, log, bindings):
    start = now()
    with log.open('wb') as output:
        completed = subprocess.run(argv, cwd=cwd, stdout=output, stderr=subprocess.STDOUT, timeout=240)
    bindings.append(dict(argv=argv, cwd=str(cwd), started=start, ended=now(),
                         exit=completed.returncode, log=str(log), sha256=sha(log)))
    assert completed.returncode == 0, log.read_text()[-4000:]


def tty(binary, case, first, after, evidence):
    started = now()
    pid, fd = pty.fork()
    if pid == 0:
        os.execv(str(binary), [str(binary), case])
    output = bytearray()
    status = None
    deadline = time.monotonic() + 15

    def until(marker):
        while marker not in output:
            assert time.monotonic() < deadline, (case, bytes(output))
            if select.select([fd], [], [], .05)[0]:
                try:
                    part = os.read(fd, 65536)
                except OSError:
                    part = b''
                assert part, (case, bytes(output))
                output.extend(part)

    try:
        if case != 'cycles':
            until(b'READY')
            os.write(fd, first)
            until(b'FLUSHED')
            os.write(fd, after)
            until(b'AFTER=')
        else:
            until(b'CYCLES=100')
        while time.monotonic() < deadline:
            child, value = os.waitpid(pid, os.WNOHANG)
            if child:
                status = value
                break
            if select.select([fd], [], [], .01)[0]:
                try:
                    output.extend(os.read(fd, 65536))
                except OSError:
                    pass
        assert status is not None, 'child exit deadline'
        assert os.waitstatus_to_exitcode(status) == 0, bytes(output)
        return dict(case=case, started=started, ended=now(), binary_sha256=sha(binary),
                    first_hex=first.hex(), after_hex=after.hex(), exit=0,
                    raw=output.decode(errors='replace'))
    finally:
        evidence.write_bytes(output)
        if status is None:
            try:
                os.kill(pid, signal.SIGKILL)
                os.waitpid(pid, 0)
            except ProcessLookupError:
                pass
        os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path)
    parser.add_argument('--candidate', type=Path, default=Path(__file__).resolve().parents[1]/'vendor/crossterm')
    parser.add_argument('--crate', type=Path)
    parser.add_argument('--report-dir', '--evidence', dest='evidence', type=Path, required=True)
    parser.add_argument('--toolchain', default=os.environ.get('RUSTUP_TOOLCHAIN', 'stable'))
    args = parser.parse_args()
    candidate = args.candidate.resolve()
    root = args.evidence.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    baseline = args.baseline.resolve() if args.baseline else root/'original'
    archive = args.crate.resolve() if args.crate else root/'crossterm-0.29.0.crate' 
    bindings, rows = [], []
    cargo, cargo_version = resolve_cargo(args.toolchain)
    result = dict(started=now(), status='failed', platform=os.uname().sysname,
                  cargo=dict(argv=cargo, version=cargo_version),
                  command_bindings=bindings, pty_cases=rows, linux_executed=os.uname().sysname == 'Linux')
    try:
        if not archive.exists() and args.crate is None:
            cache = Path(os.environ.get('CARGO_HOME', str(Path.home()/'.cargo')))/'registry/cache'
            cached = next(cache.glob('*/crossterm-0.29.0.crate'), None)
            if cached:
                shutil.copy2(cached, archive)
            else:
                with urllib.request.urlopen('https://static.crates.io/crates/crossterm/crossterm-0.29.0.crate', timeout=30) as response:
                    data = response.read(2 * 1024 * 1024 + 1)
                assert len(data) <= 2 * 1024 * 1024
                archive.write_bytes(data)
        assert sha(archive) == CRATE_SHA256
        archive_inventory = {}
        with tarfile.open(archive) as package:
            for member in package.getmembers():
                relative = Path(member.name).relative_to('crossterm-0.29.0')
                assert not relative.is_absolute() and '..' not in relative.parts
                if member.isdir():
                    continue
                assert member.isfile()
                data = package.extractfile(member).read()
                archive_inventory[str(relative)] = hashlib.sha256(data).hexdigest()
                if args.baseline is None:
                    path = baseline/relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(data)
        assert len(archive_inventory) == 77
        assert not (candidate/'.cargo-ok').exists(), 'Cargo install marker leaked into vendor'
        original, patched = inventory(baseline), inventory(candidate)
        assert original == archive_inventory
        assert original.keys() == patched.keys()
        changed = [name for name in original if original[name] != patched[name]]
        assert set(changed) == ALLOWED_CHANGED, changed
        directories = sorted({str(Path(name).parent) for name in original if str(Path(name).parent) != '.'})
        (root/'inventory.json').write_text(json.dumps(dict(
            crate_sha256=sha(archive), crate_file_count=len(archive_inventory),
            allowed_changed=sorted(ALLOWED_CHANGED), directories=directories,
            baseline=original, candidate=patched, changed=changed), indent=2)+'\n')
        bins = {}
        for label, source, dependency in [('before', BEFORE_SOURCE, baseline), ('after', AFTER_SOURCE, candidate)]:
            build = root/label
            (build/'src').mkdir(parents=True)
            (build/'src/main.rs').write_text(source)
            (build/'Cargo.toml').write_text('[package]\nname="reset-pty-probe"\nversion="0.0.0"\nedition="2024"\n'
                '[dependencies]\ncrossterm={path='+json.dumps(str(dependency))+',features=["bracketed-paste"]}\nlibc="0.2"\n')
            if label == 'after':
                shutil.copy2(root/'before/Cargo.lock', build/'Cargo.lock')
            command(cargo+['build', '--manifest-path', str(build/'Cargo.toml')]+(['--locked'] if label=='after' else []),
                    root, root/(label+'-build.log'), bindings)
            bins[label] = build/'target/debug/reset-pty-probe'
        for case, first, after in [('queued', b'\x07\r', b'x'),
                                   ('paste', b'\x07\x1b[200~BEFORE', b'AFTER\x1b[201~'),
                                   ('csi', b'\x07\x1b[', b'A'),
                                   ('utf8', b'\x07\xe7', b'\x95\x8cx')]:
            pair = {}
            for label, binary in bins.items():
                row = tty(binary, case, first, after, root/(label+'-'+case+'.pty'))
                row['variant'] = label
                rows.append(row)
                pair[label] = row['raw']
            if case == 'queued':
                assert all('code: Enter' in text and "Char('x')" in text for text in pair.values())
            elif case == 'paste':
                assert 'Paste("BEFOREAFTER")' in pair['before']
                assert "AFTER=Key(KeyEvent { code: Char('A')" in pair['after']
            elif case == 'csi':
                assert 'code: Up' in pair['before'] and "Char('A')" in pair['after']
            else:
                assert "Char('界')" in pair['before'] and "Char('x')" in pair['after']
        row = tty(bins['after'], 'queue-reset', b'\x07\r', b'x', root/'queue-reset.pty')
        assert 'code: Enter' not in row['raw'] and "Char('x')" in row['raw']
        rows.append(row)
        row = tty(bins['after'], 'cycles', b'', b'', root/'cycles.pty')
        assert 'SIGTERM=100 SIGWINCH=100' in row['raw']
        rows.append(row)
        unit = root/'unit-crate'
        shutil.copytree(candidate, unit, ignore=shutil.ignore_patterns('target', '.cargo-ok', '.git'))
        command(cargo+['test', '--locked', '--manifest-path', str(unit/'Cargo.toml'), '--lib'],
                root, root/'vendor-tests.log', bindings)
        vendored = (root/'vendor-tests.log').read_text()
        for expected in (
            'zenpi_reset_candidate_tests::reset_fails_without_waiting_for_an_existing_reader ... ok',
            'zenpi_ready_tests::single_edge_over_1024_bytes_delivers_every_key_without_another_write ... ok',
            'zenpi_escape_boundary_tests::one_write_full_buffer_trailing_escape_is_delivered_without_another_event ... ok',
        ):
            assert expected in vendored, expected
        assert '0 failed' in vendored, vendored[-4000:]
        result['status'] = 'passed'
    except Exception as error:
        result['error'] = str(error)
        raise
    finally:
        result['ended'] = now()
        (root/'result.json').write_text(json.dumps(result, ensure_ascii=False, indent=2)+'\n')
    print(json.dumps(dict(status=result['status'], result=str(root/'result.json'), sha256=sha(root/'result.json'))))


if __name__ == '__main__':
    main()
