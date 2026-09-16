import hashlib, json, os, pathlib, shutil, stat, subprocess
ROOT = pathlib.Path(__file__).resolve().parent
REPO = ROOT.parent.parent
def digest(p):
    b=p.read_bytes(); return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def inventory(root):
    result={}
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = [d for d in dirs if not pathlib.Path(directory,d).is_symlink()]
        for name in files:
            p=pathlib.Path(directory,name)
            if stat.S_ISREG(p.lstat().st_mode): result[str(p.relative_to(root))]=digest(p)
    return result
pkg=ROOT/'input-package'
manifest=json.loads((pkg/'manifest.json').read_text())
assert digest(pkg/'manifest.json')['sha256']=='822b201ac8e8f3f5bb4172599ac42c807addbba69c13362d24c26576d36f9581'
for rel, expected in manifest['artifacts'].items(): assert digest(pkg/rel)==expected, rel
protected={}
for p in REPO.joinpath('.ops').iterdir():
    if p.is_dir() and p.name.endswith('-ready'): protected[str(p)]=inventory(p)
tracked={}
for raw in subprocess.check_output(['git','ls-files','-z'],cwd=REPO).split(b'\0'):
    if raw:
        p=REPO/os.fsdecode(raw)
        if p.exists() and stat.S_ISREG(p.lstat().st_mode): tracked[str(p)]=digest(p)
original=pathlib.Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-125-render-tail-ready')
(ROOT/'preservation-before.json').write_text(json.dumps({'ready':protected,'tracked':tracked,'B_package':inventory(original)},indent=2)+'\n')
context=ROOT/'context'
harness=context/'.ops/work/harness'
harness.mkdir(parents=True)
(context/'src').mkdir()
shutil.copyfile(pkg/'after/src/render.rs',context/'src/render.rs')
for name in ['Cargo.toml','Cargo.lock','lib.rs']: shutil.copyfile(pkg/'executed-harness'/name,harness/name)
shutil.copyfile(pkg/'evidence/context-render_markdown.rs',harness.parent/'context-render_markdown.rs')
vendor=pathlib.Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/vendor/crossterm')
assert digest(vendor/'Cargo.toml')==manifest['artifacts']['context/vendor-Cargo.toml']
vendor_files=inventory(vendor)
for rel in vendor_files:
    dest=context/'vendor/crossterm'/rel; dest.parent.mkdir(parents=True,exist_ok=True); shutil.copyfile(vendor/rel,dest)
assert inventory(context/'vendor/crossterm')==vendor_files
(ROOT/'vendor-inventory.json').write_text(json.dumps(vendor_files,indent=2)+'\n')
with (harness/'Cargo.toml').open('a') as f: f.write('\n[[test]]\nname = "independent_review"\npath = "independent_review.rs"\n')
(ROOT/'commands').mkdir()
(ROOT/'source-review.json').write_text(json.dumps({'source':digest(context/'src/render.rs'),'lines':1706,'continuous_read_ranges_from_prior_context':[[1,300],[301,600],[601,900],[901,1200],[1201,1500],[1501,1706]],'scope':'single renderer module, unchanged B after; independent integration tests; pinned B lock and dependencies; no TUI wiring claim','ready_packages':len(protected),'ready_files':sum(map(len,protected.values())),'tracked_regular_files':len(tracked)},indent=2)+'\n')
print(json.dumps({'verified_B_artifacts':len(manifest['artifacts']),'preserved_ready_baseline':len(protected),'vendor_files':len(vendor_files),'harness':str(harness)}))
