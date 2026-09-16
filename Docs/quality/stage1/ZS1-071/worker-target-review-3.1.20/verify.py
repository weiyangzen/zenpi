#!/usr/bin/env python3
"""ZS1-071 offline identity/read coverage and reversible Docs patch verification."""
import argparse, gzip, hashlib, io, json, subprocess, tarfile, tempfile
from pathlib import Path, PurePosixPath
REPORT = 'Docs/learn/stage1_pi_mono/targets/zenpi/files/src/runtime.rs_learn.md'
ENTRY = 'Docs/quality/stage1/ZS1-071/worker-target-review-3.1.20'
OLD = '006f448bfbcaf5a5ddfcec73309889e0314b175891d8704c6c4fdf32678b7a3f'
def sha(b): return hashlib.sha256(b).hexdigest()
def checked(b, f):
    assert len(b) == f['bytes'] and sha(b) == f['sha256'], f.get('path', f)
def read(t, name):
    m = t.getmember(name)
    assert m.isfile() and not m.issym() and not m.islnk(), name
    return t.extractfile(m).read()
def verify(root, live=False):
    e = root / ENTRY
    with tarfile.open(e / 'historical-target071-ready.tar.gz') as t:
        mb = read(t, 'manifest.json'); assert sha(mb) == OLD
        manifest = json.loads(mb); assert len(manifest['files']) == 16
        for f in manifest['files']: checked(read(t, f['path']), f)
        prior = read(t, 'learn-report.md'); assert len(prior) == 12129
        report = (root / REPORT).read_bytes(); assert report.startswith(prior) and len(report) > len(prior)
        baseline = gzip.decompress(read(t, 'baseline-runtime.rs.gz'))
        current = gzip.decompress(read(t, 'runtime.rs.gz'))
        assert (e / 'baseline-runtime.rs').read_bytes() == baseline
        assert (e / 'current-runtime.rs').read_bytes() == current
        checked(current, manifest['subject']); assert current.startswith(baseline)
        inventory = {f['path']:f for f in json.loads(read(t, 'replay-input-inventory.json'))}
        assert len(inventory) == 139
        with tarfile.open(fileobj=io.BytesIO(read(t, 'replay-inputs.tar.gz'))) as archive:
            assert len(archive.getmembers()) == 139
            assert {m.name for m in archive.getmembers()} == set(inventory)
            for name, f in inventory.items(): checked(read(archive, name), f)
        cmd = json.loads(read(t, 'test-command.json'))
        assert cmd['exit_code'] == 0 and cmd['passed'] == 33 and cmd['failed'] == 0
        log = read(t, 'tests.stdout.log').decode()
        for n in (12,17,4): assert f'test result: ok. {n} passed; 0 failed;' in log
        assert '1 ignored' in log and cmd['ignored_helpers'] == 1
    coverage = json.loads((e / 'read-coverage.json').read_text())
    assert len(coverage['inputs']) == 2
    for f in coverage['inputs']:
        raw = (e / (f['identity'] + '-runtime.rs')).read_bytes(); checked(raw, f)
        lines = raw.splitlines(keepends=True); assert len(lines) == f['lines']
        first, offset = 1, 0
        for c in f['chunks']:
            assert c['first_line'] == first and c['byte_start'] == offset
            b = b''.join(lines[first-1:c['last_line']]); assert sha(b) == c['sha256']
            offset += len(b); first = c['last_line'] + 1
            assert c['byte_end'] == offset
        assert offset == len(raw) and first == len(lines) + 1
    if live: assert Path('/Users/wangweiyang/GitHub/zenpi/src/runtime.rs').read_bytes() == current, 'live runtime drift'
    return {'old_payloads':16, 'historical_archive_inputs':139, 'old_report_prefix_bytes':12129,
            'baseline_lines':763, 'current_lines':912, 'historical_passed':33, 'new_behavior_runs':0,
            'live_subject_checked':live}
def packet_check(packet):
    m = json.loads((packet / 'manifest.json').read_text()); files = m['files']
    assert len({f['path'] for f in files}) == len(files)
    for f in files:
        p = PurePosixPath(f['path'])
        assert not p.is_absolute() and '..' not in p.parts and p.parts[0] == 'Docs'
        checked((packet / 'files' / f['path']).read_bytes(), f)
    patch = packet / 'candidate.patch'; checked(patch.read_bytes(), m['patch'])
    with tempfile.TemporaryDirectory(prefix='target071-patch-') as tmp:
        target = Path(tmp)
        def git(*args): subprocess.run(['git','-C',tmp,*args],check=True,capture_output=True,timeout=30)
        git('init','-q'); git('apply','--check',str(patch)); git('apply',str(patch))
        for f in files: checked((target / f['path']).read_bytes(), f)
        found = {str(p.relative_to(target)) for p in target.rglob('*') if p.is_file() and '.git' not in p.relative_to(target).parts}
        assert found == {f['path'] for f in files}
        verify(target)
        git('apply','--reverse','--check',str(patch)); git('apply','--reverse',str(patch))
        assert all(not (target / f['path']).exists() for f in files)
    return {'files':len(files),'forward':'exact final bytes','reverse':'absent receiver base restored'}
def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[5]); p.add_argument('--live',action='store_true'); p.add_argument('--packet',type=Path)
    a = p.parse_args(); out = verify(a.root.resolve(),a.live)
    if a.packet: out['packet'] = packet_check(a.packet.resolve())
    print(json.dumps(out,indent=2))
if __name__ == '__main__': main()
