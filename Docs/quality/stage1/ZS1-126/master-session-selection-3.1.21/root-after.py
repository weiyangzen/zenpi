from pathlib import Path
import datetime, difflib, hashlib, json, os, shutil, subprocess

R = Path(__file__).resolve().parents[3]
D = Path(__file__).resolve().parent
OWNED = ['src/tui.rs', 'tests/tui_project_workspace.rs']

def meta(p):
    data = p.read_bytes()
    return dict(bytes=len(data), sha256=hashlib.sha256(data).hexdigest())

def dump(p, data):
    p.write_text(json.dumps(data, ensure_ascii=False, indent=2) + '\n')

assert not (D / 'after-inputs.json').exists()
initial = json.loads((D / 'initial-inputs.json').read_text())
expected = {p: meta(R / p) for p in initial}
assert all(expected[p] == initial[p] for p in initial if p not in OWNED)
assert all(expected[p] != initial[p] for p in OWNED)
dump(D / 'after-inputs.json', expected)
patch = ''
for name in OWNED:
    target = D / 'candidate' / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(R / name, target)
    patch += ''.join(difflib.unified_diff((D/'before'/name).read_text().splitlines(True),target.read_text().splitlines(True),fromfile='a/'+name,tofile='b/'+name))
(D / 'root.patch').write_text(patch)
(D / 'bin').mkdir(exist_ok=False)
shutil.copyfile(R/'target/debug/deps/tui_project_workspace-1eed673f1bf434ae',D/'bin/before-test')

def run(name, argv):
    assert not (D / (name + '.run.json')).exists()
    assert {p:meta(R/p) for p in expected} == expected
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS='2', CARGO_TARGET_DIR=str(R/'target'))
    start = datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (D / (name + '.log')).open('xb') as out:
        result = subprocess.run(argv,cwd=R,env=env,stdout=out,stderr=subprocess.STDOUT,timeout=600)
    dump(D/(name+'.run.json'),dict(argv=argv,cwd=str(R),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=result.returncode,log=meta(D/(name+'.log')),env_overrides={k:env[k] for k in ['CARGO_BUILD_JOBS','CARGO_TARGET_DIR']}))
    assert {p:meta(R/p) for p in expected} == expected
    print(name, result.returncode, flush=True)
    assert result.returncode == 0, (name, (D/(name+'.log')).read_text()[-4000:])

C = ['cargo','+stable-aarch64-apple-darwin']
suites = ['tui_project_workspace','tui_bentobox','tui_composer','layout','layout_persistence','project_workspace','tui_approval_focus','tui_busy_diff','tui_interaction']
run('after-regression', C+['test','--locked','--offline']+sum((['--test',name] for name in suites),[]))
shutil.copyfile(R/'target/debug/deps/tui_project_workspace-1eed673f1bf434ae',D/'bin/after-test')
run('after-format', ['rustup','run','stable-aarch64-apple-darwin','rustfmt','--edition','2024','--check']+OWNED)
run('after-clippy', C+['clippy','--locked','--offline','--lib','--test','tui_project_workspace','--','-D','warnings'])
run('after-build', C+['build','--locked','--offline','--bin','zenpi'])
shutil.copyfile(R/'target/debug/zenpi',D/'bin/zenpi')
(D/'bin/zenpi').chmod(0o755)
dump(D/'after-result.json',dict(passed=True,binary=meta(D/'bin/zenpi'),owned={p:meta(R/p) for p in OWNED},scope='126 session selection local repair; not full126/128/G-HOST acceptance'))
print('session selection integration checks complete',flush=True)
