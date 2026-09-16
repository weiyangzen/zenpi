# -*- coding: utf-8 -*-
from pathlib import Path
import json, hashlib, sys
root = Path('/Users/wangweiyang/GitHub/zenpi')
d = Path(__file__).resolve().parent
w = d/'worker'
src = Path('/Users/wangweiyang/GitHub/pi-mono')
folder = src/'packages/coding-agent/src/core/extensions'
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
items = json.loads((w/'directory-listing.json').read_text())
assert sorted(p.name for p in folder.iterdir()) == [i['name'] for i in items]
for i in items:
    assert (folder/i['name']).is_file() and not (folder/i['name']).is_symlink()
    assert sha(folder/i['name']) == i['sha256']
contexts = []
for r in json.loads((w/'context-index.json').read_text()):
    full = Path(r['full_snapshot'])
    parts = full.parts
    p = folder/full.name if parts[1] == 'source' else (src if parts[1] == 'pi' else root)/Path(*parts[2:])
    b = p.read_bytes()
    assert hashlib.sha256(b).hexdigest() == r['file_sha256']
    lo, hi = r['byte_range']
    assert b[lo:hi] == (w/r['fragment']).read_bytes()
    contexts.append(dict(id=r['id'], path=str(p), file_sha256=r['file_sha256'], lines=r['lines'], byte_range=[lo,hi], fragment_sha256=r['sha256'], review='reuse exact full source independently accepted022/023' if int(r['id'][1:]) <= 6 else 'independent bounded connection read this turn'))
sys.path.insert(0, str(root/'tools'))
import validate_stage1_blueprint as v
bp = v.parse((root/v.BLUEPRINT).read_text())
sel = v.selector(root,bp,v.BLUEPRINT)
sizes = v.file_hashes(bp,root,src,target=False)
assert bp.items['ZS1-052'].state == '[ ]'
assert not (root/bp.folders['ZS1-052'][2]).exists()
children = []
for key in ['ZS1-022','ZS1-023']:
    for role in ['master','worker']:
        v.check_receipt(root,bp,key,v.EVIDENCE,sel,sizes,role)
    receipt = root/v.EVIDENCE/'receipts'/f'{key}.master.json'
    assert receipt.read_bytes() == (w/'children'/key[-3:]/'master-receipt.json').read_bytes()
    canonical = root/bp.files[key].artifact
    assert canonical.read_bytes() == (w/'children'/key[-3:]/'canonical.md').read_bytes()
    children.append(dict(item_id=key, receipt=str(receipt.relative_to(root)), receipt_sha256=sha(receipt), canonical=str(canonical.relative_to(root)), canonical_sha256=sha(canonical)))
(d/'current-context.json').write_text(json.dumps(contexts,indent=2)+'\n')
(d/'current-directory.json').write_text(json.dumps(dict(path=str(folder),entries=items),indent=2)+'\n')
(d/'child-continuity.json').write_text(json.dumps(children,indent=2)+'\n')
(d/'semantic-current.json').write_text(json.dumps(dict(passed=True,scope='independent directory understanding only',formal_children=['ZS1-022','ZS1-023'],context_only_direct_files=['index.ts','loader.ts','wrapper.ts'],new_context_reads=17,prior_full_source_reuse=['ZS1-022','ZS1-023'],historical_combination_probe='54 lines/9 helper scenarios read, no rerun/no AgentSession or loader execution',new_product_executions=0),indent=2)+'\n')
(d/'final-report.md').write_bytes((w/'report.md').read_bytes()+(d/'append.md').read_bytes())
print(json.dumps(dict(contexts=len(contexts),children=children,final_report_sha256=sha(d/'final-report.md'))))
