#!/usr/bin/env python3
"""Stage 1 structural/evidence gate; stdlib only, never executes receipt commands.

Default: draft structure and current frozen file hashes (no selector needed).
--item/--strict: require the selected/all item evidence and a matching selector.
--bootstrap --actor master --run-id ID: initialize blank manifests/indexes and
activate the explicit blueprint; selector is written last. No reports, receipts,
claims, or checkbox promotions are generated. Existing scaffold is never replaced.

Receipts live at <evidence-root>/receipts/<item>.{worker,master}.json. They are
reviewable evidence, not authentication: a trusted master must control this
checkout and author the master receipt. JSON checks cannot establish semantic
truth or distinguish a fully fabricated transcript from a real observation.
"""
from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shlex
import subprocess
import sys
from dataclasses import dataclass
from typing import Any

from validate_blueprint import BlueprintError, _yaml_header

ROOT = Path(__file__).resolve().parents[1]
BLUEPRINT = 'Docs/stage_1_v3_pi_mono_blueprint.md'
EVIDENCE = 'Docs/learn/stage1_pi_mono'
SELECTOR = 'Docs/execution/active_requirement.json'
STATES = ('[ ]', '[_]', '[x]')
ID = r'ZS1-[0-9]{3}'
SHA = r'[0-9a-f]{64}'
LIMIT = 256 * 1024
FIELDS = ('Depends', 'Owner scope', 'Owned paths', 'Validators', 'Rollback', 'Estimate', 'Estimated LOC')
MANIFEST_FIELDS = ('source_id', 'source_path', 'source_kind', 'source_bytes', 'source_hash',
                   'subset_id', 'group_id', 'chunk_set_id', 'target_artifact', 'folder_artifact',
                   'mapping_mode', 'item_id', 'status')
FILE_FIELDS = ('item_id', 'source_path', 'source_hash', 'target_artifact', 'status')
DIR_FIELDS = ('item_id', 'folder_path', 'folder_artifact', 'depends', 'status')
CHUNK_FIELDS = ('item_id', 'source_path', 'start', 'end', 'source_hash')


def need(condition: Any, message: str) -> None:
    if not condition:
        raise BlueprintError(message)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def safe_rel(value: str, allow_root: bool = False) -> str:
    need(isinstance(value, str) and bool(value), 'empty/non-string path')
    path = PurePosixPath(value)
    need(not path.is_absolute() and '\\' not in value and '\x00' not in value
         and '..' not in path.parts and (allow_root or value != '.')
         and str(path) == value and not any(c in value for c in '*?[]'), f'unsafe path: {value!r}')
    return value


def local(root: Path, rel: str, allow_root: bool = False) -> Path:
    safe_rel(rel, allow_root)
    candidate = root / rel
    need(candidate.resolve().is_relative_to(root.resolve()), f'path escapes root: {rel}')
    for part in (candidate, *candidate.parents):
        if part == root:
            break
        need(not part.is_symlink(), f'symlink rejected: {rel}')
    return candidate


def read_json(path: Path) -> dict:
    def pairs(values: list) -> dict:
        result = {}
        for key, value in values:
            need(key not in result, f'duplicate JSON key: {key}')
            result[key] = value
        return result
    raw = path.read_bytes()
    need(raw.endswith(b'\n'), f'incomplete JSON record (missing final newline): {path}')
    value = json.loads(raw, object_pairs_hook=pairs)
    need(isinstance(value, dict), f'expected JSON object: {path}')
    return value


def requirement_digest(text: str) -> str:
    """Ignore checklist marks, header activation fields, and runtime JSON.

    Runtime blocks may contain ONLY activation/receipt references, never prose
    requirements. Unknown keys/unterminated blocks fail closed. Everything else,
    including behavioral prose and the frozen scope tables, is digest-sensitive.
    """
    lines, inside, runtime = [], False, []
    header_seen, in_header = False, False
    for line in text.splitlines(keepends=True):
        if line.rstrip('\r\n') == '<!-- stage1-runtime:begin -->':
            need(not inside, 'nested runtime block')
            inside, runtime = True, []
        elif line.rstrip('\r\n') == '<!-- stage1-runtime:end -->':
            need(inside, 'unmatched runtime block end')
            value = json.loads(''.join(runtime))
            need(isinstance(value, dict) and set(value) <= {'activation_ref', 'receipt_refs'},
                 'runtime block may contain only activation_ref/receipt_refs')
            if 'activation_ref' in value:
                safe_rel(value['activation_ref'])
            if 'receipt_refs' in value:
                need(isinstance(value['receipt_refs'], list), 'receipt_refs must be a list')
                for ref in value['receipt_refs']:
                    safe_rel(ref)
            inside = False
        elif inside:
            runtime.append(line)
        else:
            if line.strip() == '```yaml' and not header_seen:
                header_seen, in_header = True, True
            elif line.strip() == '```' and in_header:
                in_header = False
            if in_header and re.match(r'^(status|authoritative):', line):
                line = line.split(':', 1)[0] + ': <activation-state>\n'
            lines.append(re.sub(r'^(\s*-\s*)\[[ _x]\](\s+\*\*' + ID + r'\*\*)',
                                r'\1[ ]\2', line))
    need(not inside, 'unterminated runtime block')
    return digest(''.join(lines).encode())


@dataclass(frozen=True)
class Item:
    item_id: str
    state: str
    layer: str
    depends: tuple[str, ...]
    owned_paths: tuple[str, ...]
    validators: str
    loc: int


@dataclass(frozen=True)
class File:
    item_id: str
    source_id: str
    scope: str
    path: str
    sha256: str
    declared_bytes: int | None
    artifact: str


@dataclass
class Blueprint:
    text: str
    header: dict
    items: dict[str, Item]
    files: dict[str, File]
    folders: dict[str, tuple[str, str, str]]  # id -> scope, folder, artifact
    order: list[str]
    requirement: str
    snapshot: str
    scopes: dict[str, tuple[str, str]]  # namespace -> artifact prefix, input repo
    provisional_claims: bool
    source_revisions: dict[str, str]


def parse(text: str, evidence: str = EVIDENCE) -> Blueprint:
    header = _yaml_header(text)
    expected = {'schema_version': 'execution-blueprint/stage1',
                'stable_id_pattern': '^ZS1-[0-9]{3}$', 'status_values': '[ ]|[_]|[x]',
                'per_item_code_loc_cap': 5000, 'per_item_code_loc_rule': 'estimated_loc < 5000',
                'target_baseline': 'current-worktree-with-user-changes',
                'source_scope': 'explicit-file-list-in-section-3', 'audit_mode': 'understand',
                'product_modes': ['tui', 'headless'], 'worker_acceptance': 'self_test_only',
                'master_acceptance': 'integrated-behavioral-evidence'}
    for key, value in expected.items():
        need(header.get(key) == value, f'header {key} must be {value!r}')
    for key in ('source_revision', 'target_revision'):
        need(re.fullmatch(r'[0-9a-f]{40}', str(header.get(key, ''))), f'invalid {key}')
    need(type(header.get('authoritative')) is bool, 'authoritative must be boolean')
    need(bool(header.get('blueprint_version')), 'missing blueprint version')
    items = {}
    for n, line in enumerate(text.splitlines(), 1):
        if not re.match(r'^\s*-\s*\[', line):
            continue
        match = re.fullmatch(r'\s*-\s*(\[[ _x]\])\s+\*\*(' + ID + r')\*\*\s+(.+)', line)
        need(match, f'line {n}: invalid checkbox/ID/row')
        state, item_id, body = match.groups()
        need(item_id not in items, f'duplicate ID: {item_id}')
        cells = re.split(r'\s*\|\s*', body)
        need(len(cells) == len(FIELDS) + 1, f'{item_id}: malformed/repeated fields')
        fields = {}
        for name, cell in zip(FIELDS, cells[1:]):
            need(cell.startswith(name + ':') and cell[len(name)+1:].strip(), f'{item_id}: missing {name}')
            fields[name] = cell[len(name)+1:].strip()
        layer = re.search(r'layer `(L[0-6])`$', cells[0])
        need(layer, f'{item_id}: invalid layer')
        deps = () if fields['Depends'] == '—' else tuple(fields['Depends'].split(','))
        need(all(re.fullmatch(ID, dep.strip()) for dep in deps), f'{item_id}: malformed dependencies')
        deps = tuple(dep.strip() for dep in deps)
        need(len(set(deps)) == len(deps), f'{item_id}: duplicate dependency')
        need(re.fullmatch(r'`[^`]+`(?:,\s*`[^`]+`)*', fields['Owned paths']), f'{item_id}: malformed owned paths')
        paths = tuple(safe_rel(path) for path in re.findall(r'`([^`]+)`', fields['Owned paths']))
        need(len(paths) == len(set(paths)), f'{item_id}: duplicate owned path')
        need(re.fullmatch(r'[0-9]+', fields['Estimated LOC']), f'{item_id}: invalid LOC')
        loc = int(fields['Estimated LOC'])
        need(0 <= loc < 5000, f'{item_id}: LOC must be < 5000')
        items[item_id] = Item(item_id, state, layer.group(1), deps, paths, fields['Validators'], loc)
    need('ZS1-001' in items and not items['ZS1-001'].depends, 'missing dependency-free bootstrap ZS1-001')
    order, visiting = [], set()
    def visit(key: str) -> None:
        need(key in items, f'missing dependency: {key}')
        need(key not in visiting, f'DAG cycle: {key}')
        if key in order:
            return
        visiting.add(key)
        for dep in items[key].depends:
            visit(dep)
        visiting.remove(key)
        order.append(key)
    for key in items:
        visit(key)
    files = {}
    scopes = {}
    revisions = {'source': header['source_revision']}
    for line in text.splitlines():
        if not re.match(r'^\| ZS1-', line):
            continue
        cells = [cell.strip() for cell in line.strip('|').split('|')]
        match = re.fullmatch(r'(' + ID + r')(?: / (SRC-[0-9]+))?', cells[0])
        need(match, 'malformed scope item/source ID')
        item_id, source_id = match.groups()
        need(len(cells) == (5 if source_id else 4), f'{item_id}: malformed scope row')
        link = re.fullmatch(r'\[([^\]]+)\]\(([^)]+)\)', cells[1])
        need(link, f'{item_id}: malformed scope path')
        path, url = link.groups()
        safe_rel(path)
        need(item_id in items and item_id not in files, f'{item_id}: missing or duplicate file checklist')
        item = items[item_id]
        need(len(item.owned_paths) == 1 and '/files/' in item.owned_paths[0], f'{item_id}: missing per-file namespace')
        prefix = item.owned_paths[0].split('/files/', 1)[0]
        if source_id:
            scope, repo = 'source', header['source_repo']
            need(prefix == evidence, f'{item_id}: source namespace mismatch')
        elif prefix == evidence + '/targets/zenpi':
            scope, repo = 'target', header['target_repo']
        else:
            need(re.fullmatch(re.escape(evidence) + r'/references/[A-Za-z0-9_-]+', prefix),
                 f'{item_id}: invalid reference namespace')
            scope = 'reference:' + prefix.rsplit('/', 1)[1]
            need(url.endswith('/' + path), f'{item_id}: reference URL/path mismatch')
            repo = url[:-len(path)-1]
            revision = re.search(re.escape('`' + repo + '`') + r'，HEAD `([0-9a-f]{40})`', text)
            need(revision,
                 f'{item_id}: reference repository lacks frozen HEAD declaration')
            revisions[scope] = revision.group(1)
        need(url == str(PurePosixPath(repo) / path), f'{item_id}: scope link/path mismatch')
        need(scope not in scopes or scopes[scope] == (prefix, repo), f'{item_id}: mixed reference roots')
        scopes[scope] = (prefix, repo)
        sha = cells[-1].strip('`')
        need(re.fullmatch(SHA, sha), f'{item_id}: invalid scope hash')
        need(item_id in items and item_id not in files, f'{item_id}: missing or duplicate file checklist')
        artifact = f'{prefix}/files/{path}_learn.md'
        item = items[item_id]
        need(item.owned_paths == (artifact,) and 'G-FILE' in item.validators and item.loc == 0,
             f'{item_id}: file scope/checklist mismatch')
        declared_bytes = None if source_id else int(cells[2])
        files[item_id] = File(item_id, source_id or item_id, scope, path, sha, declared_bytes, artifact)
    need(files, 'no frozen files')
    need({'source', 'target'} <= set(scopes), 'missing source or target tree')
    for scope in scopes:
        scoped = [f for f in files.values() if f.scope == scope]
        need(scoped and len({f.path for f in scoped}) == len(scoped), f'{scope}: missing/duplicate file path')
        need(len({f.source_id for f in scoped}) == len(scoped), f'{scope}: duplicate source ID')
    need({key for key, item in items.items() if 'G-FILE' in item.validators} == set(files),
         'file closure: scope table and G-FILE items differ')
    folders = {}
    for key, item in items.items():
        if 'G-DIR' not in item.validators:
            continue
        need(len(item.owned_paths) == 1 and item.loc == 0, f'{key}: invalid directory ownership')
        artifact = item.owned_paths[0]
        matching = [(s, prefix) for s, (prefix, _) in scopes.items() if artifact.startswith(prefix + '/')]
        need(matching, f'{key}: folder outside frozen namespaces')
        scope, prefix = max(matching, key=lambda pair: len(pair[1]))
        need(artifact.startswith(prefix + '/') and artifact.endswith('/current_folder_learn.md'),
             f'{key}: invalid folder artifact')
        folder = str(PurePosixPath(artifact).parent.relative_to(prefix))
        folders[key] = (scope, folder, artifact)
    for scope in scopes:
        expected_dirs = {str(parent) for f in files.values() if f.scope == scope
                         for parent in PurePosixPath(f.path).parents}
        actual_dirs = [folder for s, folder, _ in folders.values() if s == scope]
        need(len(actual_dirs) == len(set(actual_dirs)) and set(actual_dirs) == expected_dirs,
             f'{scope}: directory closure mismatch (missing={sorted(expected_dirs-set(actual_dirs))})')
        for key, (s, folder, _) in folders.items():
            if s != scope:
                continue
            children = {f.item_id for f in files.values() if f.scope == scope and str(PurePosixPath(f.path).parent) == folder}
            children |= {k for k, (ss, d, _) in folders.items() if ss == scope and d != '.' and str(PurePosixPath(d).parent) == folder}
            need(set(items[key].depends) == children, f'{key}: directory immediate-child dependency mismatch')
    provisional = 'worker按DAG顺序在各自worktree准备候选' in text
    return Blueprint(text, header, items, files, folders, order, requirement_digest(text), digest(text.encode()), scopes, provisional, revisions)


def repository_head(root: Path) -> str:
    result = subprocess.run(['git', 'rev-parse', '--verify', 'HEAD'], cwd=root,
                            capture_output=True, text=True, check=False)
    need(result.returncode == 0 and re.fullmatch(r'[0-9a-f]{40}', result.stdout.strip()),
         f'cannot verify repository HEAD: {root}')
    return result.stdout.strip()


def check_integrated_revision(root: Path, revision: str) -> None:
    need(re.fullmatch(r'[0-9a-f]{40}', revision), 'invalid integrated revision')
    result = subprocess.run(['git', 'merge-base', '--is-ancestor', revision, 'HEAD'],
                            cwd=root, capture_output=True, check=False)
    need(result.returncode == 0, 'integrated revision is not an ancestor of current HEAD')


def integration_inputs(root: Path, item: Item) -> dict[str, str]:
    """Hash owned implementation/config inputs plus root build configuration.

    Historical receipts survive unrelated commits only while these bytes stay
    identical. Docs are separately bound by artifact receipts; generated runtime
    surfaces and checkbox changes do not invalidate an implementation receipt.
    """
    paths = {p for p in item.owned_paths if not p.startswith('Docs/')}
    paths.update(p for p in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain',
                            'rust-toolchain.toml', '.cargo/config', '.cargo/config.toml')
                 if local(root, p).exists())
    inputs = {}
    for rel in sorted(paths):
        path = local(root, rel)
        need(path.exists(), f'{item.item_id}: missing integrated input: {rel}')
        if path.is_dir():
            for child in sorted(path.rglob('*')):
                child_rel = child.relative_to(root).as_posix()
                verified = local(root, child_rel)
                if verified.is_file():
                    inputs[child_rel] = digest(verified.read_bytes())
        else:
            inputs[rel] = digest(path.read_bytes())
    return inputs


def file_hashes(bp: Blueprint, root: Path, source_root: Path, *, target: bool = True, reference_roots: dict | None = None) -> dict[str, int]:
    sizes = {}
    roots = {'source': source_root, 'target': root}
    roots.update({scope: Path((reference_roots or {}).get(scope.split(':', 1)[1], repo))
                  for scope, (_, repo) in bp.scopes.items() if scope.startswith('reference:')})
    for scope, revision in bp.source_revisions.items():
        need(repository_head(roots[scope]) == revision, f'{scope}: frozen repository HEAD mismatch')
    for key, file in bp.files.items():
        if file.scope == 'target' and not target:
            sizes[key] = file.declared_bytes
            continue
        input_root = roots[file.scope]
        data = local(input_root, file.path).read_bytes()
        need(digest(data) == file.sha256, f'{key}: {file.scope} hash mismatch: {file.path}')
        if file.declared_bytes is not None:
            need(len(data) == file.declared_bytes, f'{key}: target bytes mismatch')
        sizes[key] = len(data)
    return sizes


def scaffold(bp: Blueprint, sizes: dict[str, int], evidence: str) -> dict[str, tuple[tuple, list[dict]]]:
    outputs = {}
    for scope, (prefix, _) in bp.scopes.items():
        manifests, indexes, directories = [], [], []
        for file in bp.files.values():
            if file.scope != scope:
                continue
            row = dict(source_id=file.source_id, source_path=file.path, source_kind=scope,
                       source_bytes=str(sizes[file.item_id]), source_hash=file.sha256,
                       subset_id='stage1_pi_mono', group_id='', chunk_set_id=file.item_id if sizes[file.item_id] > LIMIT else '',
                       target_artifact=file.artifact,
                       folder_artifact=f'{prefix}/{PurePosixPath(file.path).parent}/current_folder_learn.md'.replace('/./', '/'),
                       mapping_mode='understand', item_id=file.item_id, status=bp.items[file.item_id].state)
            manifests.append(row)
            indexes.append({k: row[k] for k in FILE_FIELDS})
        for key, (s, folder, artifact) in bp.folders.items():
            if s == scope:
                directories.append(dict(item_id=key, folder_path=folder, folder_artifact=artifact,
                                        depends=','.join(bp.items[key].depends), status=bp.items[key].state))
        for name, fields, rows in (('source_manifest.tsv', MANIFEST_FIELDS, manifests),
                                   ('file_learn_index.tsv', FILE_FIELDS, indexes),
                                   ('folder_learn_index.tsv', DIR_FIELDS, directories)):
            outputs[prefix + '/' + name] = (fields, rows)
    chunks = [dict(item_id=f.item_id, source_path=f.path, start=str(start), end=str(min(start+LIMIT, sizes[f.item_id])), source_hash=f.sha256)
              for f in bp.files.values() if sizes[f.item_id] > LIMIT
              for start in range(0, sizes[f.item_id], LIMIT)]
    outputs[evidence + '/chunk_manifest.tsv'] = (CHUNK_FIELDS, chunks)
    return outputs


def tsv(fields: tuple, rows: list[dict]) -> bytes:
    stream = io.StringIO(newline='')
    writer = csv.DictWriter(stream, fields, delimiter='\t', lineterminator='\n')
    writer.writeheader()
    writer.writerows(rows)
    return stream.getvalue().encode()


def check_scaffold(root: Path, expected: dict) -> None:
    for rel, (fields, rows) in expected.items():
        raw = local(root, rel).read_text()
        need(raw.endswith('\n'), f'truncated index: {rel}')
        reader = csv.DictReader(io.StringIO(raw), delimiter='\t')
        need(reader.fieldnames == list(fields), f'index columns mismatch: {rel}')
        actual = list(reader)
        need(sorted(map(canonical, actual)) == sorted(map(canonical, rows)), f'index coverage/hash/state mismatch: {rel}')


def overlap(a: str, b: str) -> bool:
    return a == b or a.startswith(b + '/') or b.startswith(a + '/')


def frontiers(bp: Blueprint, claims: list[dict], run_id: str, capacity: int) -> dict:
    need(type(capacity) is int and capacity >= 0, 'invalid worker capacity')
    need(not bp.provisional_claims or capacity <= 3, 'operator worker cap is 3')
    live = {}
    for claim in claims:
        key = claim.get('item_id')
        need(key in bp.items and key != 'ZS1-001' and key not in live, 'invalid/duplicate/bootstrap claim')
        need(claim.get('run_id') == run_id and claim.get('requirement_digest') == bp.requirement, 'stale claim binding')
        need(claim.get('runtime_status') in ('live', 'finished'), 'invalid claim runtime status')
        need(claim.get('owned_paths') == list(bp.items[key].owned_paths), 'claim owned paths mismatch')
        need(bool(claim.get('session')), 'claim missing session')
        need(bp.items[key].state != '[x]', 'accepted item has claim')
        live[key] = claim
    reservations = [c for c in claims if c['runtime_status'] == 'live']
    need(len(reservations) <= capacity, 'live worker capacity exceeded')
    claim_order, integration, blocked = [], [], []
    reserved = [p for c in reservations for p in c['owned_paths']]
    for key in bp.order:
        item = bp.items[key]
        deps_ok = all(bp.items[d].state == '[x]' for d in item.depends)
        integration_conflicts = any(overlap(p, q) for c in reservations if c['item_id'] != key
                                    for p in item.owned_paths for q in c['owned_paths'])
        conflicts = any(overlap(p, q) for p in item.owned_paths for q in reserved)
        if item.state == '[_]':
            (integration if deps_ok and not integration_conflicts else blocked).append(key)
        elif item.state == '[ ]' and key not in ('ZS1-001', 'ZS1-199') and key not in live and (
            bp.provisional_claims or (deps_ok and not conflicts)):
            if len(claim_order) < max(0, capacity-len(reservations)):
                claim_order.append(key)
                reserved.extend(item.owned_paths)
    return dict(worker_claim_frontier=claim_order, integration_frontier=integration,
                integration_blocked=blocked, live_workers=len(reservations), capacity=capacity,
                saturation=len(reservations) + len(claim_order), claims=claims, cycle_detected=False,
                provisional_claims=[key for key in claim_order if not all(bp.items[d].state == '[x]' for d in bp.items[key].depends)])


def todo(bp: Blueprint, frontier: dict, blueprint: str, ledger: str) -> str:
    counts = {name: sum(i.state == state for i in bp.items.values()) for name, state in
              zip(('not_done', 'worker_self_tested', 'master_accepted'), STATES)}
    lines = [f'# Stage 1 todo — {dt.date.today():%Y%m%d}', f'Source: {blueprint}',
             f'requirement_digest: {bp.requirement}', f'snapshot_sha256: {bp.snapshot}',
             f'Claim ledger: {ledger}', *[f'{k}={v}' for k, v in counts.items()],
             f'Unfinished={counts["not_done"]+counts["worker_self_tested"]}',
             'Cycle detected: false', f'Worker saturation: {frontier["saturation"]}/{frontier["capacity"]}',
             'Worker claim frontier: ' + ','.join(frontier['worker_claim_frontier']),
             'Integration frontier: ' + ','.join(frontier['integration_frontier']),
             'Integration blocked: ' + ','.join(frontier['integration_blocked'])]
    claims = {c['item_id']: c for c in frontier['claims']}
    for key in bp.order:
        item = bp.items[key]
        if item.state == '[x]':
            continue
        claim = claims.get(key)
        state = f'{claim["runtime_status"]}:{claim["session"]}' if claim else 'unclaimed'
        integration = 'ready' if key in frontier['integration_frontier'] else 'blocked'
        lines.append(f'- {item.state} **{key}** | Depends: {",".join(item.depends)} | Claim: {state} | Integration: {integration} | Owned paths: {",".join(item.owned_paths)}')
    return '\n'.join(lines) + '\n'


def selector(root: Path, bp: Blueprint, blueprint: str) -> dict:
    value = read_json(local(root, SELECTOR))
    need(value.get('schema_version') == 'stage1-selector/v1' and value.get('active') is True, 'inactive/invalid selector')
    need(value.get('blueprint') == blueprint and value.get('requirement_digest') == bp.requirement
         and value.get('snapshot_sha256') == bp.snapshot and value.get('blueprint_version') == bp.header['blueprint_version'],
         'selector requirement/snapshot/version mismatch')
    need(bool(value.get('run_id')) and value.get('activated_by') == 'master', 'selector requires run_id and master activation')
    need(value.get('target_baseline') == 'current-worktree-with-user-changes', 'selector has wrong baseline')
    for file in local(root, 'Docs/execution').glob('*.json'):
        if file.name == 'active_requirement.json':
            continue
        other = read_json(file)
        need(not (other.get('active') is True and ('blueprint' in other or 'requirement_digest' in other)), 'multiple active requirements')
    baseline = value.get('baseline_files', {})
    need(set(baseline) == set(bp.files), 'selector baseline file closure mismatch')
    for key, file in bp.files.items():
        record = baseline[key]
        need(record.get('path') == file.path and record.get('scope') == file.scope
             and record.get('sha256') == file.sha256 and type(record.get('bytes')) is int
             and record['bytes'] >= 0, f'{key}: selector baseline mismatch')
        if file.declared_bytes is not None:
            need(record['bytes'] == file.declared_bytes, f'{key}: selector baseline bytes mismatch')
    need(value.get('baseline_snapshot_sha256') == digest(canonical(baseline)), 'selector baseline digest mismatch')
    return value


def artifact(root: Path, value: dict, *, nonempty: bool = True) -> bytes:
    need(isinstance(value, dict), 'invalid artifact record')
    data = local(root, value.get('path', '')).read_bytes()
    need(type(value.get('bytes')) is int and value['bytes'] == len(data)
         and value.get('sha256') == digest(data), f'artifact bytes/hash mismatch: {value.get("path")}')
    need(not nonempty or bool(data.strip()), f'empty artifact: {value["path"]}')
    return data


def check_ranges(ranges: list, size: int) -> None:
    need(isinstance(ranges, list) and bool(ranges), 'missing byte ranges')
    cursor = 0
    for pair in ranges:
        need(isinstance(pair, list) and len(pair) == 2 and all(type(n) is int for n in pair), 'invalid range')
        start, end = pair
        need(start == cursor and 0 < end-start <= LIMIT, 'range gap/overlap/oversized chunk')
        cursor = end
    need(cursor == size, 'byte ranges do not cover whole file')


def normalize_argv(argv: list[str]) -> list[str]:
    # The 3.1.0 blueprint explicitly authorizes this installed toolchain prefix.
    if argv[:2] == ['cargo', '+stable-aarch64-apple-darwin']:
        return [argv[0], *argv[2:]]
    return argv


def command_evidence(root: Path, commands: list, validators: str) -> None:
    need(isinstance(commands, list) and bool(commands), 'missing command evidence')
    allowed = []
    for part in re.split(r'[；;]', validators):
        part = part.strip()
        if part.startswith(('cargo ', 'python3 ')):
            allowed.append(shlex.split(part))
    if 'G-RUST' in validators:
        allowed.extend([['cargo', 'fmt', '--check'], ['cargo', 'check', '--locked'], ['cargo', 'test', '--locked']])
    if 'G-PROD' in validators:
        allowed.extend([['cargo', 'build', '--release', '--locked'],
                        ['python3', 'tools/stage1_host_smoke.py', '--binary', 'target/release/zenpi',
                         '--report-dir', 'Docs/quality/stage1/ZS1-117']])
    if 'G-BASE' in validators:
        allowed.extend([['python3', 'tools/validate_blueprint.py'], ['python3', 'tools/validate_blueprint_v2.py']])
    if 'G-STAGE' in validators:
        allowed.extend([['python3', 'tools/validate_stage1_blueprint.py'],
                        ['python3', 'tools/validate_stage1_blueprint.py', '--blueprint', BLUEPRINT, '--evidence-root', EVIDENCE]])
    expected_targets = set(re.findall(r'--test\s+([\w-]+)', validators))
    seen = set()
    for command in commands:
        need(isinstance(command, dict), 'invalid command record')
        argv = command.get('argv')
        need(isinstance(argv, list) and argv and all(isinstance(v, str) and v for v in argv), 'invalid argv')
        argv = normalize_argv(argv)
        need(argv in allowed, 'command argv is not a prescribed validator')
        need(command.get('exit_code') == 0 and type(command.get('exit_code')) is int, 'command failed/missing exit')
        env_names = command.get('env_names')
        need(command.get('cwd') == '.' and isinstance(env_names, list)
             and all(isinstance(n, str) and re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', n) for n in env_names)
             and len(env_names) == len(set(env_names)), 'invalid command cwd/env names')
        start = dt.datetime.fromisoformat(command.get('started_at', '').replace('Z', '+00:00'))
        end = dt.datetime.fromisoformat(command.get('ended_at', '').replace('Z', '+00:00'))
        need(start.tzinfo and end.tzinfo and end >= start, 'invalid command timestamps')
        out = artifact(root, command.get('stdout'), nonempty=False)
        err = artifact(root, command.get('stderr'), nonempty=False)
        if 'test' in argv or any('test_' in v for v in argv):
            count = command.get('tests_run')
            need(type(count) is int and count > 0, 'zero/missing executed tests')
            text = (out + b'\n' + err).decode('utf-8', errors='replace')
            python_counts = re.findall(r'Ran (\d+) tests? in ', text)
            rust_counts = re.findall(r'test result: ok\. (\d+) passed;', text)
            need(sum(map(int, python_counts + rust_counts)) == count, 'test count lacks runner output')
            if '--test' in argv:
                need(len(rust_counts) == argv.count('--test') and all(int(c) > 0 for c in rust_counts),
                     'zero/missing per-target test execution')
            for index, arg in enumerate(argv[:-1]):
                if arg == '--test':
                    target = argv[index+1]
                    need(local(root, f'tests/{target}.rs').is_file(), f'missing test target: {target}')
                    seen.add(target)
        for arg in argv:
            if arg.startswith('tools/') and arg.endswith(('.py', '.sh')):
                need(local(root, arg).is_file(), f'missing validator script: {arg}')
    observed = [normalize_argv(c['argv']) for c in commands]
    need(all(argv in observed for argv in allowed if argv not in [
        ['python3', 'tools/validate_stage1_blueprint.py'],
        ['python3', 'tools/validate_stage1_blueprint.py', '--blueprint', BLUEPRINT, '--evidence-root', EVIDENCE]]),
        'missing prescribed validation command')
    if 'G-STAGE' in validators:
        need(any(c['argv'][1:2] == ['tools/validate_stage1_blueprint.py'] for c in commands), 'missing G-STAGE execution')
    need(expected_targets <= seen, f'missing prescribed test targets: {sorted(expected_targets-seen)}')


def check_receipt(root: Path, bp: Blueprint, key: str, evidence: str, sel: dict,
                  sizes: dict, role: str) -> None:
    receipt = read_json(local(root, f'{evidence}/receipts/{key}.{role}.json'))
    item = bp.items[key]
    expected = dict(schema_version='stage1-receipt/v1', item_id=key, role=role,
                    run_id=sel['run_id'], requirement_digest=bp.requirement,
                    baseline_snapshot_sha256=sel['baseline_snapshot_sha256'])
    for field, value in expected.items():
        need(receipt.get(field) == value, f'{key}: receipt {field} mismatch')
    need(receipt.get('complete') is True, f'{key}: incomplete receipt')
    need(bool(receipt.get('reviewer')) and bool(receipt.get('attempt_id')), f'{key}: missing reviewer/attempt')
    need(re.fullmatch(r'[0-9a-f]{40}', receipt.get('integrated_revision', '')), f'{key}: missing integrated revision')
    if role == 'master':
        check_integrated_revision(root, receipt['integrated_revision'])
    need(receipt.get('validators') == item.validators, f'{key}: stale validator obligation')
    records = receipt.get('artifacts')
    need(isinstance(records, list) and records, f'{key}: missing actual artifacts')
    refs = {}
    for record in records:
        need(record['path'] not in refs, f'{key}: duplicate artifact')
        refs[record['path']] = artifact(root, record)
    if key in bp.files:
        file = bp.files[key]
        need(receipt.get('source_path') == file.path and receipt.get('source_hash') == file.sha256,
             f'{key}: source identity mismatch')
        check_ranges(receipt.get('read_ranges'), sizes[key])
        need(file.artifact in refs, f'{key}: missing unique per-file report')
        report = refs[file.artifact].decode()
        need(file.path in report and file.sha256 in report, f'{key}: report lacks source path/hash')
    elif key in bp.folders:
        scope, folder, report = bp.folders[key]
        need(receipt.get('scope') == scope and receipt.get('folder_path') == folder and report in refs,
             f'{key}: wrong/missing directory report')
        need(receipt.get('children') == list(item.depends), f'{key}: directory child evidence mismatch')
        need(all(bp.items[d].state == '[x]' for d in item.depends), f'{key}: directory children unaccepted')
    else:
        command_evidence(root, receipt.get('commands'), item.validators)
        actual_tree = integration_inputs(root, item)
        need(receipt.get('integrated_tree_sha256') == digest(canonical(actual_tree)), f'{key}: integrated worktree hash mismatch')
        if 'G-CODE' in item.validators:
            cases = receipt.get('scenarios', {})
            for case in ('positive', 'negative', 'cancel', 'restart'):
                value = cases.get(case, {})
                need(isinstance(value, dict) and bool(value.get('expected')) and bool(value.get('actual')),
                     f'{key}: missing {case} criterion')
                need(value.get('evidence') in refs or (case in ('cancel', 'restart') and bool(value.get('not_applicable_reason'))),
                     f'{key}: missing {case} evidence')
            artifact(root, receipt.get('binary'))
            need(bool(receipt.get('resource_peaks')) and bool(receipt.get('rollback')), f'{key}: missing budget/rollback evidence')
    if role == 'master':
        review = receipt.get('manual_review', {})
        need(isinstance(review, dict) and review.get('decision') == 'accepted'
             and review.get('reviewer') == receipt['reviewer'] and bool(review.get('findings')),
             f'{key}: missing independent master semantic review')
        artifact(root, review.get('evidence'))
        need(all(bp.items[d].state == '[x]' for d in item.depends), f'{key}: unaccepted dependency')


def validate(root: Path = ROOT, blueprint: str = BLUEPRINT, evidence: str = EVIDENCE,
             source_root: Path | None = None, item: str | None = None, strict: bool = False,
             capacity: int = 3, reference_roots: dict | None = None) -> dict:
    report = dict(ok=False, structural={'ok': False}, semantic_manual={'verified_by_checker': False,
                  'note': 'Artifact structure is machine checked; semantic truth requires independent master review.'}, errors=[])
    try:
        root = root.resolve()
        bp = parse(local(root, blueprint).read_text(), evidence)
        active = local(root, SELECTOR).exists()
        sel = selector(root, bp, blueprint) if active else None
        need(not (item or strict or any(i.state != '[ ]' for i in bp.items.values())) or active,
             'strict/item/progressed validation requires active selector')
        sizes = file_hashes(bp, root, source_root or Path(bp.header['source_repo']), target=not active, reference_roots=reference_roots)
        expected = scaffold(bp, sizes, evidence)
        if active or any(local(root, rel).exists() for rel in expected):
            check_scaffold(root, expected)
        claims_path = f'{evidence}/claims.json'
        claims = read_json(local(root, claims_path)).get('claims', []) if local(root, claims_path).exists() else []
        need(not claims or active, 'claims require active selector')
        frontier = frontiers(bp, claims, sel['run_id'] if sel else '', capacity)
        if not active:
            frontier['worker_claim_frontier'] = []
            frontier['provisional_claims'] = []
            frontier['saturation'] = 0
        report['structural'] = {'ok': True}
        report['semantic_manual']['receipt_structure_ok'] = False
        selected = set(bp.items) if strict else ({item} if item else set())
        need(selected <= set(bp.items), f'unknown item: {item}')
        checked = []
        for key, row in bp.items.items():
            if row.state == '[x]' or key in selected:
                check_receipt(root, bp, key, evidence, sel, sizes, 'master')
                checked.append(key)
            elif row.state == '[_]':
                check_receipt(root, bp, key, evidence, sel, sizes, 'worker')
        if bp.items.get('ZS1-199') and bp.items['ZS1-199'].state == '[x]':
            need(all(i.state == '[x]' for i in bp.items.values()), 'final item lacks whole-stage closure')
        # Extra final artifacts cannot silently inflate coverage.
        if active:
            final = {f.artifact for f in bp.files.values()} | {a for _, _, a in bp.folders.values()}
            for path in local(root, evidence).rglob('*_learn.md'):
                need(path.relative_to(root).as_posix() in final, f'extra final report: {path}')
        report.update(ok=True, structural={'ok': True}, requirement_digest=bp.requirement,
                      snapshot_sha256=bp.snapshot, items=len(bp.items), files=len(bp.files),
                      folders=len(bp.folders), scope_counts={scope: {
                          'files': sum(f.scope == scope for f in bp.files.values()),
                          'folders': sum(s == scope for s, _, _ in bp.folders.values())}
                          for scope in bp.scopes}, frontier=frontier, bootstrap_required=not active, master_receipts_checked=checked,
                      counts={name: sum(i.state == state for i in bp.items.values()) for name, state in
                              zip(('not_done', 'worker_self_tested', 'master_accepted'), STATES)})
        report['semantic_manual']['receipt_structure_ok'] = True
        report['semantic_manual']['master_review_receipts'] = checked
    except (BlueprintError, OSError, ValueError, TypeError, KeyError) as exc:
        report['errors'].append(str(exc))
    return report


def write_new(root: Path, rel: str, data: bytes) -> None:
    path = local(root, rel)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as out:
        out.write(data)
        out.flush()
        os.fsync(out.fileno())


def bootstrap(root: Path, blueprint: str, evidence: str, source_root: Path | None,
              actor: str, run_id: str, reference_roots: dict | None = None) -> dict:
    need(actor == 'master' and re.fullmatch(r'[A-Za-z0-9_.-]+', run_id or ''), 'bootstrap requires master and run-id')
    bp = parse(local(root, blueprint).read_text(), evidence)
    need(all(item.state == '[ ]' for item in bp.items.values()), 'bootstrap cannot inherit existing states')
    need(not local(root, SELECTOR).exists(), 'selector already exists; bootstrap refuses overwrite')
    execution_dir = local(root, 'Docs/execution')
    if execution_dir.exists():
        for path in execution_dir.glob('*.json'):
            value = read_json(path)
            need(not (value.get('active') is True and ('blueprint' in value or 'requirement_digest' in value)),
                 'multiple active requirements at bootstrap')
    sizes = file_hashes(bp, root, source_root or Path(bp.header['source_repo']), reference_roots=reference_roots)
    expected = scaffold(bp, sizes, evidence)
    outputs = {rel: tsv(fields, rows) for rel, (fields, rows) in expected.items()}
    frontier = frontiers(bp, [], run_id, 3)
    outputs[f'{evidence}/todos_{dt.date.today():%Y%m%d}.md'] = todo(bp, frontier, blueprint, f'{evidence}/claims.json').encode()
    outputs[f'{evidence}/claims.json'] = canonical({'claims': []}) + b'\n'
    # The full dirty baseline is recorded separately from the requirement digest.
    baseline = {file.item_id: dict(path=file.path, scope=file.scope, sha256=file.sha256, bytes=sizes[file.item_id])
                for file in bp.files.values()}
    outputs[SELECTOR] = canonical(dict(schema_version='stage1-selector/v1', active=True,
        blueprint=blueprint, blueprint_version=bp.header['blueprint_version'], run_id=run_id,
        requirement_digest=bp.requirement, snapshot_sha256=bp.snapshot,
        baseline_snapshot_sha256=digest(canonical(baseline)), baseline_files=baseline,
        target_baseline='current-worktree-with-user-changes', activated_by='master')) + b'\n'
    for rel in outputs:
        need(not local(root, rel).exists(), f'bootstrap refuses existing output: {rel}')
    # All inputs/preconditions checked before writes; selector commits activation last.
    for rel, data in outputs.items():
        write_new(root, rel, data)
    return {'ok': True, 'bootstrap': 'master', 'created_paths': list(outputs), 'requirement_digest': bp.requirement,
            'note': 'All checklist states remain [ ]; no semantic receipts generated.'}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    parser.add_argument('--blueprint', default=BLUEPRINT)
    parser.add_argument('--evidence-root', default=EVIDENCE)
    parser.add_argument('--source-root', type=Path)
    parser.add_argument('--reference-root', action='append', default=[], metavar='NAME=PATH')
    parser.add_argument('--item')
    parser.add_argument('--strict', action='store_true')
    parser.add_argument('--json', action='store_true')
    parser.add_argument('--bootstrap', action='store_true')
    parser.add_argument('--actor', choices=['master', 'worker'])
    parser.add_argument('--run-id')
    parser.add_argument('--capacity', type=int, default=3)
    args = parser.parse_args(argv)
    try:
        reference_roots = {}
        for entry in args.reference_root:
            name, sep, path = entry.partition('=')
            need(sep and re.fullmatch(r'[A-Za-z0-9_-]+', name) and path and name not in reference_roots, 'invalid reference-root NAME=PATH')
            reference_roots[name] = Path(path)
        if args.bootstrap:
            need(not args.item and not args.strict, 'bootstrap cannot combine with item/strict acceptance')
            report = bootstrap(args.root.resolve(), args.blueprint, args.evidence_root, args.source_root, args.actor, args.run_id, reference_roots)
        else:
            report = validate(args.root, args.blueprint, args.evidence_root, args.source_root, args.item, args.strict, args.capacity, reference_roots)
    except (BlueprintError, OSError, ValueError, TypeError, KeyError) as exc:
        report = {'ok': False, 'errors': [str(exc)]}
    print(json.dumps(report, indent=2, ensure_ascii=False) if args.json else
          ('PASS: ' + json.dumps(report, ensure_ascii=False) if report['ok'] else '\n'.join('ERROR: ' + e for e in report['errors'])))
    return 0 if report['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
