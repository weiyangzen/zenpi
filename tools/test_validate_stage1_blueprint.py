#!/usr/bin/env python3
"""Behavioral tests of Stage 1 gates; temporary fixtures never assert semantic acceptance."""
from __future__ import annotations

import copy
from dataclasses import replace
import datetime as dt
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

import validate_stage1_blueprint as v


class Stage1Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        # The fixture begins at bootstrap regardless of live acceptance state.
        # Mutating the real checklist is never a test prerequisite.
        cls.blueprint_text = re.sub(
            r'^(\s*-\s*)\[[ _x]\](\s+\*\*ZS1-[0-9]{3}\*\*)',
            r'\1[ ]\2', (v.ROOT / v.BLUEPRINT).read_text(), flags=re.M)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / 'zenpi'
        self.source = Path(self.temp.name) / 'pi-mono'
        self.root.mkdir()
        self.source.mkdir()
        self.references = {'codex': Path(self.temp.name) / 'codex'}
        self.references['codex'].mkdir()
        self.text = self.blueprint_text
        parsed = v.parse(self.text)
        for namespace, source in [('source', self.source), ('reference:codex', self.references['codex'])]:
            self.git(source, 'init', '-q')
            self.git(source, 'commit', '--allow-empty', '-qm', namespace + ' fixture')
            self.text = self.text.replace(parsed.source_revisions[namespace], v.repository_head(source))
        for file in parsed.files.values():
            data = (file.path + '\n').encode()
            if file.item_id in ('ZS1-084', 'ZS1-300'):
                data = b'a' * (v.LIMIT + 17)
            input_root = self.source if file.scope == 'source' else (
                self.root if file.scope == 'target' else self.references[file.scope.split(':', 1)[1]])
            path = input_root / file.path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            self.text = self.text.replace(file.sha256, v.digest(data))
            if file.declared_bytes is not None:
                self.text = re.sub(r'(^\| ' + file.item_id + r' \| [^|]+ \| )\d+( \|)',
                                   lambda m: m[1] + str(len(data)) + m[2], self.text, flags=re.M)
        self.write_blueprint()

    def git(self, root, *args):
        result = subprocess.run(['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                                 '-c', 'commit.gpgsign=false', *args], cwd=root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def write_blueprint(self):
        path = self.root / v.BLUEPRINT
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(self.text)

    def parse(self):
        return v.parse(self.text)

    def validate(self, **kw):
        return v.validate(self.root, source_root=self.source, reference_roots=self.references, **kw)

    def boot(self):
        return v.bootstrap(self.root, v.BLUEPRINT, v.EVIDENCE, self.source, 'master', 'test-run', self.references)

    def write_json(self, rel, value):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(v.canonical(value) + b'\n')

    def refresh_states(self):
        bp = self.parse()
        sizes = v.file_hashes(bp, self.root, self.source, reference_roots=self.references)
        for rel, (fields, rows) in v.scaffold(bp, sizes, v.EVIDENCE).items():
            (self.root / rel).write_bytes(v.tsv(fields, rows))
        selector = v.read_json(self.root / v.SELECTOR)
        selector['snapshot_sha256'] = bp.snapshot
        self.write_json(v.SELECTOR, selector)

    def assertInvalid(self, report, contains):
        self.assertFalse(report['ok'], report)
        self.assertIn(contains, ' '.join(report['errors']))

    def test_current_repository_draft_and_counts_are_derived(self):
        report = self.validate()
        self.assertTrue(report['ok'], report)
        self.assertEqual(report['files'], len(self.parse().files))
        self.assertEqual(report['folders'], len(self.parse().folders))
        self.assertEqual(report['frontier']['worker_claim_frontier'], [])
        self.assertFalse(report['semantic_manual']['verified_by_checker'])

    def test_duplicate_id_rejected(self):
        row = next(x for x in self.text.splitlines() if x.startswith('- [ ] **ZS1-010**'))
        self.text += '\n' + row + '\n'
        with self.assertRaisesRegex(v.BlueprintError, 'duplicate ID'):
            self.parse()

    def test_bad_states_and_ids_rejected(self):
        for mark in ('[X]', '[✓]', '[done]', '[]'):
            with self.subTest(mark=mark):
                with self.assertRaisesRegex(v.BlueprintError, 'invalid checkbox'):
                    v.parse(self.text.replace('- [ ] **ZS1-010**', '- ' + mark + ' **ZS1-010**'))
        with self.assertRaises(v.BlueprintError):
            v.parse(self.text.replace('**ZS1-010**', '**OLD-010**'))

    def test_missing_file_row_cannot_be_hidden_by_partition(self):
        self.text = '\n'.join(x for x in self.text.splitlines() if not x.startswith('| ZS1-010 /'))
        with self.assertRaisesRegex(v.BlueprintError, 'file closure'):
            self.parse()

    def test_missing_file_checklist_rejected(self):
        self.text = '\n'.join(x for x in self.text.splitlines() if not x.startswith('- [ ] **ZS1-010**'))
        with self.assertRaises(v.BlueprintError):
            self.parse()

    def test_missing_directory_even_with_dependency_rewired_rejected(self):
        self.text = '\n'.join(x for x in self.text.splitlines() if not x.startswith('- [ ] **ZS1-064**'))
        self.text = self.text.replace('Depends: ZS1-064 |', 'Depends: ZS1-061,ZS1-062,ZS1-063 |')
        with self.assertRaisesRegex(v.BlueprintError, 'directory closure'):
            self.parse()

    def test_skip_immediate_child_directory_rejected(self):
        self.text = self.text.replace('Depends: ZS1-064 |', 'Depends: ZS1-061,ZS1-062,ZS1-063 |')
        with self.assertRaisesRegex(v.BlueprintError, 'immediate-child'):
            self.parse()

    def test_cycle_and_unknown_dependency_rejected(self):
        for deps, error in [('ZS1-101', 'cycle'), ('ZS1-999', 'missing dependency')]:
            with self.subTest(deps=deps):
                changed = self.text.replace('Depends: ZS1-057,ZS1-058,ZS1-091 |', 'Depends: ' + deps + ' |')
                with self.assertRaisesRegex(v.BlueprintError, error):
                    v.parse(changed)

    def test_duplicate_field_loc_limit_and_path_traversal(self):
        for before, after in [('Estimated LOC: 1200', 'Estimated LOC: 5000'),
                              ('Estimated LOC: 1200', 'Estimated LOC: 1.2'),
                              ('Estimated LOC: 1200', 'Estimated LOC: 1 | Estimated LOC: 1'),
                              ('`tools/validate_stage1_blueprint.py`', '`../escape.py`'),
                              ('`tools/validate_stage1_blueprint.py`', '`/tmp/escape.py`')]:
            with self.subTest(after=after):
                with self.assertRaises(v.BlueprintError):
                    v.parse(self.text.replace(before, after))

    def test_cross_tree_report_substitution_rejected(self):
        self.text = self.text.replace(v.EVIDENCE + '/targets/zenpi/files/src/core.rs_learn.md',
                                      v.EVIDENCE + '/files/src/core.rs_learn.md')
        with self.assertRaisesRegex(v.BlueprintError, 'namespace|file scope/checklist mismatch'):
            self.parse()

    def test_reference_tree_counts_and_indexes_remain_independent(self):
        report = self.validate()
        self.assertEqual(report['scope_counts']['reference:codex'], {'files': 10, 'folders': 5})
        self.assertEqual(report['scope_counts']['source']['files'], 21)
        self.assertEqual(report['scope_counts']['target']['files'], 27)
        self.boot()
        ref = self.root / v.EVIDENCE / 'references/codex/source_manifest.tsv'
        source = self.root / v.EVIDENCE / 'source_manifest.tsv'
        self.assertNotIn('ZS1-300', source.read_text())
        ref.write_bytes(source.read_bytes())
        self.assertInvalid(self.validate(), 'references/codex/source_manifest.tsv')

    def test_missing_reference_file_and_root_directory_rejected(self):
        for prefix in ('| ZS1-300 |', '- [ ] **ZS1-350**'):
            text = '\n'.join(line for line in self.text.splitlines() if not line.startswith(prefix))
            if '350' in prefix:
                text = text.replace('ZS1-350', 'ZS1-351')
            with self.assertRaises(v.BlueprintError):
                v.parse(text)

    def test_reference_hash_and_chunk_tail_are_independent(self):
        file = self.parse().files['ZS1-300']
        path = self.references['codex'] / file.path
        old = path.read_bytes()
        path.write_bytes(old + b'wrong')
        self.assertInvalid(self.validate(), 'reference:codex hash mismatch')
        path.write_bytes(old)
        self.boot()
        chunk = self.root / v.EVIDENCE / 'chunk_manifest.tsv'
        lines = chunk.read_text().splitlines(keepends=True)
        last = max(index for index, line in enumerate(lines) if line.startswith('ZS1-300\t'))
        chunk.write_text(''.join(lines[:last] + lines[last+1:]))
        self.assertInvalid(self.validate(), 'chunk_manifest.tsv')

    def test_activation_header_changes_preserve_requirement_digest(self):
        original = v.requirement_digest(self.text)
        text = self.text.replace('authoritative: true', 'authoritative: false').replace(
            'status: bootstrap-active', 'status: proposed')
        self.assertEqual(original, v.requirement_digest(text))
        self.assertNotEqual(v.digest(self.text.encode()), v.digest(text.encode()))

    def test_requirement_stable_for_all_cursor_transitions(self):
        original = v.requirement_digest(self.text)
        for mark in ('[_]', '[x]'):
            changed = self.text.replace('- [ ] **', '- ' + mark + ' **')
            self.assertEqual(original, v.requirement_digest(changed))
            self.assertNotEqual(v.digest(self.text.encode()), v.digest(changed.encode()))

    def test_real_requirement_changes_invalidate_digest(self):
        for before, after in [('Estimated LOC: 1200', 'Estimated LOC: 1201'),
                              ('工具运行时排入 follow-up 不打断该工具', '工具运行时排入 follow-up 会打断该工具'),
                              ('tools/test_validate_stage1_blueprint.py', 'tools/test_other.py'),
                              ('blueprint_version: ' + self.parse().header['blueprint_version'], 'blueprint_version: changed')]:
            self.assertIn(before, self.text)
            self.assertNotEqual(v.requirement_digest(self.text), v.requirement_digest(self.text.replace(before, after)))

    def test_runtime_record_exclusion_is_narrow_and_fail_closed(self):
        block = '\n<!-- stage1-runtime:begin -->\n{"receipt_refs":["Docs/a.json"]}\n<!-- stage1-runtime:end -->\n'
        # Separator newline is requirement text; compare identical separator.
        self.assertEqual(v.requirement_digest(self.text + '\n'), v.requirement_digest(self.text + block))
        for bad in (block.replace('receipt_refs', 'requirements'), block.replace('<!-- stage1-runtime:end -->', ''),
                    block.replace('Docs/a.json', '../escape.json')):
            with self.assertRaises((v.BlueprintError, ValueError)):
                v.requirement_digest(self.text + bad)

    def test_wrong_source_hash_and_target_dirty_baseline_hash(self):
        for scope, path in ((self.source, 'packages/agent/src/agent.ts'), (self.root, 'src/core.rs')):
            old = (scope / path).read_bytes()
            (scope / path).write_bytes(old + b'changed')
            self.assertInvalid(self.validate(), 'hash mismatch')
            (scope / path).write_bytes(old)

    def test_symlink_is_rejected(self):
        path = self.root / 'src/core.rs'
        data = path.read_bytes()
        path.unlink()
        target = Path(self.temp.name) / 'outside'
        target.write_bytes(data)
        path.symlink_to(target)
        self.assertInvalid(self.validate(), 'escapes root')

    def test_bootstrap_explicit_master_without_selector(self):
        with self.assertRaisesRegex(v.BlueprintError, 'requires master'):
            v.bootstrap(self.root, v.BLUEPRINT, v.EVIDENCE, self.source, 'worker', 'test')
        self.assertTrue(self.boot()['ok'])
        self.assertTrue(self.validate()['ok'])
        self.assertFalse((self.root / v.EVIDENCE / 'receipts').exists())
        self.assertEqual(list((self.root / v.EVIDENCE).rglob('*_learn.md')), [])
        with self.assertRaisesRegex(v.BlueprintError, 'already exists'):
            self.boot()

    def test_bootstrap_does_not_inherit_old_accepted_state(self):
        self.text = self.text.replace('- [ ] **ZS1-010**', '- [x] **ZS1-010**')
        self.write_blueprint()
        with self.assertRaisesRegex(v.BlueprintError, 'cannot inherit'):
            self.boot()
        self.assertFalse((self.root / v.SELECTOR).exists())

    def test_manifest_missing_row_directory_or_duplicate_rejected(self):
        self.boot()
        for name in ('source_manifest.tsv', 'folder_learn_index.tsv', 'file_learn_index.tsv'):
            path = self.root / v.EVIDENCE / name
            raw = path.read_text()
            lines = raw.splitlines(keepends=True)
            for mutated in (''.join(lines[:-1]), raw + lines[-1]):
                path.write_text(mutated)
                self.assertInvalid(self.validate(), 'coverage/hash/state mismatch')
            path.write_text(raw)

    def test_wrong_manifest_hash_rejected(self):
        self.boot()
        path = self.root / v.EVIDENCE / 'source_manifest.tsv'
        raw = path.read_text()
        sha = next(iter(self.parse().files.values())).sha256
        path.write_text(raw.replace(sha, '0'*64))
        self.assertInvalid(self.validate(), 'coverage/hash/state mismatch')

    def test_selector_cannot_replace_baseline_or_requirement(self):
        self.boot()
        original = v.read_json(self.root / v.SELECTOR)
        for key in ('requirement_digest', 'snapshot_sha256', 'baseline_snapshot_sha256'):
            changed = copy.deepcopy(original)
            changed[key] = '0'*64
            self.write_json(v.SELECTOR, changed)
            self.assertInvalid(self.validate(), 'mismatch')
        changed = copy.deepcopy(original)
        changed['baseline_files']['ZS1-070']['sha256'] = '0'*64
        changed['baseline_snapshot_sha256'] = v.digest(v.canonical(changed['baseline_files']))
        self.write_json(v.SELECTOR, changed)
        self.assertInvalid(self.validate(), 'baseline mismatch')

    def test_multiple_active_requirement_rejected(self):
        self.boot()
        self.write_json('Docs/execution/other.json', {'active': True, 'blueprint': 'Docs/other.md'})
        self.assertInvalid(self.validate(), 'multiple active')

    def test_bootstrap_rejects_second_authority_before_any_write(self):
        self.write_json('Docs/execution/other.json', {'active': True, 'blueprint': 'Docs/other.md'})
        with self.assertRaisesRegex(v.BlueprintError, 'multiple active'):
            self.boot()
        self.assertFalse((self.root / v.EVIDENCE).exists())

    def test_forged_master_checkbox_rejected_even_after_snapshot_refresh(self):
        self.boot()
        self.text = self.text.replace('- [ ] **ZS1-001**', '- [x] **ZS1-001**')
        self.write_blueprint()
        self.refresh_states()
        self.assertInvalid(self.validate(), 'ZS1-001.master.json')
        self.write_json(v.EVIDENCE + '/receipts/ZS1-001.master.json', {'role': 'master', 'acceptance_passed': True})
        self.assertInvalid(self.validate(), 'receipt schema_version mismatch')

    def test_item_evidence_requires_selector_and_actual_receipt(self):
        self.assertInvalid(self.validate(item='ZS1-010'), 'requires active selector')
        self.boot()
        self.assertInvalid(self.validate(item='ZS1-010'), 'ZS1-010.master.json')

    def test_wrong_integrated_revision_is_rejected(self):
        self.boot()
        selector = v.read_json(self.root / v.SELECTOR)
        # Deliberately forged record: rejection test only, never accepted evidence.
        receipt = dict(schema_version='stage1-receipt/v1', item_id='ZS1-010', role='master',
                       run_id='test-run', requirement_digest=self.parse().requirement,
                       baseline_snapshot_sha256=selector['baseline_snapshot_sha256'],
                       complete=True, reviewer='attacker', attempt_id='forged', integrated_revision='0'*40)
        self.write_json(v.EVIDENCE + '/receipts/ZS1-010.master.json', receipt)
        self.assertInvalid(self.validate(item='ZS1-010'), 'integrated revision is not an ancestor')

    def test_source_and_reference_actual_head_are_checked_even_when_bytes_match(self):
        for namespace, root in [('source', self.source), ('reference:codex', self.references['codex'])]:
            previous = v.repository_head(root)
            self.git(root, 'commit', '--allow-empty', '-qm', 'unapproved source revision')
            self.assertInvalid(self.validate(), namespace + ': frozen repository HEAD mismatch')
            self.git(root, 'checkout', '--detach', previous)

    def test_historical_integrated_revision_survives_unrelated_checkpoint(self):
        self.git(self.root, 'init', '-q')
        self.git(self.root, 'commit', '--allow-empty', '-qm', 'accepted integration')
        accepted = v.repository_head(self.root)
        self.git(self.root, 'commit', '--allow-empty', '-qm', 'later unrelated checkpoint')
        self.assertNotEqual(accepted, v.repository_head(self.root))
        v.check_integrated_revision(self.root, accepted)

    def test_nonancestor_and_missing_integrated_revisions_are_rejected(self):
        self.git(self.root, 'init', '-q')
        self.git(self.root, 'commit', '--allow-empty', '-qm', 'base')
        base = v.repository_head(self.root)
        self.git(self.root, 'checkout', '-qb', 'other')
        self.git(self.root, 'commit', '--allow-empty', '-qm', 'unintegrated worker output')
        unintegrated = v.repository_head(self.root)
        self.git(self.root, 'checkout', '--detach', base)
        for revision in (unintegrated, '0'*40):
            with self.assertRaisesRegex(v.BlueprintError, 'not an ancestor'):
                v.check_integrated_revision(self.root, revision)

    def test_integration_hash_covers_cargo_lock_and_owned_ci_bytes(self):
        item = replace(self.parse().items['ZS1-101'], owned_paths=('src/runtime.rs', '.github/workflows/ci.yml'))
        before = v.integration_inputs(self.root, item)
        self.assertIn('Cargo.toml', before)
        self.assertIn('Cargo.lock', before)
        for rel in ('Cargo.toml', 'Cargo.lock', '.github/workflows/ci.yml'):
            path = self.root / rel
            content = path.read_bytes()
            path.write_bytes(content + b'changed build inputs')
            self.assertNotEqual(v.digest(v.canonical(before)), v.digest(v.canonical(v.integration_inputs(self.root, item))))
            path.write_bytes(content)
        (self.root / 'README.md').write_text('Unrelated documentation checkpoint\n')
        self.assertEqual(before, v.integration_inputs(self.root, item))

    def test_extra_final_report_cannot_inflate_coverage(self):
        self.boot()
        path = self.root / v.EVIDENCE / 'extra_learn.md'
        path.write_text('Deliberately out-of-scope adversarial artifact.\n')
        self.assertInvalid(self.validate(), 'extra final report')

    def test_truncated_and_duplicate_json_records_rejected(self):
        path = self.root / 'receipt.json'
        for raw in (b'{"complete":true', b'{"complete":true}', b'{"complete":true,"complete":false}\n'):
            path.write_bytes(raw)
            with self.assertRaises((v.BlueprintError, ValueError)):
                v.read_json(path)

    def test_chunk_gap_overlap_and_truncated_tail_rejected(self):
        v.check_ranges([[0, v.LIMIT], [v.LIMIT, v.LIMIT+17]], v.LIMIT+17)
        for ranges in ([[0, v.LIMIT]], [[0, v.LIMIT], [v.LIMIT-1, v.LIMIT+17]],
                       [[0, v.LIMIT], [v.LIMIT+1, v.LIMIT+17]], [[0, v.LIMIT+17]]):
            with self.assertRaises(v.BlueprintError):
                v.check_ranges(ranges, v.LIMIT+17)
        self.boot()
        path = self.root / v.EVIDENCE / 'chunk_manifest.tsv'
        lines = path.read_text().splitlines(keepends=True)
        path.write_text(''.join(lines[:-1]))
        self.assertInvalid(self.validate(), 'coverage/hash/state mismatch')

    def test_claim_and_integration_are_separate(self):
        bp = self.parse()
        bp.items['ZS1-001'] = replace(bp.items['ZS1-001'], state='[x]')
        bp.items['ZS1-010'] = replace(bp.items['ZS1-010'], state='[_]')
        data = v.frontiers(bp, [], 'test', 3)
        self.assertIn('ZS1-010', data['integration_frontier'])
        self.assertNotIn('ZS1-010', data['worker_claim_frontier'])
        self.assertEqual(len(data['worker_claim_frontier']), 3)
        claim = dict(item_id='ZS1-010', run_id='test', requirement_digest=bp.requirement,
                     owned_paths=list(bp.items['ZS1-010'].owned_paths), runtime_status='finished', session='worker1')
        data = v.frontiers(bp, [claim], 'test', 3)
        self.assertEqual(data['live_workers'], 0)
        self.assertEqual(len(data['worker_claim_frontier']), 3)
        bp.items['ZS1-001'] = replace(bp.items['ZS1-001'], state='[ ]')
        self.assertIn('ZS1-010', v.frontiers(bp, [claim], 'test', 3)['integration_blocked'])

    def test_provisional_claims_do_not_close_unaccepted_dependencies(self):
        self.boot()
        report = self.validate()
        self.assertTrue(report['ok'], report)
        self.assertEqual(len(report['frontier']['worker_claim_frontier']), 3)
        self.assertEqual(report['frontier']['worker_claim_frontier'], report['frontier']['provisional_claims'])
        self.assertEqual(report['frontier']['integration_frontier'], [])
        with self.assertRaisesRegex(v.BlueprintError, 'operator worker cap'):
            v.frontiers(self.parse(), [], 'test-run', 4)

    def test_explicit_toolchain_alias_preserves_validation_argv(self):
        self.assertEqual(v.normalize_argv(['cargo', '+stable-aarch64-apple-darwin', 'test', '--locked']),
                         ['cargo', 'test', '--locked'])
        self.assertEqual(v.normalize_argv(['cargo', '+unknown', 'test']), ['cargo', '+unknown', 'test'])

    def test_claim_binding_ownership_and_capacity(self):
        bp = self.parse()
        claim = dict(item_id='ZS1-010', run_id='test', requirement_digest=bp.requirement,
                     owned_paths=list(bp.items['ZS1-010'].owned_paths), runtime_status='live', session='worker1')
        for key, value in [('requirement_digest', '0'*64), ('owned_paths', ['src/core.rs']), ('item_id', 'ZS1-001')]:
            with self.assertRaises(v.BlueprintError):
                v.frontiers(bp, [{**claim, key: value}], 'test', 3)
        with self.assertRaisesRegex(v.BlueprintError, 'capacity'):
            v.frontiers(bp, [claim], 'test', 0)

    def test_finished_claim_does_not_hide_another_live_path_conflict(self):
        bp = self.parse()
        bp.items['ZS1-001'] = replace(bp.items['ZS1-001'], state='[x]')
        bp.items['ZS1-010'] = replace(bp.items['ZS1-010'], state='[_]')
        bp.items['ZS1-011'] = replace(bp.items['ZS1-011'], owned_paths=bp.items['ZS1-010'].owned_paths)
        claims = [dict(item_id=key, run_id='test', requirement_digest=bp.requirement,
                       owned_paths=list(bp.items[key].owned_paths), runtime_status=status, session=key)
                  for key, status in [('ZS1-010', 'finished'), ('ZS1-011', 'live')]]
        frontier = v.frontiers(bp, claims, 'test', 3)
        self.assertIn('ZS1-010', frontier['integration_blocked'])

    def test_todo_preserves_three_states_and_unfinished_counts(self):
        bp = self.parse()
        bp.items['ZS1-001'] = replace(bp.items['ZS1-001'], state='[x]')
        bp.items['ZS1-010'] = replace(bp.items['ZS1-010'], state='[_]')
        text = v.todo(bp, v.frontiers(bp, [], 'test', 3), v.BLUEPRINT, v.EVIDENCE + '/claims.json')
        self.assertIn('worker_self_tested=1', text)
        self.assertIn('master_accepted=1', text)
        self.assertIn('Unfinished=' + str(len(bp.items)-1), text)
        self.assertIn('- [_] **ZS1-010**', text)
        self.assertIn('Claim ledger:', text)

    def run_actual_probe(self):
        script = self.root / 'tools/test_probe.py'
        script.parent.mkdir(exist_ok=True)
        script.write_text('import unittest\nclass Probe(unittest.TestCase):\n def test_io(self):\n  self.assertEqual(bytes.fromhex("6162"), b"ab")\nunittest.main()\n')
        argv = ['python3', 'tools/test_probe.py']
        start = dt.datetime.now(dt.timezone.utc).isoformat()
        result = subprocess.run(argv, cwd=self.root, capture_output=True, check=False)
        end = dt.datetime.now(dt.timezone.utc).isoformat()
        def record(name, data):
            (self.root / name).write_bytes(data)
            return dict(path=name, bytes=len(data), sha256=v.digest(data))
        return dict(argv=argv, cwd='.', env_names=[], exit_code=result.returncode, started_at=start, ended_at=end,
                    tests_run=1, stdout=record('stdout.log', result.stdout), stderr=record('stderr.log', result.stderr))

    def test_real_subprocess_test_output_can_be_checked(self):
        command = self.run_actual_probe()
        v.command_evidence(self.root, [command], 'python3 tools/test_probe.py')

    def test_zero_tests_missing_command_wrong_hash_and_forged_argv(self):
        command = self.run_actual_probe()
        for changed in ({**command, 'tests_run': 0}, {**command, 'exit_code': 1},
                        {**command, 'argv': ['python3', '-c', 'print("ok")']},
                        {**command, 'stdout': {**command['stdout'], 'sha256': '0'*64}}):
            with self.assertRaises(v.BlueprintError):
                v.command_evidence(self.root, [changed], 'python3 tools/test_probe.py')
        with self.assertRaisesRegex(v.BlueprintError, 'missing command'):
            v.command_evidence(self.root, [], 'python3 tools/test_probe.py')

    def test_zero_tests_in_one_target_not_hidden_by_other_target(self):
        command = self.run_actual_probe()
        # Deliberately fabricated runner text exercises the rejection gate only.
        stdout = b'test result: ok. 1 passed;\ntest result: ok. 0 passed;\n'
        (self.root / 'stdout.log').write_bytes(stdout)
        (self.root / 'stderr.log').write_bytes(b'')
        command.update(argv=['cargo', 'test', '--locked', '--test', 'a', '--test', 'b'],
                       stdout=dict(path='stdout.log', bytes=len(stdout), sha256=v.digest(stdout)),
                       stderr=dict(path='stderr.log', bytes=0, sha256=v.digest(b'')))
        with self.assertRaisesRegex(v.BlueprintError, 'zero/missing per-target'):
            v.command_evidence(self.root, [command], 'cargo test --locked --test a --test b')

    def test_cli_draft_and_strict_exit_codes(self):
        argv = [sys.executable, str(v.ROOT / 'tools/validate_stage1_blueprint.py'), '--root', str(self.root),
                '--source-root', str(self.source), '--reference-root', 'codex=' + str(self.references['codex']), '--blueprint', v.BLUEPRINT, '--evidence-root', v.EVIDENCE, '--json']
        result = subprocess.run(argv, capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        result = subprocess.run(argv + ['--item', 'ZS1-010'], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertFalse(json.loads(result.stdout)['ok'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
