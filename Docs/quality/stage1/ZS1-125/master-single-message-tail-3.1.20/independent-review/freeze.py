import hashlib, json, os, pathlib, shutil, stat
root=pathlib.Path(__file__).resolve().parent
def digest(p):
    b=p.read_bytes(); return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def inventory(path):
    out={}
    for directory, dirs, files in os.walk(path,followlinks=False):
        dirs[:]=[d for d in dirs if not pathlib.Path(directory,d).is_symlink()]
        for name in files:
            p=pathlib.Path(directory,name)
            if stat.S_ISREG(p.lstat().st_mode): out[str(p.relative_to(path))]=digest(p)
    return out
before=json.loads((root/'preservation-before.json').read_text())
for name, expected in before['ready'].items(): assert inventory(pathlib.Path(name))==expected, name
for name, expected in before['tracked'].items(): assert digest(pathlib.Path(name))==expected, name
original=pathlib.Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-125-render-tail-ready')
assert inventory(original)==before['B_package']
assert digest(root/'context/src/render.rs')['sha256']=='b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d'
result={'passed':True,'old_ready_packages':len(before['ready']),'old_ready_regular_files':sum(map(len,before['ready'].values())),'tracked_regular_files':len(before['tracked']),'B_regular_files':len(before['B_package']),'method':'exact relative regular-file sets, byte lengths and SHA-256; no FIFO opened','baseline':digest(root/'preservation-before.json')}
(root/'preservation-result.json').write_text(json.dumps(result,indent=2)+'\n')
ready=root/'ready'; ready.mkdir()
for folder in ['input-package','context','commands']:
    for rel in inventory(root/folder):
        dst=ready/folder/rel; dst.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(root/folder/rel,dst)
for name in ['review.md','source-review.json','input-identity.json','vendor-inventory.json','preservation-before.json','preservation-result.json','setup.py','freeze.py']:
    shutil.copyfile(root/name,ready/name)
shutil.copyfile(root/'verify-review.py',ready/'verify.py')
shutil.copyfile(root.parent/'run_candidate_probe.py',ready/'run_candidate_probe.py')
manifest={'item':'ZS1-125','kind':'independent renderer review','requirement_digest':'8d525b351d066ce9b0487a485337647d47233275782e4f4d154423a623fe1645','reviewed_B_manifest_sha256':'822b201ac8e8f3f5bb4172599ac42c807addbba69c13362d24c26576d36f9581','reviewed_render_sha256':'b239971a91e3b9c05400e36b4ca9da3eebfb64a0566d36c26dd08340b33a3b5d','review_complete':True,'whole_product_acceptance':False,'product_changes':[],'independent_tests_passed':8,'existing_tests_rerun_passed':27,'pty_runs':0,'http_runs':0,'network_runs':0,'artifacts':inventory(ready)}
(ready/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'ready':str(ready),'manifest':digest(ready/'manifest.json'),'artifacts':len(manifest['artifacts']),'preservation':result}))
