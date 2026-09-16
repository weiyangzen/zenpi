# -*- coding: utf-8 -*-
"""Publish reviewed diagnostic evidence only; never launch archived products."""
from pathlib import Path
import datetime,hashlib,json,shutil,subprocess,tarfile
R=Path(__file__).resolve().parents[2]
D=R/'.ops/stage1_execution/fivephase-master-3.1.21';D.mkdir(exist_ok=False)
W=Path('/Users/wangweiyang/.codex/worktrees/2267/zenpi/.ops/zs1-117-fivephase-ready')
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
assert meta(W/'manifest.json')['sha256']=='77c1a00a10025cc1cd6c31fa169f04d7d66f2770da08c59eee08d21e368f2aba'
shutil.copytree(W,D/'worker');W=D/'worker';E=W/'evidence'
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
result=subprocess.run(['python3','-B',str(W/'verify_offline.py')],capture_output=True,timeout=60,cwd=R)
(D/'offline.stdout.json').write_bytes(result.stdout);(D/'offline.stderr.log').write_bytes(result.stderr)
(D/'offline.run.json').write_text(json.dumps(dict(argv=['python3','-B',str(W/'verify_offline.py')],cwd=str(R),started_at=started,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=result.returncode,stdout=meta(D/'offline.stdout.json'),stderr=meta(D/'offline.stderr.log')),indent=2)+'\n')
assert result.returncode==0
raw=(E/'trace.bin').read_bytes();assert len(raw)==160
ticks={chr(raw[i+8]):int.from_bytes(raw[i+20:i+28],'little') for i in range(0,160,32)}
obs=json.loads((E/'observation.json').read_text());factor=obs['timebase']['numer']/obs['timebase']['denom']/1e6
actual=dict(pre_main_ms=(ticks['M']-obs['marks']['before_popen']['mach_ticks'])*factor,inside_main_ms=(ticks['X']-ticks['M'])*factor)
assert 935.70<actual['pre_main_ms']<935.72 and 40.39<actual['inside_main_ms']<40.40
(D/'independent-intervals.json').write_text(json.dumps(actual,indent=2)+'\n')
review='''# ZS1-117 — independent five-phase diagnosis review

The controller read the complete diagnostic report, final156-line observer, complete main/core/headless/emitter diff, raw phase records and interval tables, and complete offline verifier. The copied348-artifact sealed package passed its28 offline integrity checks; the controller separately decoded the160 raw bytes and recomputed parent→main935.707583ms and main→return40.391917ms from the shared mach_absolute_time source. Both the producer and observer call the same API and use the recorded125/3 timebase. The five records identify target97022 and wrapper97021. The one shutdown response and reconnect terminal match, all streams end, exit0 and reaping are recorded. No archived executable, build command or startup observer was rerun.

This new19d5887b diagnostic artifact demonstrates predominant pre-main delay for that observed run only. M is the first call inside Rust main, not kernel exec/dyld entry. Parent→M includes wrapper, loader, scheduler and policy activity; the data do not isolate their contributions or establish the cause of the historical1109.425125ms failure. The observer differs from the official communicate path and adds five nonblocking pipe writes, so977.101334ms outer duration is not an official budget sample.

The13-event system log has a923.836083ms interval within its own clock domain. Its raw clock epoch differs from the parent/child clock; the later ABI offset sample cannot backdate a stable mapping. The displayed scan begins18.285ms before parent launch. Preserve this discrepancy: no cross-clock subtraction, overlay, strict inclusion or causal attribution. The explicit Skipping up front XProtect scan entry precludes labeling the full interval antivirus work. Identical linker identifier does not identify identical binaries; this new artifact's SHA and CDHash differ. Matching path/token is association only.

Old failure, immutable prior package, initial offline ValueError, diagnostic source scope and concurrent build observations are retained. Core/headless fragments are contextual reading, not new full-file acceptance; the original private source predates current headless fixes. Every old raw artifact remains linked through the package manifest. This publication accepts the diagnostic interpretation only, not117,084, any folder, any runtime gate or the full stage. It does not change thresholds, samples, timing boundaries or retry the old artifact. A newly changed product may receive a separate ordinary release gate, whose result must remain distinct.
'''
(D/'review.md').write_text(review)
P=R/'Docs/quality/stage1/ZS1-117/master-fivephase-3.1.21';P.mkdir(parents=True,exist_ok=False)
for name in ['review.md','offline.stdout.json','offline.stderr.log','offline.run.json','independent-intervals.json']:
 shutil.copy2(D/name,P/name)
shutil.copy2(W/'manifest.json',P/'worker-manifest.json')
shutil.copy2(W/'diagnosis.md',P/'worker-diagnosis.md')
shutil.copy2(Path(__file__),P/'publish.py')
with tarfile.open(P/'worker-ready.tar.gz','x:gz') as tar:
 for p in sorted(W.rglob('*')):
  if p.is_file():tar.add(p,arcname=str(p.relative_to(W)),recursive=False)
manifest={str(p.relative_to(P)):meta(p) for p in P.rglob('*') if p.is_file()}
(P/'manifest.json').write_text(json.dumps(dict(artifacts=manifest,complete=False,scope='independently reviewed diagnostic evidence only'),indent=2)+'\n')
print(json.dumps(dict(public=str(P),artifacts=len(manifest),offline_exit=result.returncode,intervals=actual)))
