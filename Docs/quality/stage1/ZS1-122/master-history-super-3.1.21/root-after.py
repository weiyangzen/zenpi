from pathlib import Path
import datetime,difflib,hashlib,json,os,shutil,subprocess
R=Path(__file__).resolve().parents[3];D=Path(__file__).resolve().parent
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def dump(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
assert not (D/'root-after-inputs.json').exists()
prior=json.loads((D/'root-before-inputs.json').read_text())
expected={p:meta(R/p) for p in prior}
assert all(expected[p]==prior[p] for p in prior if p!='src/tui.rs')
assert expected['src/tui.rs']!=prior['src/tui.rs']
assert (R/'tests/tui_composer.rs').read_bytes()==(D/'worker/project/tests/tui_composer.rs').read_bytes()
dump(D/'root-after-inputs.json',expected)
patch=''
for name in ['src/tui.rs','tests/tui_composer.rs']:
 target=D/'root-candidate'/name;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(R/name,target)
 patch+=''.join(difflib.unified_diff((D/'root-before'/name).read_text().splitlines(True),target.read_text().splitlines(True),fromfile='a/'+name,tofile='b/'+name))
(D/'root.patch').write_text(patch)
(D/'root-bin').mkdir(exist_ok=False)
shutil.copyfile(R/'target/debug/deps/tui_composer-69c8fa2b8a92f5dd',D/'root-bin/before-test')
def run(name,argv):
 assert not (D/(name+'.run.json')).exists()
 assert {p:meta(R/p) for p in expected}==expected
 env=os.environ.copy();env.update(CARGO_TARGET_DIR=str(R/'target'),CARGO_BUILD_JOBS='2')
 start=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with (D/(name+'.log')).open('xb') as out:p=subprocess.run(argv,cwd=R,env=env,stdout=out,stderr=subprocess.STDOUT,timeout=600)
 dump(D/(name+'.run.json'),dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=p.returncode,log=meta(D/(name+'.log')),env_overrides={k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_BUILD_JOBS']}))
 assert {p:meta(R/p) for p in expected}==expected
 print(name,p.returncode,flush=True)
 assert p.returncode==0,(name,(D/(name+'.log')).read_text()[-4000:])
C=['cargo','+stable-aarch64-apple-darwin']
suites=['tui_composer','tui_project_workspace','tui_bentobox','tui_interaction','tui_command_palette']
run('root-after-regression',C+['test','--locked','--offline']+sum((['--test',s] for s in suites),[]))
shutil.copyfile(R/'target/debug/deps/tui_composer-69c8fa2b8a92f5dd',D/'root-bin/after-test')
run('root-after-format',['rustup','run','stable-aarch64-apple-darwin','rustfmt','--edition','2024','--check','src/tui.rs','tests/tui_composer.rs'])
run('root-after-clippy',C+['clippy','--locked','--offline','--lib','--test','tui_composer','--','-D','warnings'])
run('root-after-build',C+['build','--locked','--offline','--bin','zenpi'])
shutil.copyfile(R/'target/debug/zenpi',D/'root-bin/zenpi');(D/'root-bin/zenpi').chmod(0o755)
dump(D/'root-after-result.json',dict(passed=True,binary=meta(D/'root-bin/zenpi'),owned={p:meta(R/p) for p in ['src/tui.rs','tests/tui_composer.rs']},scope='122 history Super local repair integrated after126 session selection; full items pending'))
print('history Super integration pipeline complete',flush=True)
