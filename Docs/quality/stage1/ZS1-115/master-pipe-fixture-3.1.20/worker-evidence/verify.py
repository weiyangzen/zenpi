"""Offline evidence verification and isolated single-file patch roundtrip."""
from pathlib import Path
import json,hashlib,tarfile,re,tempfile,subprocess
D=Path(__file__).resolve().parent
sha=lambda b:hashlib.sha256(b).hexdigest()
def j(n):return json.loads((D/n).read_text())
def check(b,m):assert len(b)==m['bytes'] and sha(b)==m['sha256']
scope=j('scope.json');before=(D/'before.rs').read_bytes();after=(D/'after.rs').read_bytes();check(before,scope['before']);check(after,scope['after']);check((D/'candidate.patch').read_bytes(),scope['patch'])
marker=b'#[cfg(all(test, unix))]\nmod pipe_deadline_tests';assert before.split(marker)[0]==after.split(marker)[0]
btail=before[before.index(b'    fn fixture_phase'):].decode();atail=after[after.index(b'    fn fixture_phase'):].decode()
start=atail.index('        let mut ext = catalog.loaded.get("fixture").unwrap().clone();')
end=atail.index('        let pipe =',start)
restored=atail[:start]+'        let ext = catalog.loaded.get("fixture").unwrap();\n'+atail[end:]
restored=restored.replace('            process_request(\n                &ext,','            process_request(\n                ext,')
assert restored==btail,'Existing runner, budgets, matrix, cleanup and assertions changed'
assert after.count(b'#[test]')==before.count(b'#[test]')==5 and after.count(b'#[ignore')==before.count(b'#[ignore')==1
negative=(D/'negative-startup-delay.rs').read_bytes();assert negative==after.replace(b'phase(f"python-parent-entry interpreter={sys.executable}")',b'time.sleep(2)  # negative control: readiness must miss the unchanged host deadline\nphase(f"python-parent-entry interpreter={sys.executable}")')
read=j('read-coverage.json');first=1;allread=b''
for c in read['chunks']:
 assert c['continuously_read'] and c['first_line']==first;b=b''.join(before.splitlines(True)[first-1:c['last_line']]);check(b,c);allread+=b;first=c['last_line']+1
assert allread==before
bindings=j('run-source-bindings.json');source_names={'before-build':'before.rs','before-tests':'before.rs','after-tests':'attempt1-shebang.rs','direct-interpreter-tests':'after.rs','fixture-fmt':'after.rs','negative-startup-delay-tests':'negative-startup-delay.rs'}
expected_exits={'before-build':0,'before-tests':101,'after-tests':101,'direct-interpreter-tests':0,'fixture-fmt':0,'negative-startup-delay-tests':101,'toolchain':0,'interpreter-version':0}
for name,code in expected_exits.items():
 run=j(name+'.run.json');assert run['exit_code']==code and run['HOME_preserved'] and run['CODEX_HOME_preserved'];check((D/run['log']).read_bytes(),run)
 if name in source_names:assert bindings[name]==sha((D/source_names[name]).read_bytes())
 if 'cargo' in run['argv']:assert '--offline' in run['argv'] and '--locked' in run['argv'] and any(x.startswith('CARGO_TARGET_DIR=') for x in run['argv'])
log=(D/'direct-interpreter-tests.log').read_text();assert '4 passed; 0 failed; 1 ignored;' in log
rows=j('case-timings.json');expected={(api,pipe,stop) for api in (1,2) for pipe in ('stdin','stdout') for stop in ('timeout','cancel')}|{(2,pipe,stop) for pipe in ('stdin','stdout') for stop in ('revoke','panic')};assert len(rows)==12 and {(x['api'],x['pipe'],x['stop']) for x in rows}==expected
assert len(re.findall(r'^api=.*outer_timeout=false.*status=exit status: 0',log,re.M))==12
assert len(re.findall(r'fd_count=(\d+)->\1 escaped_still_alive=true',log))==12
for row in rows:
 assert row['outer_timeout']=='false';assert row['phase_times']['host-start']<row['phase_times']['escaped-ready']<row['phase_times']['host-return'];assert row['host_start_to_escaped_ready_ms']<800
assert '3 passed; 1 failed; 1 ignored;' in (D/'before-tests.log').read_text()
assert '2 passed; 2 failed; 1 ignored;' in (D/'after-tests.log').read_text()
neg=(D/'negative-startup-delay-tests.log').read_text();assert neg.count('fixture never escaped')==4 and neg.count('actual_host_outcome=Ok(Err(CommandTimeout(800)))')==4 and '0 passed; 1 failed;' in neg
for arc,key in [('baseline-build-context.tar.gz','baseline_context_archive_sha256'),('startup-diagnostic.tar.gz','startup_diagnostic_archive_sha256')]:assert sha((D/arc).read_bytes())==scope[key]
with tarfile.open(D/'baseline-build-context.tar.gz') as tf:
 files=j('build-context-capture.json')['files'];assert sorted(tf.getnames())==sorted(x['path'] for x in files)
 for f in files:check(tf.extractfile(f['path']).read(),f)
 assert tf.extractfile('src/extension_runtime.rs').read()==before
with tarfile.open(D/'startup-diagnostic.tar.gz') as tf:
 commands=json.load(tf.extractfile('commands.json'));assert commands[0]['exit_code']==0 and commands[1]['escaped_ready'] is False and commands[1]['exit_code']==-9
 for row in commands:assert sha(tf.extractfile(row['log']).read())==row['log_sha256']
 incomplete=json.load(tf.extractfile('perl-first-incomplete.json'));assert incomplete['exit_code_unavailable'] and incomplete['status'].startswith('incomplete')
 py=json.load(tf.extractfile('python-first.run.json'));assert py['escaped_ready'] and py['exit_code']==-9
if (D/'manifest.json').exists():
 for f in j('manifest.json')['evidence_files']:check((D/f['path']).read_bytes(),f)
def git(*args,cwd):return subprocess.run(['git','-C',str(cwd),*args],capture_output=True,check=True,timeout=30).stdout
with tempfile.TemporaryDirectory(prefix='117-independent-receiver-') as tmp:
 root=Path(tmp);git('init','-q',cwd=root);f=root/'src/extension_runtime.rs';f.parent.mkdir();f.write_bytes(before);sentinel=root/'unrelated.sentinel';sentinel.write_bytes(b'preserve unrelated bytes\x00\xff\n')
 git('apply','--check',str(D/'candidate.patch'),cwd=root);git('apply',str(D/'candidate.patch'),cwd=root);assert f.read_bytes()==after
 assert sorted(str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and '.git' not in p.parts)==['src/extension_runtime.rs','unrelated.sentinel']
 assert sentinel.read_bytes()==b'preserve unrelated bytes\x00\xff\n'
 git('apply','--reverse','--check',str(D/'candidate.patch'),cwd=root);git('apply','--reverse',str(D/'candidate.patch'),cwd=root);assert f.read_bytes()==before and sentinel.read_bytes()==b'preserve unrelated bytes\x00\xff\n'
print(json.dumps(dict(status='passed',scope='offline evidence integrity only; no new behavior execution',production_prefix_identical=True,existing_runner_budget_assertion_sentinel=True,unchanged_test_count=5,unchanged_helper_ignore_count=1,final_cases_checked=12,negative_startup_cases_failed=4,context_files_checked=len(files),forward_apply_check=True,actual_forward_hash=True,reverse_apply_check=True,actual_reverse_hash=True,unrelated_sentinel_unchanged=True,master_acceptance_pending=True),indent=2))
