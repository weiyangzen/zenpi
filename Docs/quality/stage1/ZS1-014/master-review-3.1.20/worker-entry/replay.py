#!/usr/bin/env python3
"""One isolated replay reusing local pinned dependencies; never writes original packet/runtime."""
import argparse,json,os,shutil,subprocess,sys
sys.dont_write_bytecode=True
from pathlib import Path
from verify import verify,sha,CACHE
p=argparse.ArgumentParser();p.add_argument('--packet',type=Path,required=True);p.add_argument('--runtime',type=Path,required=True);p.add_argument('--node',type=Path,required=True);p.add_argument('--destination',type=Path,required=True);a=p.parse_args();here=Path(__file__).resolve().parent
expected=json.loads((here/'integrity.json').read_text());before=verify(a.packet,a.runtime);assert before['runtime']==expected['runtime'];assert sha(a.node)=='27db838bb204ef7c21df2931f5656e4c8fb32e6e947f363a402b49714d32b5b1'
cache=a.runtime/CACHE;cache_before=sha(cache) if cache.exists() else None
assert not a.destination.exists(),'Destination must be new';shutil.copytree(a.packet,a.destination,symlinks=True);shutil.copytree(a.runtime,a.destination/'runtime/node_modules',symlinks=True)
env=dict(os.environ,NODE_BIN=str(a.node.resolve()));subprocess.run(['sh',str((a.destination/'run.sh').resolve())],env=env,check=True)
result=json.loads((a.destination/'replay-results.json').read_text());assert result['numPassedTests']==23 and result['numFailedTests']==result['numPendingTests']==result['numTodoTests']==0
assert verify(a.packet,a.runtime)==before,'Original packet/runtime changed';assert (sha(cache) if cache.exists() else None)==cache_before,'Original generated cache changed';verify(a.destination)
print(json.dumps({'destination':str(a.destination.resolve()),'passed':23,'original_packet_and_runtime_unchanged':True},indent=2))
