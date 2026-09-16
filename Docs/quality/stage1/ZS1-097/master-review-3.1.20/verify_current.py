from pathlib import Path
import hashlib, json, tomllib, datetime

ROOT = Path('/Users/wangweiyang/GitHub/zenpi')
D = Path(__file__).resolve().parent
E = D/'worker/Docs/quality/stage1/ZS1-097/worker-cargo-manifest-review-3.1.20'
sha = lambda b: hashlib.sha256(b).hexdigest()
before = (E/'source/baseline-Cargo.toml').read_bytes()
current = (ROOT/'Cargo.toml').read_bytes()
assert sha(before) == '94c25405859cbec9d1a02e7c1a0a4e0dbbfd96e5c6bc476c23b9dc8be7229a65'
assert current == (E/'source/current-Cargo.toml').read_bytes()
old, new = tomllib.loads(before.decode()), tomllib.loads(current.decode())
expected = tomllib.loads(before.decode())
expected['dependencies'].update(ignore='0.4', **{'unicode-segmentation':'=1.13.3','yaml_serde':'0.10'})
expected['profile']['release']['strip'] = 'symbols'
expected['patch'] = {'crates-io': {'crossterm': {'path': 'vendor/crossterm'}}}
assert expected == new
assert len(old['dependencies']) == 11 and len(new['dependencies']) == 14
assert new['target'] == {'cfg(unix)': {'dependencies': {'libc':'0.2'}}}
metadata = json.loads((D/'cargo-metadata.log').read_text())
package = next(p for p in metadata['packages'] if p['name'] == 'zenpi')
assert metadata['resolve'] is None  # --no-deps cannot prove a resolved feature graph.
assert package['edition'] == '2024' and package['rust_version'] == '1.88'
assert package['features'] == new['features'] == {'default': [], 'dev-fixtures': []}
dependencies = {p['name']: p for p in package['dependencies']}
normal = sorted(n for n, p in dependencies.items() if p['kind'] is None)
assert len(normal) == 15 and len(dependencies) == 16
assert dependencies['tempfile']['kind'] == 'dev'
assert dependencies['libc']['target'] == 'cfg(unix)'
assert dependencies['ratatui']['uses_default_features'] is False
assert dependencies['crossterm']['uses_default_features'] is True
assert dependencies['crossterm']['features'] == ['bracketed-paste']
assert dependencies['unicode-segmentation']['req'] == '=1.13.3'
assert next(t for t in package['targets'] if t['name'] == 'runtime_budget_probe')['src_path'] == str(ROOT/'tools/runtime_budget_probe.rs')
context = []
for group in json.loads((E/'context-capture.json').read_text()):
    path = Path(group['path'])
    raw = path.read_bytes()
    row = dict(path=str(path.relative_to(ROOT)), historical_sha256=group['full_sha256'], current_sha256=sha(raw), excerpts=[])
    for part in group['excerpts']:
        snippet = (E/part['snapshot']).read_bytes()
        position = raw.find(snippet)
        row['excerpts'].append(dict(historical_snapshot=part['snapshot'], present_unchanged=position >= 0,
            current_start_line=raw[:position].count(b'\n')+1 if position >= 0 else None))
    context.append(row)
    if row['path'] != '.github/workflows/ci.yml':
        assert all(e['present_unchanged'] for e in row['excerpts']), row
# CI changed only the budget/upload steps in the previous goal turn. Package
# feature/testing/release claims concern these still-present command strings.
ci = (ROOT/'.github/workflows/ci.yml').read_text()
for command in ('cargo clippy --all-targets --all-features -- -D warnings',
                'cargo test --all-targets --all-features --locked -- --test-threads=1',
                'ZENPI_SMOKE_FEATURES=dev-fixtures python3 tools/user_smoke.py',
                'tools/release.sh'):
    assert command in ci
assert 'steps.runtime_budget.outcome' in ci
report = dict(passed=True, mode='formal-single-file-understand', source_functions=0, source_tests=0,
    before_sha256=sha(before), current_sha256=sha(current), semantic_toml_delta_exact=True,
    cargo_metadata_no_deps=True, normal_declarations=normal, dev_declarations=['tempfile'],
    feature_graph_verified=False, MSRV_compilation_verified=False, release_verified=False,
    context=context, checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
print(json.dumps(report, indent=2))
