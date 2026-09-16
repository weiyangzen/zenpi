import { it, expect, afterAll } from 'vitest';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { createBashTool, createBashToolDefinition, createLocalBashOperations, createLocalShellOperations } from './source/packages/coding-agent/src/core/tools/bash.ts';

const base=fileURLToPath(new URL('.',import.meta.url));
const artifactRoot=process.env.SOURCE025_ARTIFACTS||join(base,'artifacts');mkdirSync(artifactRoot,{recursive:true});const root=mkdtempSync(join(artifactRoot,'fs-'));
const records:any[]=[];const ownedPids=new Set<number>();
const pause=(ms:number)=>new Promise(r=>setTimeout(r,ms));
const q=(s:string)=>"'"+s.replaceAll("'","'\\''")+"'";
const area=(name:string)=>{const p=join(root,name);mkdirSync(p);return p;};
const command=(mode:string,dir:string,arg='')=>`exec ${q(process.execPath)} ${q(join(base,'child-fixture.mjs'))} ${q(mode)} ${q(dir)} ${q(arg)}`;
const text=(r:any)=>r.content.filter((b:any)=>b.type==='text').map((b:any)=>b.text).join('\n');
const hash=(d:Buffer)=>createHash('sha256').update(d).digest('hex');
const alive=(pid:number)=>{try{process.kill(pid,0);return true;}catch(e:any){if(e.code==='ESRCH')return false;throw e;}};
async function until(fn:()=>boolean,ms=3000){const start=Date.now();while(!fn()){if(Date.now()-start>ms)throw Error('condition timeout');await pause(10);}}
function pids(dir:string){const value=JSON.parse(readFileSync(join(dir,'pids.json'),'utf8'));for(const n of Object.values(value))ownedPids.add(n as number);return value;}
async function stop(pid:number){if(alive(pid)){try{process.kill(pid,'SIGKILL');}catch{}}await until(()=>!alive(pid));}
afterAll(async()=>{for(const pid of ownedPids){if(alive(pid)){try{process.kill(pid,'SIGKILL');}catch{}}}writeFileSync(join(artifactRoot,'probe-observations.json'),JSON.stringify({node:process.versions.node,root,records},null,2)+'\n');});

it('actual Bash wrapper returns merged stdout/stderr, raw controls, empty output and nonzero status',async()=>{
  const dir=area('basic');const b=createBashTool(dir);const r=await b.execute('mixed',{command:command('mixed',dir)});expect(text(r)).toContain('OUT\n');expect(text(r)).toContain('ERR\n');expect(text(r)).toContain('\u0000\u001b[31m');expect(r.details).toBeUndefined();
  expect(text(await b.execute('empty',{command:':'}))).toBe('(no output)');
  await expect(b.execute('fail',{command:"printf 'before-failure'; exit 7"})).rejects.toThrow('before-failure\n\nCommand exited with code 7');
  expect((b as any).promptGuidelines).toHaveLength(1);records.push({case:'basic',mixed:text(r),nonzero:7,mergedWithoutStreamIdentity:true});
});

it('real environment: stale PI fields removed, context cwd/model injected, prefix and spawn hook apply',async()=>{
  const dir=area('env');const ctxDir=area('ctx-dir');const hookDir=area('hook-dir');const keys=['PI_SESSION_ID','PI_SESSION_FILE','PI_PROVIDER','PI_MODEL','PI_REASONING_LEVEL'];const previous=Object.fromEntries(keys.map(k=>[k,process.env[k]]));for(const k of keys)process.env[k]='STALE';
  try{
    const ordinary=JSON.parse(text(await createBashTool(dir).execute('env',{command:command('env',dir)})));expect(keys.every(k=>ordinary.env[k]===null)).toBe(true);
    const ctx:any={cwd:ctxDir,model:{provider:'fixture-provider',id:'fixture-model'},thinkingLevel:'high',sessionManager:{getSessionId:()=> 'fixture-session',getSessionFile:()=>'/fixture/session.jsonl'}};
    const tool=createBashToolDefinition(dir,{commandPrefix:'export PROBE_PREFIX=prefix'});const contextual=JSON.parse(text(await tool.execute('ctx',{command:command('env',dir)},undefined,undefined,ctx)));expect(contextual.cwd).toBe(ctxDir);expect(contextual.env).toMatchObject({PI_SESSION_ID:'fixture-session',PI_PROVIDER:'fixture-provider',PI_MODEL:'fixture-model',PI_REASONING_LEVEL:'high',PROBE_PREFIX:'prefix'});
    const hook=createBashToolDefinition(dir,{exposeSessionEnvironment:false,spawnHook:c=>({...c,cwd:hookDir,env:{...c.env,PROBE_HOOK:'hook'},command:command('env',dir)})});const hooked=JSON.parse(text(await hook.execute('hook',{command:'ignored original command'},undefined,undefined,ctx)));expect(hooked.cwd).toBe(hookDir);expect(hooked.env.PROBE_HOOK).toBe('hook');expect(keys.every(k=>hooked.env[k]===null)).toBe(true);expect(hook.promptGuidelines).toBeUndefined();
    records.push({case:'environment',ordinary,contextual,hooked,injection:'ExtensionContext session/model metadata and spawnHook only; real process/env/cwd'});
  }finally{for(const k of keys){if(previous[k]===undefined)delete process.env[k];else process.env[k]=previous[k];}}
});

it('invalid timeout, pre-abort, nonexistent cwd and shell reject before command side effects',async()=>{
  const dir=area('preflight');const b=createBashTool(dir);const invalid=[0,-1,NaN,Infinity,2147483.648];const errors=[];
  for(const timeout of invalid){let message='';try{await b.execute('invalid',{command:command('mark',dir),timeout});}catch(e:any){message=e.message;}expect(message).toContain('Invalid timeout');errors.push({timeout:String(timeout),message});}
  const c=new AbortController();c.abort();await expect(b.execute('pre-abort',{command:command('mark',dir)},c.signal)).rejects.toThrow('Command aborted');expect(existsSync(join(dir,'effect'))).toBe(false);
  await expect(createBashTool(join(dir,'missing')).execute('cwd',{command:command('mark',dir)})).rejects.toThrow('Working directory does not exist');
  await expect(createBashTool(dir,{shellPath:join(dir,'missing-shell')}).execute('shell',{command:command('mark',dir)})).rejects.toThrow('Custom shell path not found');
  const ops=createLocalShellOperations('fixture',()=>({shell:join(dir,'absent-executable'),args:['-c']}));await expect(ops.exec('true',dir,{onData:()=>{}})).rejects.toThrow(/ENOENT/);
  records.push({case:'preflight',errors,zeroSideEffects:true,resolverInjection:'absent executable for actual spawn error'});
});

it('actual stdin command transport and exported local BashOperations execute in real shell',async()=>{
  const dir=area('stdin');const buffers:Buffer[]=[];
  const ops=createLocalShellOperations('stdin-bash',()=>({shell:'/bin/bash',args:['-s'],commandTransport:'stdin'}));const r=await ops.exec("printf 'stdin-transport'",dir,{onData:b=>buffers.push(b)});expect(r.exitCode).toBe(0);expect(Buffer.concat(buffers).toString()).toBe('stdin-transport');
  const b:Buffer[]=[];expect((await createLocalBashOperations({shellPath:'/bin/bash'}).exec("printf 'local-ops'",dir,{onData:x=>b.push(x)})).exitCode).toBe(0);expect(Buffer.concat(b).toString()).toBe('local-ops');records.push({case:'stdin',result:r,transport:'real /bin/bash -s, injected resolver; no Windows WSL claim'});
});

it('line cap spills exact full output and final close is readable at resolution',async()=>{
  const dir=area('lines');const b=createBashTool(dir);const boundary=await b.execute('boundary',{command:command('lines',dir,'2000')});expect(boundary.details).toBeUndefined();
  const result:any=await b.execute('lines',{command:command('lines',dir,'2100')});const raw=readFileSync(result.details.fullOutputPath);const expected=Buffer.from(Array.from({length:2100},(_,i)=>`line-${i}\n`).join(''));expect(raw.equals(expected)).toBe(true);expect(result.details.truncation).toMatchObject({truncated:true,truncatedBy:'lines',totalLines:2100,outputLines:2000});expect(text(result)).toContain('Showing lines 101-2100 of 2100');expect(text(result)).not.toMatch(/^line-0\n/);
  records.push({case:'line cap',file:result.details.fullOutputPath,bytes:raw.length,sha256:hash(raw),truncation:result.details.truncation});
});

it('actual split UTF8, oversized single line and invalid raw bytes preserve distinct display/raw semantics',async()=>{
  const dir=area('bytes');const b=createBashTool(dir);const small=await b.execute('small-utf8',{command:command('utf8',dir,'1')});expect(text(small)).toBe('界🌍');
  const large:any=await b.execute('large-utf8',{command:command('utf8',dir,'22000')});const raw=readFileSync(large.details.fullOutputPath);expect(raw.equals(Buffer.from('界🌍'.repeat(22000)))).toBe(true);expect(large.details.truncation.lastLinePartial).toBe(true);expect(large.details.truncation.outputBytes).toBeLessThanOrEqual(51200);expect(text(large)).not.toContain('\ufffd');expect(text(large)).toContain('of line 1');
  const interleaved=await b.execute('interleaved-utf8',{command:command('interleaved-utf8',dir)});expect(text(interleaved)).toContain('\ufffd');expect(text(interleaved)).toContain('E');
  const invalid:any=await b.execute('invalid-utf8',{command:command('invalid',dir)});const invalidRaw=readFileSync(invalid.details.fullOutputPath);expect(invalidRaw.equals(Buffer.concat([Buffer.alloc(55000,120),Buffer.from([255,254,226,130])]))).toBe(true);expect(text(invalid)).toContain('\ufffd');expect(invalid.details.truncation.totalBytes).toBeGreaterThan(invalidRaw.length);
  records.push({case:'byte cap',interleavedStreamText:text(interleaved),singleDecoderAcrossBothStreams:true,valid:{file:large.details.fullOutputPath,bytes:raw.length,sha256:hash(raw),truncation:large.details.truncation},invalid:{file:invalid.details.fullOutputPath,rawBytes:invalidRaw.length,decodedBytes:invalid.details.truncation.totalBytes,sha256:hash(invalidRaw)}});
});

it('live real-process updates are throttled, can expose a not-yet-final file, and stop after resolution',async()=>{
  const dir=area('stream');const updates:any[]=[];let released=false;
  const result:any=await createBashTool(dir).execute('stream',{command:command('stream',dir)},undefined,(update:any)=>{
    const t=update.content.map((b:any)=>b.text||'').join('');const f=update.details?.fullOutputPath;updates.push({at:Date.now(),length:t.length,tail:t.slice(-80),fullOutputPath:f,bytesAtCallback:f&&existsSync(f)?statSync(f).size:null});
    if(t.includes('WAITING_RELEASE')){released=true;writeFileSync(join(dir,'release'),'go');}
  });
  expect(released).toBe(true);expect(updates[0].length).toBe(0);expect(updates.length).toBeLessThan(40);expect(text(result)).toContain('FINAL_AFTER_RELEASE');const n=updates.length;await pause(200);expect(updates).toHaveLength(n);const raw=readFileSync(result.details.fullOutputPath);expect(raw.toString()).toContain('FINAL_AFTER_RELEASE');expect(updates.some(u=>u.fullOutputPath&&u.tail.includes('WAITING_RELEASE'))).toBe(true);
  records.push({case:'stream',updates,finalFile:result.details.fullOutputPath,finalBytes:raw.length,sha256:hash(raw),afterResolutionUpdateCount:0});
});

it('actual timeout kills process group, reaps direct process and preserves completed prefix file',async()=>{
  const dir=area('timeout');const start=Date.now();let error='';try{await createBashTool(dir).execute('timeout',{command:command('tree',dir),timeout:0.7});}catch(e:any){error=e.message;}
  expect(error).toContain('Command timed out after 0.7 seconds');expect(error).toContain('TREE_READY');const ps=pids(dir);await until(()=>!alive(ps.parent)&&!alive(ps.grandchild));
  const f=error.match(/Full output: ([^\]\n]+)/)?.[1];expect(f).toBeDefined();const raw=readFileSync(f!);expect(raw.toString()).toContain('TREE_READY');const h=readFileSync(join(dir,'heartbeat'),'utf8');await pause(80);expect(readFileSync(join(dir,'heartbeat'),'utf8')).toBe(h);
  records.push({case:'timeout',duration:Date.now()-start,pids:ps,pidsAbsentAfter:true,file:f,bytes:raw.length,sha256:hash(raw),suffix:error.slice(-240),completeness:'captured prefix closed, command killed; no full-command-completion claim'});
});

it('actual AbortController kills process group, flushes prefix and emits no later output updates',async()=>{
  const dir=area('abort');const c=new AbortController();const updates:any[]=[];let error='';try{await createBashTool(dir).execute('abort',{command:command('tree',dir)},c.signal,(u:any)=>{const t=u.content.map((b:any)=>b.text||'').join('');updates.push({length:t.length,tail:t.slice(-50)});if(t.includes('TREE_READY'))c.abort();});}catch(e:any){error=e.message;}
  expect(error).toContain('Command aborted');const ps=pids(dir);await until(()=>!alive(ps.parent)&&!alive(ps.grandchild));const n=updates.length;await pause(150);expect(updates).toHaveLength(n);const f=error.match(/Full output: ([^\]\n]+)/)?.[1];expect(readFileSync(f!).toString()).toContain('TREE_READY');records.push({case:'abort',pids:ps,pidsAbsentAfter:true,updates,file:f,suffix:error.slice(-220),signalAborted:c.signal.aborted});
});

it('external signal without timeout/abort produces null exit code accepted as success',async()=>{
  const dir=area('signal');const r=await createBashTool(dir).execute('signal',{command:command('signal',dir)});expect(text(r)).toBe('BEFORE_SIGNAL\n');records.push({case:'external signal',returnedSuccess:true,text:text(r),signal:'child self SIGTERM; signal not surfaced by waitForChildProcess return type'});
});

it('active inherited descendant output remains readable past direct-child exit grace',async()=>{
  const dir=area('active-descendant');const start=Date.now();const r=await createBashTool(dir).execute('active',{command:command('inherit',dir,'active')});const ps=pids(dir);expect(text(r)).toContain('DESC_FINAL');expect(text(r)).toContain('TICK7');await until(()=>!alive(ps.parent)&&!alive(ps.grandchild));records.push({case:'active descendant',pids:ps,duration:Date.now()-start,text:text(r),allFixturePidsAbsent:true});
});

it('quiet inherited detached descendant survives successful shell return and needs fixture cleanup',async()=>{
  const dir=area('quiet-descendant');const start=Date.now();const r=await createBashTool(dir).execute('quiet',{command:command('inherit',dir,'quiet')});const ps=pids(dir);expect(text(r)).toContain('PARENT_EXIT');expect(alive(ps.parent)).toBe(false);expect(alive(ps.grandchild)).toBe(true);const duration=Date.now()-start;await stop(ps.grandchild);records.push({case:'quiet descendant',pids:ps,duration,text:text(r),descendantAliveAtToolReturn:true,fixtureCleanup:'SIGKILL known owned PID then observed ESRCH',notGlobalDescendantReapGuarantee:true});
});
