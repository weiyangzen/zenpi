from pathlib import Path
import datetime, hashlib, json, os, shutil, subprocess
R=Path(__file__).resolve().parents[3]; D=Path(__file__).resolve().parent
def meta(p):
 b=p.read_bytes();return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def dump(p,v):p.write_text(json.dumps(v,ensure_ascii=False,indent=2)+'\n')
expected=json.loads((D/'main-inputs.json').read_text())
def audit(label):
 actual={p:meta(R/p) for p in expected};dump(D/(label+'-inputs.json'),actual)
 assert actual==expected, 'build input drift: '+label
def run(name,args,expected_exit=0):
 assert not (D/(name+'.run.json')).exists();audit(name+'-before')
 env=os.environ.copy();env.update(CARGO_TARGET_DIR=str(R/'target'),CARGO_BUILD_JOBS='2',PYTHONDONTWRITEBYTECODE='1')
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (D/(name+'.log')).open('xb') as f:
  p=subprocess.run(args,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=600)
 dump(D/(name+'.run.json'),{'argv':args,'cwd':str(R),'started':start,'ended':datetime.datetime.now(datetime.timezone.utc).isoformat(),'exit_code':p.returncode,'log':meta(D/(name+'.log')),'env_overrides':{k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_BUILD_JOBS','PYTHONDONTWRITEBYTECODE']}})
 print(name,p.returncode,flush=True);audit(name+'-after');assert p.returncode==expected_exit,name
def install(relative,source):
 assert meta(R/relative)==expected[relative];shutil.copy2(source,R/relative);expected[relative]=meta(R/relative)
C=['cargo','+stable-aarch64-apple-darwin']
expected=json.loads((D/'root-after-timer-after-inputs.json').read_text())
audit('resume-current-candidate')
run('root-after-timer-rebuilt',C+['test','--locked','--offline','--lib','tui::tests::approval_timer_'])
assert '13 passed; 0 failed;' in (D/'root-after-timer-rebuilt.log').read_text()
shutil.copy2(R/'target/debug/deps/zenpi-3ab8d8bb60969eba',D/'root-bin/after-test-binary')
run('root-after-canonical',C+['test','--locked','--offline','--lib','tui::tests::canonical_approval_and_drop_events_are_visible'])
suites=['tui_approval_focus','tui_bentobox','tui_command_palette','tui_composer','tui_project_workspace','tui_transcript_ux','tui_interaction','tui_busy_diff','layout','layout_persistence','project_workspace']
run('root-integration-regression',C+['test','--locked','--offline']+sum((['--test',t] for t in suites),[]))
run('root-owned-format',['rustup','run','stable-aarch64-apple-darwin','rustfmt','--edition','2024','--check','src/tui.rs','tests/tui_transcript_ux.rs'])
run('root-clippy',C+['clippy','--locked','--offline','--lib','--test','tui_transcript_ux','--','-D','warnings'])
run('root-debug-build',C+['build','--locked','--offline','--bin','zenpi'])
shutil.copy2(R/'target/debug/zenpi',D/'root-bin/zenpi')
dump(D/'root-integration-result.json',{'passed':True,'binary':meta(D/'root-bin/zenpi'),'changes':{p:meta(R/p) for p in ['src/tui.rs','tests/tui_transcript_ux.rs']},'scope':'125 approval timer local repair; not full125 or117 acceptance'})
print('root integration pipeline completed',flush=True)
