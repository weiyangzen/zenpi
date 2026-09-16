from pathlib import Path
import datetime,hashlib,json,os,subprocess
D=Path(__file__).resolve().parent;R=D.parents[2]
def meta(p):
 b=p.read_bytes();return dict(bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
def audit():
 for name,m in json.loads((D/'main-inputs.json').read_text()).items():assert meta(R/name)==m,name
def run(name,argv,cwd):
 audit();assert not (D/(name+'.run.json')).exists();start=datetime.datetime.now(datetime.timezone.utc).isoformat();env=os.environ.copy();env['CARGO_TARGET_DIR']=str(R/'target');env['CARGO_BUILD_JOBS']='2';env['PYTHONDONTWRITEBYTECODE']='1'
 with (D/(name+'.stdout.log')).open('xb') as out,(D/(name+'.stderr.log')).open('xb') as err:r=subprocess.run(argv,cwd=cwd,env=env,stdout=out,stderr=err,timeout=600)
 rec=dict(argv=argv,cwd=str(cwd),started_at=start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),exit_code=r.returncode,stdout=meta(D/(name+'.stdout.log')),stderr=meta(D/(name+'.stderr.log')))
 (D/(name+'.run.json')).write_text(json.dumps(rec,indent=2)+'\n');audit();print(name,r.returncode,flush=True);return r.returncode
assert run('build',['cargo','+stable-aarch64-apple-darwin','build','--offline','--manifest-path',str(D/'Cargo.toml')],D)==0
binary=R/'target/debug/zenpi-status125-observation';(D/'binary.json').write_text(json.dumps(meta(binary),indent=2)+'\n')
assert run('before-observation',[str(binary)],D)==1
result=json.loads((D/'before-observation.stdout.log').read_text());assert result['failed']==3,result
print('Three public-state timer defects observed; fourth pending-approval control passed.',flush=True)
