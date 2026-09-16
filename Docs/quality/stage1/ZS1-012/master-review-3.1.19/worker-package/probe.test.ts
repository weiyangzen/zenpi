import { it, expect, afterAll } from 'vitest';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { calculateContextTokens, estimateTokens, estimateContextTokens, shouldCompact, findCutPoint, findTurnStartIndex, prepareCompaction, compactWithRequest, generateSummaryWithRequest, createSummaryRequestOptions, completeSimpleWithRetries, serializeConversation } from './source/packages/agent/src/harness/compaction/compaction.ts';
import { prepareBranchEntries, collectEntriesForBranchSummary, generateBranchSummaryWithRequest } from './source/packages/agent/src/harness/compaction/branch-summarization.ts';
import { BACKGROUND_CONTEXT as bg, withAbortSignal, withTelemetryContext } from './source/packages/agent/src/harness/context.ts';
import { buildSessionContext } from './source/packages/agent/src/harness/session/context.ts';
import { createFileOps, computeFileLists } from './source/packages/agent/src/harness/compaction/utils.ts';
import { convertToLlm } from './source/packages/agent/src/harness/messages.ts';
import { getOrThrow } from './source/packages/agent/src/harness/types.ts';

const records:any[]=[];
const usage={input:2,output:3,cacheRead:4,cacheWrite:5,totalTokens:14,cost:{input:1,output:2,cacheRead:3,cacheWrite:4,total:10}};
const model:any={id:'fixture',provider:'fixture',api:'fixture',reasoning:true,maxTokens:1000,contextWindow:5000};
const settings={enabled:true,reserveTokens:100,keepRecentTokens:1};
const user=(text:string):any=>({role:'user',content:[{type:'text',text}],timestamp:1});
const answer=(text:string,extra:any={}):any=>({role:'assistant',content:[{type:'text',text}],api:'fixture',provider:'fixture',model:'fixture',usage,stopReason:'stop',timestamp:2,...extra});
const call=(id:string,name='read',path=`${id}.ts`):any=>({type:'toolCall',id,name,arguments:{path}});
const tool=(id:string,text='output',isError=false):any=>({role:'toolResult',toolCallId:id,toolName:'read',content:[{type:'text',text}],isError,timestamp:3});
function entries(messages:any[]):any[]{return messages.map((message,i)=>({type:'message',id:`m${i}`,parentId:i?`m${i-1}`:null,seq:i,timestamp:i,message}));}
const checkpoint=(tail:any[],extra:any={}):any=>({type:'compaction',id:'checkpoint',parentId:null,seq:0,timestamp:0,summary:'OLD GOAL; constraint keep API; PENDING unfinished migration',tokensBefore:900,retainedTail:tail,...extra});
function prep(extra:any={}):any{return {messagesToSummarize:[user('history')],turnPrefixMessages:[],retainedTail:[user('tail')],isSplitTurn:false,tokensBefore:100,fileOps:createFileOps(),settings,...extra};}
function requestQueue(responses:any[],events:any[]=[]):any {
  return async (ctx:any,options:any,context:any)=>{
    events.push({prompt:ctx.messages[0].content[0].text,systemPrompt:ctx.systemPrompt,maxTokens:options.maxTokens,reasoning:options.reasoning,cacheRetention:options.cacheRetention,sessionId:options.sessionId,aborted:options.signal?.aborted});
    expect(options.signal).toBe(context.abortSignal);const next=responses.shift();if(!next)throw Error('fixture response exhausted');return next;
  };
}
afterAll(()=>writeFileSync(process.env.SOURCE012_OBSERVATIONS||fileURLToPath(new URL('./probe-observations.json',import.meta.url)),JSON.stringify({node:process.versions.node,providerBoundary:'scripted SummaryRequest or Models.completeSimple response, never a real model/network',records},null,2)+'\n'));

it('whole-turn and split-turn budget sweep preserves well-formed parallel tool pairs in each partition',()=>{
  const messages=[user('first task'),answer('',{content:[call('A'),call('B')],stopReason:'toolUse'}),tool('B','b'.repeat(31)),tool('A','a'.repeat(47)),answer('',{content:[call('C')],stopReason:'toolUse'}),tool('C','c'.repeat(29)),answer('finished first turn'),user('second task'),answer('second answer')];
  const source=entries(messages);const before=JSON.stringify(source);let split=0,whole=0;
  const cuts=[];
  for(let budget=1;budget<=250;budget++){
    const p=getOrThrow(prepareCompaction(source,{...settings,keepRecentTokens:budget}))!;
    expect([...p.messagesToSummarize,...p.turnPrefixMessages,...p.retainedTail]).toEqual(messages);
    const partitions=[p.messagesToSummarize,p.turnPrefixMessages,p.retainedTail];
    for(const id of ['A','B','C']){
      const ci=partitions.findIndex(ms=>ms.some((m:any)=>m.role==='assistant'&&m.content.some((b:any)=>b.type==='toolCall'&&b.id===id)));
      const ri=partitions.findIndex(ms=>ms.some((m:any)=>m.role==='toolResult'&&m.toolCallId===id));expect(ri).toBe(ci);
    }
    expect(p.retainedTail[0]?.role).not.toBe('toolResult');if(p.isSplitTurn)split++;else whole++;
    cuts.push({budget,split:p.isSplitTurn,lengths:partitions.map(p=>p.length)});
  }
  expect(split).toBeGreaterThan(0);expect(whole).toBeGreaterThan(0);expect(JSON.stringify(source)).toBe(before);
  records.push({case:'tool-pair sweep',budgets:250,split,whole,cuts});
});

it('source boundary: malformed orphan results and unresolved tool calls are not rejected',()=>{
  const orphan=tool('unknown');const unresolved=answer('',{content:[call('pending','write','unconfirmed.ts')],stopReason:'toolUse'});
  const orphanP=getOrThrow(prepareCompaction(entries([orphan]),settings))!;expect(orphanP.retainedTail).toEqual([orphan]);
  const p=getOrThrow(prepareCompaction(entries([user('task'),unresolved,user('next'),answer('done')]),settings))!;
  expect(p.messagesToSummarize).toContain(unresolved);expect(computeFileLists(p.fileOps).modifiedFiles).toEqual(['unconfirmed.ts']);
  records.push({case:'malformed/unresolved acceptance',orphanRetained:true,unresolvedCallSummarized:true,fileFactIsIntentNotSuccessfulWrite:true});
});

it('metadata and branch-summary cut boundaries differ from message token accounting',()=>{
  const u=entries([user('start')])[0];const b:any={type:'branch_summary',id:'branch',parentId:u.id,seq:1,timestamp:1,fromId:'abandoned',summary:'S'.repeat(10000)};
  const a=entries([answer('A'.repeat(100))])[0];a.id='reply';a.parentId=b.id;
  const cut=findCutPoint([u,b,a],0,3,1);expect(cut).toEqual({firstKeptEntryIndex:1,turnStartIndex:1,isSplitTurn:true});
  const custom:any={type:'custom',id:'custom',parentId:u.id,seq:1,timestamp:1,customType:'state'};
  expect(findCutPoint([u,custom,a],0,3,1).firstKeptEntryIndex).toBe(1);
  const customMessage=entries([{role:'custom',customType:'note',content:'n',display:true,timestamp:1}])[0];expect(findTurnStartIndex([customMessage],0,0)).toBe(-1);
  const shell=entries([{role:'bashExecution',command:'true',output:'ok',exitCode:0,cancelled:false,truncated:false,timestamp:1}])[0];
  expect(findTurnStartIndex([shell],0,0)).toBe(0);expect(findCutPoint([shell],0,1,1).isSplitTurn).toBe(true);
  records.push({case:'cut boundary',branchSummaryCut:cut,branchSummaryMessageEstimate:estimateTokens({role:'branchSummary',summary:b.summary,fromId:b.fromId,timestamp:1}),nonmessageBranchSummaryNotCountedByCutLoop:true,customNotTurnStart:true,bashClassifiedSplitAtItsOwnStart:true});
});

it('iterative preparation retains exact message references, previous summary and filtered file facts',async()=>{
  const oldCall=answer('',{content:[call('r','read','文档.md'),call('w','write','same.ts'),call('r2','read','same.ts')]});
  const cp=checkpoint([user('old tail task'),oldCall,tool('w','write failed',true)],{details:{readFiles:['prior.md','same.ts',42],modifiedFiles:['old-edit.ts',null]}});
  const newer=entries([user('new task'),answer('newest')]);const path=[cp,...newer];const p=getOrThrow(prepareCompaction(path,settings))!;
  expect([...p.messagesToSummarize,...p.turnPrefixMessages,...p.retainedTail]).toEqual([...cp.retainedTail,...newer.map(e=>e.message)]);
  expect(p.previousSummary).toBe(cp.summary);expect(computeFileLists(p.fileOps)).toEqual({readFiles:['prior.md','文档.md'],modifiedFiles:['old-edit.ts','same.ts']});
  expect(p.messagesToSummarize[0]).toBe(cp.retainedTail[0]);expect(p.retainedTail[0]).toBe(newer[1].message);
  const requests:any[]=[];const result=getOrThrow(await compactWithRequest(p,{model,customInstructions:'keep pending'},requestQueue([answer('FIXTURE summary intentionally drops old pending'),answer('FIXTURE prefix')],requests),bg));
  expect(requests[0].prompt).toContain(`<previous-summary>\n${cp.summary}\n</previous-summary>`);expect(requests[0].prompt).toContain('In Progress');expect(requests[0].prompt).toContain('Next Steps');
  expect(result.summary).not.toContain('PENDING unfinished migration');expect(result.details).toEqual(computeFileLists(p.fileOps));
  expect(result.summary).toContain('<modified-files>');expect(result.retainedTail).toBe(p.retainedTail);
  const next=checkpoint(result.retainedTail,{summary:result.summary,details:result.details});const reprojected=await buildSessionContext([next],undefined,bg);
  expect(reprojected.map((m:any)=>m.role)).toEqual(['compactionSummary','assistant']);
  const prefixOnlyRequests:any[]=[];
  const prefixOnly=getOrThrow(await compactWithRequest(prep({messagesToSummarize:[],turnPrefixMessages:[user('prefix')],isSplitTurn:true,previousSummary:'OLD_ONLY_PENDING'}),{model},requestQueue([answer('prefix fixture')],prefixOnlyRequests),bg));
  expect(prefixOnlyRequests).toHaveLength(1);expect(prefixOnlyRequests[0].prompt).not.toContain('OLD_ONLY_PENDING');expect(prefixOnly.summary).toContain('No prior history.');expect(prefixOnly.summary).not.toContain('OLD_ONLY_PENDING');
  records.push({case:'iterative',requests,result,prefixOnlyRequests,prefixOnly,previousSummaryOmittedWhenHistoryEmpty:true,semanticPreservationNotValidated:true,failedWriteIncludedAsModifiedIntent:true,shallowReferences:true});
});

it('sequential history-prefix requests preserve options, sum usage and stop on second failure',async()=>{
  const events:string[]=[];let release!:()=>void;const gate=new Promise<void>(r=>release=r);let started!:()=>void;const begin=new Promise<void>(r=>started=r);
  const u1={...usage,reasoning:7,cacheWrite1h:11};const u2={...usage,reasoning:2};let calls=0;const seen:any[]=[];
  const request:any=async(ctx:any,opts:any)=>{const n=++calls;events.push(`start${n}`);seen.push({prompt:ctx.messages[0].content[0].text,maxTokens:opts.maxTokens,reasoning:opts.reasoning,sessionId:opts.sessionId,cacheRetention:opts.cacheRetention});if(n===1){started();await gate;}events.push(`end${n}`);return answer(`summary${n}`,{usage:n===1?u1:u2});};
  const p=prep({isSplitTurn:true,turnPrefixMessages:[user('prefix')]});const running=compactWithRequest(p,{model,thinkingLevel:'high',customInstructions:'history focus'},request,bg);await begin;expect(events).toEqual(['start1']);release();const result=getOrThrow(await running);
  expect(events).toEqual(['start1','end1','start2','end2']);expect(seen.map(o=>o.maxTokens)).toEqual([80,50]);expect(seen[0].sessionId).not.toBe(seen[1].sessionId);expect(seen.every(o=>o.cacheRetention==='none'&&o.reasoning==='high')).toBe(true);
  expect(seen[0].prompt).toContain('history focus');expect(seen[1].prompt).not.toContain('history focus');expect(result.usage).toMatchObject({input:4,output:6,cacheRead:8,cacheWrite:10,totalTokens:28,reasoning:9,cacheWrite1h:11,cost:{total:20}});
  const failures:any[]=[];const failed=await compactWithRequest(p,{model},requestQueue([answer('history'),answer('',{stopReason:'error',errorMessage:'prefix failed'})],failures),bg);expect(failed).toMatchObject({ok:false,error:{code:'summarization_failed'}});expect(failures).toHaveLength(2);
  const firstFailure:any[]=[];expect(await compactWithRequest(p,{model},requestQueue([answer('',{stopReason:'error'})],firstFailure),bg)).toMatchObject({ok:false});expect(firstFailure).toHaveLength(1);
  records.push({case:'sequential/usage',events,seen,result,secondFailure:true,firstFailureStopsPrefix:true});
});

it('source response validation: length, empty and tool-call summaries succeed; thrown errors escape',async()=>{
  const outputs:any[]=[];
  for(const response of [answer('partial',{stopReason:'length'}),answer(''),answer('',{content:[call('bad')],stopReason:'toolUse'})]){
    const r=await generateSummaryWithRequest([user('task')],{model,reserveTokens:100},requestQueue([response]),bg);expect(r.ok).toBe(true);outputs.push({stopReason:response.stopReason,result:r});
  }
  await expect(generateSummaryWithRequest([],{model,reserveTokens:100},async()=>{throw Error('transport rejected');},bg)).rejects.toThrow('transport rejected');
  records.push({case:'response validation',outputs,requestThrowEscapesResult:true});
});

it('actual AbortSignal propagates; cooperative request aborts, ignored abort can still return success',async()=>{
  const controller=new AbortController();const context=withAbortSignal(controller.signal,bg);let entered!:()=>void;const started=new Promise<void>(r=>entered=r);
  const cooperative:any=async(_ctx:any,opts:any)=>new Promise(resolve=>{expect(opts.signal).toBe(controller.signal);opts.signal.addEventListener('abort',()=>resolve(answer('',{stopReason:'aborted',errorMessage:'observed abort'})),{once:true});entered();});
  const promise=generateSummaryWithRequest([],{model,reserveTokens:100},cooperative,context);await started;controller.abort();expect(await promise).toMatchObject({ok:false,error:{code:'aborted'}});
  let calls=0;const noncooperative:any=async()=>{calls++;return answer('ignored aborted signal');};
  const ignored=await compactWithRequest(prep({isSplitTurn:true,turnPrefixMessages:[user('prefix')]}),{model},noncooperative,context);expect(ignored.ok).toBe(true);expect(calls).toBe(2);
  records.push({case:'abort',signalAborted:controller.signal.aborted,cooperative:'aborted result',noncooperativeCalls:calls,noncooperativeReturnedSuccess:true,noCommitOwnerInModule:true});
});

it('real retry implementation: transient retries, quota stops, abort during backoff stops retry',async()=>{
  const events:any[]=[];let calls=0;const requestOptions:any[]=[];
  const models:any={completeSimple:async(_m:any,_ctx:any,opts:any)=>{calls++;requestOptions.push(opts);return calls===1?answer('',{stopReason:'error',errorMessage:'socket hang up'}):answer('recovered');}};
  const callbacks:any={onRetryScheduled:(...a:any[])=>events.push(['scheduled',...a]),onRetryAttemptStart:()=>events.push(['start']),onRetryFinished:(...a:any[])=>events.push(['finished',...a])};
  const result=await completeSimpleWithRetries(models,model,{messages:[]},{sessionId:'fixed-route'}, {enabled:true,maxRetries:2,baseDelayMs:0},callbacks,bg);
  expect(result.stopReason).toBe('stop');expect(calls).toBe(2);expect(requestOptions[0]).toBe(requestOptions[1]);expect(requestOptions[0].sessionId).toBe('fixed-route');expect(events.map(e=>e[0])).toEqual(['scheduled','start','finished']);
  let quotaCalls=0;await completeSimpleWithRetries({completeSimple:async()=>{quotaCalls++;return answer('',{stopReason:'error',errorMessage:'insufficient_quota'});}} as any,model,{messages:[]},{},{enabled:true,maxRetries:3,baseDelayMs:0},undefined,bg);expect(quotaCalls).toBe(1);
  const c=new AbortController();let abortCalls=0;const abortEvents:any[]=[];
  const aborted=await completeSimpleWithRetries({completeSimple:async()=>{abortCalls++;return answer('',{stopReason:'error',errorMessage:'rate limit'});}} as any,model,{messages:[]},{},{enabled:true,maxRetries:3,baseDelayMs:1000},{onRetryScheduled:()=>{abortEvents.push('scheduled');c.abort();},onRetryFinished:(success)=>{abortEvents.push(['finished',success]);}},withAbortSignal(c.signal,bg));
  expect(aborted.stopReason).toBe('aborted');expect(abortCalls).toBe(1);records.push({case:'retry',events,calls,quotaCalls,abortCalls,abortEvents,abortedStop:aborted.stopReason});
});

it('summary request options force context signal, telemetry and no-cache while retaining explicit routing',()=>{
  const a=new AbortController();const old=new AbortController();const telemetry:any={startSpan:()=>{throw Error('not invoked');}};const ctx=withTelemetryContext(telemetry,withAbortSignal(a.signal,bg));
  const opts=createSummaryRequestOptions({signal:old.signal,sessionId:'route',cacheRetention:'long'} as any,ctx);
  expect(opts.signal).toBe(a.signal);expect(opts.telemetryContext).toBe(telemetry);expect(opts.sessionId).toBe('route');expect(opts.cacheRetention).toBe('none');
  records.push({case:'options',contextSignalOverridesCaller:true,actualContextTelemetry:true,explicitRoute:opts.sessionId,cacheRetention:opts.cacheRetention});
});

it('serialization boundary truncates tool data and omits image/hidden shell content; files remain call-derived',()=>{
  const long='x'.repeat(2000)+'PENDING_ONLY_AFTER_TRUNCATION';
  const messages=[user('visible'),{role:'user',content:[{type:'image',mimeType:'image/png',data:'opaque-image'}],timestamp:1},answer('',{content:[call('x','edit','file.ts')]}),tool('x',long,true),{role:'bashExecution',command:'secret-command',output:'secret-output',exitCode:0,cancelled:false,truncated:false,excludeFromContext:true,timestamp:1}];
  const text=serializeConversation(convertToLlm(messages as any));expect(text).not.toContain('PENDING_ONLY_AFTER_TRUNCATION');expect(text).not.toContain('opaque-image');expect(text).not.toContain('secret-command');expect(text).toContain('more characters truncated');expect(text).toContain('edit(path="file.ts")');expect(text).not.toContain('isError');
  records.push({case:'serialization',text,truncatedPendingLost:true,imageBytesOmitted:true,toolResultErrorFlagNotSerialized:true,toolIdsNotSerialized:true});
});

it('branch boundary: collection uses injected session reader, not persistence; excludes common and target sides',async()=>{
  const all:any[]=[{...entries([user('root')])[0],id:'root',parentId:null},{...entries([user('common')])[0],id:'common',parentId:'root'},{...entries([user('abandoned')])[0],id:'old',parentId:'common'},{...entries([user('target')])[0],id:'target',parentId:'common'}];const byId=new Map(all.map(e=>[e.id,e]));
  const branch:any={findEntries:async(q:any)=>{const out=[];let e=byId.get(q.start);while(e){out.push(e);e=byId.get(e.parentId);}return out;}};
  const session:any={getEntry:async(id:string)=>byId.get(id)};
  expect(await collectEntriesForBranchSummary(branch,session,'old','target',bg)).toMatchObject({entries:[byId.get('old')],commonAncestorId:'common'});
  expect(await collectEntriesForBranchSummary(branch,session,'common','target',bg)).toMatchObject({entries:[],commonAncestorId:'common'});
  await expect(collectEntriesForBranchSummary(branch,{getEntry:async()=>undefined} as any,'old','target',bg)).rejects.toThrow('Corrupt session');
  records.push({case:'branch collection',fixtureBoundary:'in-memory injected branch.findEntries/session.getEntry',onlyAbandonedSide:true,missingEntryThrows:true});
});

it('branch summary budget and generation differ: tool results omitted, metadata can survive dropped text',async()=>{
  const b:any={type:'branch_summary',id:'b',parentId:null,seq:0,timestamp:0,fromId:'old',summary:'old branch',details:{readFiles:['prior.md'],modifiedFiles:['prior-write.ts']}};
  const write=entries([answer('',{content:[call('write','write','over-budget.ts')]}),tool('write','FAILED',true)]);
  const p=prepareBranchEntries([b,...write],1);expect(p.messages).toHaveLength(0);expect(computeFileLists(p.fileOps)).toEqual({readFiles:['prior.md'],modifiedFiles:['over-budget.ts','prior-write.ts']});
  let requests=0;const noContent=getOrThrow(await generateBranchSummaryWithRequest(p,{},async()=>{requests++;return answer('unused');},bg));expect(requests).toBe(0);expect(noContent).toEqual({summary:'No content to summarize',readFiles:[],modifiedFiles:[]});
  const over=prepareBranchEntries([b],1);expect(over.messages).toHaveLength(1);expect(over.totalTokens).toBeGreaterThan(1);
  const full=prepareBranchEntries([b,...write],0);expect(full.messages.some((m:any)=>m.role==='toolResult')).toBe(false);
  const seen:any[]=[];const r=getOrThrow(await generateBranchSummaryWithRequest(full,{replaceInstructions:true,customInstructions:'CUSTOM ONLY'},requestQueue([answer('partial branch',{stopReason:'length'})],seen),bg));
  expect(seen[0].maxTokens).toBe(2048);expect(seen[0].prompt).not.toContain('## Goal');expect(seen[0].prompt).toContain('CUSTOM ONLY');expect(r.summary).toContain('partial branch');expect(r.modifiedFiles).toContain('over-budget.ts');
  const aborted=await generateBranchSummaryWithRequest(full,{},requestQueue([answer('',{stopReason:'aborted'})]),bg);expect(aborted).toMatchObject({ok:false,error:{code:'aborted'}});
  records.push({case:'branch generation',seen,result:r,noContent,metadataCollectedBeforeBudget:true,branchSummaryCanExceedBudget:over.totalTokens,toolResultsOmitted:true,lengthAccepted:true});
});

it('token arithmetic boundary is heuristic; disabled prepare and empty-summary requests are caller responsibility',async()=>{
  expect(calculateContextTokens({...usage,totalTokens:0})).toBe(14);expect(calculateContextTokens({...usage,totalTokens:99})).toBe(99);
  expect(shouldCompact(90,100,{...settings,reserveTokens:10})).toBe(false);expect(shouldCompact(91,100,{...settings,reserveTokens:10})).toBe(true);expect(shouldCompact(100,100,{...settings,enabled:false})).toBe(false);
  expect(estimateTokens({role:'user',content:[{type:'image',data:'x',mimeType:'image/png'}],timestamp:1} as any)).toBe(1200);
  const cyclic:any={};cyclic.self=cyclic;expect(estimateTokens(answer('',{content:[{...call('x'),arguments:cyclic}]}))).toBeGreaterThan(0);
  expect(estimateContextTokens([answer('old'),answer('failed',{stopReason:'error'}),user('tail')]).lastUsageIndex).toBe(0);
  const p=getOrThrow(prepareCompaction(entries([user('only user')]),{...settings,enabled:false,keepRecentTokens:1000}))!;expect(p.messagesToSummarize).toHaveLength(0);expect(p.retainedTail).toHaveLength(1);
  const seen:any[]=[];expect((await compactWithRequest(p,{model},requestQueue([answer('empty history summary')],seen),bg)).ok).toBe(true);expect(seen).toHaveLength(1);
  records.push({case:'heuristics and preparation',imageTokens:1200,cyclicArgumentsSafe:true,disabledDoesNotGatePreparation:true,emptyHistoryRequestOccurred:true});
});
