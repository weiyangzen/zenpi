import { it, expect, afterAll } from 'vitest';
import { mkdirSync, mkdtempSync, writeFileSync, readFileSync, existsSync, renameSync, chmodSync, rmSync, symlinkSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createFindTool, createFindToolDefinition, relativizeFindResultPath } from './source/packages/coding-agent/src/core/tools/find.ts';
import { getToolPath } from './source/packages/coding-agent/src/utils/tools-manager.ts';

const base=fileURLToPath(new URL('.',import.meta.url));const root=mkdtempSync('/tmp/zenpi-source027-');const records:any[]=[];
const artifactRoot=process.env.SOURCE027_ARTIFACTS||join(base,'artifacts');mkdirSync(artifactRoot,{recursive:true});
const area=(name:string)=>{const p=join(root,name);mkdirSync(p);return p;};
const file=(p:string,text='')=>{mkdirSync(dirname(p),{recursive:true});writeFileSync(p,text);};
const text=(r:any)=>r.content.map((b:any)=>b.text||'').join('\n');
const lines=(r:any)=>text(r).split('\n').filter((l:string)=>l&&!l.startsWith('[')).sort();
const pause=(ms:number)=>new Promise(r=>setTimeout(r,ms));
async function until(fn:()=>boolean,ms=3000){const start=Date.now();while(!fn()){if(Date.now()-start>ms)throw Error('condition timeout');await pause(5);}}
const alive=(pid:number)=>{try{process.kill(pid,0);return true;}catch(e:any){if(e.code==='ESRCH')return false;throw e;}};
const q=(s:string)=>"'"+s.replaceAll("'","'\\''")+"'";
const native=getToolPath('fd')!;
afterAll(()=>writeFileSync(join(artifactRoot,'probe-observations.json'),JSON.stringify({node:process.versions.node,root,nativeFd:native,records},null,2)+'\n'));

it('actual fd honors nonrepo hierarchical ignore, hidden matches and nested repository boundaries',async()=>{
  const dir=area('ignore');file(join(dir,'.gitignore'),'ignored.txt\n');file(join(dir,'ignored.txt'));file(join(dir,'kept.txt'));file(join(dir,'.hidden.txt'));file(join(dir,'node_modules','visible.txt'));
  const nonrepo=await createFindTool(dir).execute('nonrepo',{pattern:'**/*.txt'});expect(lines(nonrepo)).toEqual(['.hidden.txt','kept.txt','node_modules/visible.txt']);
  const repo=area('nested-repo');mkdirSync(join(repo,'.git'));file(join(repo,'.gitignore'),'ignored.txt\n');file(join(repo,'ignored.txt'));file(join(repo,'kept.txt'));mkdirSync(join(repo,'nested','.git'),{recursive:true});file(join(repo,'nested','ignored.txt'));file(join(repo,'nested','.gitignore'),'nested-only.txt\n');file(join(repo,'nested','nested-only.txt'));
  const nested=await createFindTool(repo).execute('nested',{pattern:'**/*.txt'});expect(lines(nested)).toContain('nested/ignored.txt');expect(lines(nested)).not.toContain('ignored.txt');expect(lines(nested)).not.toContain('nested/nested-only.txt');records.push({case:'actual ignore',nonrepo:lines(nonrepo),nested:lines(nested),nodeModulesNotUnconditionallyExcluded:true});
});

it('actual fd applies relative/absolute path globs, context cwd and explicit sibling search root',async()=>{
  const dir=area('paths');const project=join(dir,'project');const sibling=join(dir,'sibling');file(join(project,'src','a.spec.ts'));file(join(project,'src','sub','b.spec.ts'));file(join(sibling,'outside.txt'));
  const def=createFindToolDefinition(dir);const ctx:any={cwd:project};const result=await def.execute('ctx',{pattern:'src/**/*.spec.ts'},undefined,undefined,ctx);expect(lines(result)).toEqual(['src/a.spec.ts','src/sub/b.spec.ts']);
  const absolute=await def.execute('absolute',{pattern:join(project,'src','**','*.spec.ts'),path:project});expect(lines(absolute)).toEqual(lines(result));
  const outside=await def.execute('sibling',{pattern:'*',path:'../sibling'},undefined,undefined,ctx);expect(lines(outside)).toEqual(['outside.txt']);
  await expect(def.execute('missing',{pattern:'*',path:join(dir,'absent')})).rejects.toThrow();records.push({case:'actual path',relative:lines(result),absolute:lines(absolute),explicitSibling:lines(outside),workspaceConfinementNotEnforced:true});
});

it('actual fd filename whitespace/newline and symlink output reveal line-protocol limitations',async()=>{
  const dir=area('names');file(join(dir,' lead.txt'));file(join(dir,'trail.txt '));file(join(dir,'first\nsecond'));file(join(dir,'unicode-界.txt'));const outside=area('link-target');file(join(outside,'secret.txt'));symlinkSync(outside,join(dir,'linked'),'dir');
  const r=await createFindTool(dir).execute('names',{pattern:'*'});const observed=lines(r);
  expect(observed).toContain(' lead.txt');expect(observed).toContain('trail.txt');expect(observed).not.toContain('trail.txt ');expect(observed).toContain('first');expect(observed).toContain('second');expect(observed).not.toContain('linked/secret.txt');expect(observed.some(p=>p==='linked'||p==='linked/')).toBe(true);expect(observed).toContain('unicode-界.txt');
  records.push({case:'actual filename protocol',observed,trailingFilenameSpaceLost:true,leadingBasenameSpaceRetained:true,newlineFilenameSplit:true,symlinkListedButNotFollowed:true});
});

it('actual fd limit flag is conservative at exact equality, zero is not a positive cap, byte truncation has no full path',async()=>{
  const dir=area('limits');file(join(dir,'one.txt'));const b=createFindTool(dir);const exact:any=await b.execute('one',{pattern:'*',limit:1});expect(exact.details.resultLimitReached).toBe(1);expect(text(exact)).toContain('Use limit=2');
  file(join(dir,'two.txt'));const zero:any=await b.execute('zero',{pattern:'*',limit:0});expect(lines(zero)).toHaveLength(2);expect(zero.details.resultLimitReached).toBe(0);
  for(const limit of [-1,1.5])await expect(b.execute('invalid-limit',{pattern:'*',limit})).rejects.toThrow();
  const large=area('byte-cap');for(let i=0;i<650;i++)file(join(large,`${String(i).padStart(4,'0')}-`+'x'.repeat(100)+'.txt'));
  const clipped:any=await createFindTool(large).execute('large',{pattern:'*.txt',limit:1000});expect(clipped.details.truncation.truncated).toBe(true);expect(clipped.details.truncation.totalLines).toBe(650);expect(clipped.details.truncation.outputBytes).toBeLessThanOrEqual(51200);expect(clipped.details.fullOutputPath).toBeUndefined();expect(clipped.details.resultLimitReached).toBeUndefined();
  records.push({case:'actual limits',exact:text(exact),zero:text(zero),truncation:clipped.details.truncation,displayBytes:Buffer.byteLength(text(clipped)),noSpillArtifact:true});
});

it('custom glob passes ignore/limit but trusts returned count and paths including escapes',async()=>{
  const dir=area('custom');const calls:any[]=[];const def=createFindToolDefinition(dir,{operations:{exists:p=>{calls.push({exists:p});return true;},glob:(pattern,cwd,options)=>{calls.push({pattern,cwd,options});return [join(dir,'a.txt'),join(dir,'b.txt'),join(dirname(dir),'outside.txt'),'folder/'];}}});
  const r:any=await def.execute('custom',{pattern:'*',limit:1});expect(lines(r)).toEqual(['../outside.txt','a.txt','b.txt','folder/']);expect(r.details.resultLimitReached).toBe(1);expect(calls[1].options).toEqual({ignore:['**/node_modules/**','**/.git/**'],limit:1});
  const empty=await createFindTool(dir,{operations:{exists:()=>true,glob:()=>[]}}).execute('empty',{pattern:'*'});expect(text(empty)).toBe('No files found matching pattern');await expect(createFindTool(dir,{operations:{exists:()=>false,glob:()=>{throw Error('must not run');}}}).execute('missing',{pattern:'*'})).rejects.toThrow('Path not found');
  expect(relativizeFindResultPath('/tmp/outside','/workspace')).toBe('../tmp/outside');records.push({case:'custom trust',calls,result:text(r),returnedMoreThanLimit:true,escapeNotRejected:true,injection:'FindOperations exists/glob fixture; no fd claim'});
});

it('custom async glob abort settles before operation completes and does not cancel underlying work',async()=>{
  const dir=area('custom-abort');const controller=new AbortController();let entered!:()=>void;const started=new Promise<void>(r=>entered=r);let release!:()=>void;const gate=new Promise<void>(r=>release=r);let finished=false;
  const b=createFindTool(dir,{operations:{exists:()=>true,glob:async()=>{entered();await gate;finished=true;return ['late.txt'];}}});const running=b.execute('abort',{pattern:'*'},controller.signal);await started;controller.abort();await expect(running).rejects.toThrow('Operation aborted');expect(finished).toBe(false);release();await until(()=>finished);
  let called=false;const already=new AbortController();already.abort();await expect(createFindTool(dir,{operations:{exists:()=>{called=true;return true;},glob:()=>[]}}).execute('preabort',{pattern:'*'},already.signal)).rejects.toThrow('Operation aborted');expect(called).toBe(false);records.push({case:'custom abort',rejectedBeforeOperationFinished:true,underlyingOperationCompletedLater:true,preabortNoExists:true});
});

it('default spawn fault fixtures expose exact argv, partial-error success, blank-error success and pre-reap cancellation',async()=>{
  const dir=area('fd-fault');const log=join(dir,'requests.jsonl');const ready=join(dir,'ready');const term=join(dir,'term');const backup=native+'.real-fd';const keys=['SOURCE027_FAULT','SOURCE027_FAULT_LOG','SOURCE027_READY','SOURCE027_TERM_SEEN'];const prev=Object.fromEntries(keys.map(k=>[k,process.env[k]]));let pid:number|undefined;
  renameSync(native,backup);writeFileSync(native,`#!/bin/sh\nexec ${q(process.execPath)} ${q(join(base,'fd-fault-fixture.mjs'))} "$@"\n`,{mode:0o755});process.env.SOURCE027_FAULT_LOG=log;process.env.SOURCE027_READY=ready;process.env.SOURCE027_TERM_SEEN=term;
  try{
    const b=createFindTool(dir);process.env.SOURCE027_FAULT='args';await b.execute('args',{pattern:'src/**/*.spec.ts',limit:3});const argv=JSON.parse(readFileSync(log,'utf8').trim()).args;expect(argv).toEqual(['--glob','--color=never','--hidden','--no-require-git','--max-results','3','--full-path','--','**/src/**/*.spec.ts',dir]);
    process.env.SOURCE027_FAULT='partial';const partial=await b.execute('partial',{pattern:'*'});expect(text(partial)).toBe('partial.txt');expect(partial.details).toBeUndefined();
    process.env.SOURCE027_FAULT='empty-error';await expect(b.execute('error',{pattern:'*'})).rejects.toThrow('injected empty failure');
    process.env.SOURCE027_FAULT='blank-error';const blank=await b.execute('blank',{pattern:'*'});expect(text(blank)).toBe('');
    process.env.SOURCE027_FAULT='cancel';const c=new AbortController();const running=b.execute('cancel',{pattern:'*'},c.signal);await until(()=>existsSync(ready));pid=Number(readFileSync(ready,'utf8'));c.abort();await expect(running).rejects.toThrow('Operation aborted');const aliveAtRejection=alive(pid);expect(aliveAtRejection).toBe(true);await until(()=>existsSync(term));await until(()=>!alive(pid!));
    records.push({case:'default process fault injection',injection:'only test-local fd executable temporarily replaced with controlled Node script; actual find/ensureTool/spawn/readline/error/abort logic unchanged',argv,partial:text(partial),blank:text(blank),pid,aliveAtRejection,observedTerm:true,eventualPidAbsent:true,log});
  }finally{
    if(pid&&alive(pid)){process.kill(pid,'SIGKILL');await until(()=>!alive(pid!));}rmSync(native,{force:true});renameSync(backup,native);for(const k of keys){if(prev[k]===undefined)delete process.env[k];else process.env[k]=prev[k];}
  }
});

it('default actual spawn rejects non-executable fd and offline missing-tool path without network',async()=>{
  const dir=area('unavailable');const backup=native+'.real-fd';const previousPath=process.env.PATH;const previousOffline=process.env.PI_OFFLINE;renameSync(native,backup);
  try{
    writeFileSync(native,'fixture nonexecutable',{mode:0o600});await expect(createFindTool(dir).execute('spawnfail',{pattern:'*'})).rejects.toThrow('Failed to run fd');rmSync(native);process.env.PATH='';process.env.PI_OFFLINE='1';await expect(createFindTool(dir).execute('missing',{pattern:'*'})).rejects.toThrow('fd is not available and could not be downloaded');records.push({case:'unavailable',realSpawnEACCES:true,offlineMissingToolRejected:true,networkDuringTests:false});
  }finally{rmSync(native,{force:true});renameSync(backup,native);if(previousPath===undefined)delete process.env.PATH;else process.env.PATH=previousPath;if(previousOffline===undefined)delete process.env.PI_OFFLINE;else process.env.PI_OFFLINE=previousOffline;}
});
