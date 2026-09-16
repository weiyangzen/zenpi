"""Read-only current CI semantics and Bash syntax; never executes workflow runs."""
from pathlib import Path
import hashlib, json, subprocess
import yaml

ROOT = Path('/Users/wangweiyang/GitHub/zenpi')
D = Path(__file__).resolve().parent
E = D/'worker/files/Docs/quality/stage1/ZS1-089/worker-current-ci320'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
current = ROOT/'.github/workflows/ci.yml'
assert sha(current) == '44c1d0964cd0c3504f028f3fa2dbcac89c8063ca895d039eba729dd64bc263c9'
assert current.read_bytes() == (E/'current-ci.yml').read_bytes()
assert (len(current.read_bytes()),len(current.read_bytes().splitlines())) == (3973,113)
versions = [yaml.load(p.read_text(), Loader=yaml.BaseLoader) for p in
            [E/'frozen-baseline-ci.yml',E/'previous-ci.yml',current]]
baseline, previous, now = versions
assert set(now) == {'name','on','permissions','concurrency','jobs'}
assert now['on'] == {'push':'','pull_request':''}
assert now['permissions'] == {'contents':'read'}
assert now['concurrency'] == {'group':'zenpi-${{ github.workflow }}-${{ github.ref }}','cancel-in-progress':'true'}
assert list(now['jobs']) == ['verify']
job = now['jobs']['verify']
assert set(job) == {'name','runs-on','timeout-minutes','steps'}
assert job['runs-on'] == 'ubuntu-latest' and job['timeout-minutes'] == '15'
steps = job['steps']
assert len(steps) == 16
assert [s['name'] for s in steps] == [
    'Check out source','Install stable Rust','Cache Cargo artifacts','Check formatting',
    'Run Clippy','Run tests','Validate execution blueprint and Gantt',
    'Validate v2 blueprint review draft','Report Rust source inventory',
    'Run runtime and size budget gate','Upload runtime budget receipt','Check two-mode boundary',
    'Exercise headless JSONL protocol','Exercise the installed release user paths',
    'Exercise the release headless protocol','Package and verify the production artifact']
assert [i for i,(a,b) in enumerate(zip(previous['jobs']['verify']['steps'],steps)) if a!=b] == [9,10]
assert [i for i,(a,b) in enumerate(zip(baseline['jobs']['verify']['steps'],previous['jobs']['verify']['steps'])) if a!=b] == [15]
assert steps[0]['uses']=='actions/checkout@v4'
assert steps[1]['uses']=='dtolnay/rust-toolchain@stable' and steps[1]['with']=={'components':'rustfmt, clippy'}
assert steps[2]['uses']=='Swatinem/rust-cache@v2'
assert steps[3]['run']=='cargo fmt --all -- --check'
assert steps[4]['run']=='cargo clippy --all-targets --all-features -- -D warnings'
assert steps[5]['run']=='cargo test --all-targets --all-features --locked -- --test-threads=1'
assert steps[6]['run']=='python3 tools/validate_blueprint.py'
assert steps[7]['run']=='python3 tools/validate_blueprint_v2.py'
assert steps[8]['run']=='python3 tools/check_rust_loc.py'
budget, upload = steps[9:11]
identity = '${{ runner.temp }}/zenpi-runtime-budget-${{ github.run_id }}-${{ github.run_attempt }}'
assert set(budget)=={'name','id','shell','env','run'}
assert budget['id']=='runtime_budget' and budget['shell']=='bash'
assert budget['env']=={'BUDGET_EVIDENCE_DIR':identity}
lines=budget['run'].splitlines()
assert lines[0]=='mkdir "$BUDGET_EVIDENCE_DIR"' and lines[1]=='set +e'
assert lines[5]=='  2>&1 | tee "$BUDGET_EVIDENCE_DIR/budget.log"'
assert lines[6]=='budget_status=("${PIPESTATUS[@]}")' and lines[7]=='set -e'
assert lines[8]=='printf \'%s\\n\' "${budget_status[0]}" > "$BUDGET_EVIDENCE_DIR/exit-code.txt"'
assert lines[9:]==['if [ "${budget_status[0]}" -ne 0 ]; then','  exit "${budget_status[0]}"','fi','exit "${budget_status[1]}"']
assert upload['if']=="${{ always() && steps.runtime_budget.outcome != 'skipped' }}"
assert upload['uses']=='actions/upload-artifact@v4'
assert upload['with']=={'name':'zenpi-runtime-budget-${{ github.run_id }}-${{ github.run_attempt }}','path':identity,'if-no-files-found':'error'}
assert all('continue-on-error' not in s for s in steps)
start=b'      - name: Run runtime and size budget gate\n'
end=b'      - name: Check two-mode boundary\n'
before=(E/'previous-ci.yml').read_bytes(); after=current.read_bytes()
assert before.split(start)[0]==after.split(start)[0] and before.split(end)[1]==after.split(end)[1]
contexts=[]
for ref in json.loads((E/'context-references.json').read_text()):
    p=Path(ref['source'])
    assert sha(p)==ref['sha256'] and p.stat().st_size==ref['bytes']
    raw=p.read_bytes().splitlines(keepends=True)
    for excerpt in ref['excerpts']:
        a,b=excerpt['lines']; fragment=b''.join(raw[a-1:b]); frozen=E/excerpt['artifact']
        assert fragment==frozen.read_bytes() and sha(frozen)==excerpt['sha256']
    contexts.append({'source':str(p),'sha256':sha(p),'excerpts':len(ref['excerpts'])})
syntax=[]
for i,s in enumerate(steps):
    if 'run' not in s: continue
    result=subprocess.run(['bash','--noprofile','--norc','-n'],input=s['run'],text=True,capture_output=True)
    assert result.returncode==0,result.stderr
    syntax.append({'step':i+1,'name':s['name'],'exit_code':result.returncode})
print(json.dumps({'passed':True,'yaml_loader':'BaseLoader','yaml_version':yaml.__version__,
    'steps':16,'changed_steps_previous_to_current':[10,11],'changed_steps_baseline_to_previous':[16],
    'other_14_steps_byte_identical':True,'current_ci_sha256':sha(current),'contexts':contexts,
    'bash_syntax':syntax,'workflow_run_commands_executed':0,'new_behavior_runs':0,
    'remote_actions_runs':0,'semantic_acceptance_by_checker':False},indent=2))
