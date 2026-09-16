from pathlib import Path
import datetime,hashlib,json,os,shutil,subprocess
R=Path(__file__).resolve().parents[3]
D=Path(__file__).resolve().parent
W=D/'worker'
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,x):p.write_text(json.dumps(x,ensure_ascii=False,indent=2)+'\n')
expected=json.loads((W/'capture.json').read_text())['baseline']
def audit(label):
 actual={p:meta(R/p) for p in expected}
 dump(D/(label+'-inputs.json'),actual)
 assert actual==expected,'current input drift'
def run(name,args,code=0):
 assert not (D/(name+'.log')).exists()
 audit(name+'-before')
 env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']=str(R/'target');env['PYTHONDONTWRITEBYTECODE']='1'
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (D/(name+'.log')).open('xb') as log:
  p=subprocess.run(args,cwd=R,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=600)
 dump(D/(name+'.run.json'),dict(argv=args,cwd=str(R),env_overrides={k:env[k] for k in ['CARGO_BUILD_JOBS','CARGO_TARGET_DIR','PYTHONDONTWRITEBYTECODE']},started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,log=meta(D/(name+'.log'))))
 print(name,p.returncode,flush=True)
 audit(name+'-after');assert p.returncode==code,name
CARGO=['cargo','+stable-aarch64-apple-darwin']
assert not (D/'binaries').exists()
audit('initial')
(D/'binaries').mkdir()
run('before-build',CARGO+['build','--locked','--offline','--bin','zenpi'])
shutil.copy2(R/'target/debug/zenpi',D/'binaries/before-zenpi')
test='tests/tui_composer.rs';assert (R/test).read_bytes()==(W/'baseline'/test).read_bytes()
(R/test).write_bytes((W/'candidate'/test).read_bytes());expected[test]=meta(R/test)
run('before-negative',CARGO+['test','--locked','--offline','--test','tui_composer','terminal_shortcut_'],101)
raw=(D/'before-negative.log').read_text();assert '2 passed; 3 failed;' in raw
for name in ['editor_hint_agrees_with_action_in_the_rendered_state','modified_characters_never_become_draft_text','super_key_keeps_preceding_held_ascii_without_submitting']:
 assert 'terminal_shortcut_'+name in raw
product='src/tui.rs';assert (R/product).read_bytes()==(W/'baseline'/product).read_bytes()
(R/product).write_bytes((W/'candidate'/product).read_bytes());expected[product]=meta(R/product)
suites=['tui_composer','tui_bentobox','layout','layout_persistence','tui_project_workspace','project_workspace','tui_interaction','tui_command_palette','tui_approval_focus','tui_busy_diff']
run('after-regression',CARGO+['test','--locked','--offline']+sum((['--test',t] for t in suites),[]))
run('owned-format',['rustup','run','stable-aarch64-apple-darwin','rustfmt','--edition','2024','--check',product,test])
run('after-build',CARGO+['build','--locked','--offline','--bin','zenpi'])
shutil.copy2(R/'target/debug/zenpi',D/'binaries/after-zenpi')
dump(D/'integration-result.json',dict(product_changes={p:meta(R/p) for p in [product,test]},binaries={p.name:meta(p) for p in (D/'binaries').iterdir()},passed=True,scope='Two local fixes only; current native debug builds and regression. No release or budget claim.'))
print('integration complete',flush=True)
