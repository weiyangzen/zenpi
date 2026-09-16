import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import {spawn} from 'node:child_process';
import {join} from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';
import {AssistantMessageEventStream} from './pi-ai-runtime.mjs';
export const model={id:'scripted-model',name:'scripted-model',api:'probe',provider:'probe',baseUrl:'',reasoning:true,input:['text'],cost:{input:0,output:0,cacheRead:0,cacheWrite:0},contextWindow:32000,maxTokens:1000};
export const user=text=>({role:'user',content:text,timestamp:1});
export const toolCall=(id,name='process_tool',args={id})=>({type:'toolCall',id,name,arguments:args});
export const assistant=(content=[],stopReason='stop')=>({role:'assistant',content,api:'probe',provider:'probe',model:'scripted-model',stopReason,usage:{input:0,output:0,cacheRead:0,cacheWrite:0,totalTokens:0,cost:{input:0,output:0,cacheRead:0,cacheWrite:0,total:0}},timestamp:2});
export function scriptedStream(responses,requests) {
 return (selected,context,options)=>{
  const index=requests.length;assert(index<responses.length,'unexpected provider turn');
  requests.push(structuredClone({model:selected,context:{...context,tools:context.tools?.map(({execute,prepareArguments,...declaration})=>declaration)},reasoning:options.reasoning??null,aborted:options.signal?.aborted??false}));
  const stream=new AssistantMessageEventStream();const message=responses[index];
  queueMicrotask(()=>{stream.push({type:'start',partial:{...message,content:[]}});stream.push({type:'done',reason:message.stopReason,message});});return stream;
 };
}
export async function exists(path){try{await fs.stat(path);return true}catch(e){if(e.code==='ENOENT')return false;throw e}}
export async function until(predicate,label){const end=Date.now()+8000;while(!await predicate()){assert(Date.now()<end,'timeout: '+label);await delay(5)}}
export async function release(root,id){await fs.writeFile(join(root,'release-'+id),'released')}
const childCode=`const fs=require('node:fs'),p=require('node:path');const [root,id]=process.argv.slice(1);process.on('SIGTERM',()=>{fs.writeFileSync(p.join(root,'cancelled-'+id),String(process.pid));process.exit(23)});fs.writeFileSync(p.join(root,'started-'+id),String(process.pid));let ticks=0;const t=setInterval(()=>{if(fs.existsSync(p.join(root,'release-'+id))){clearInterval(t);fs.writeFileSync(p.join(root,'product-'+id),'actual-process:'+id+':'+process.pid);process.stdout.write('actual-output:'+id);process.exit(0)}if(++ticks>1200)process.exit(24)},5)`;
export function processTool(root,trace,options={}) {
 return {name:options.name??'process_tool',label:'actual process fixture',description:'Actual local process and file effects; not upstream bash tool',executionMode:options.executionMode,parameters:{type:'object',properties:{id:{type:'string'}},required:['id'],additionalProperties:false},execute:async(callId,args,signal,update)=>{
  options.onUpdate?.(update);const id=String(args.id);assert(/^[a-zA-Z0-9-]+$/.test(id));trace.push({kind:'execute',callId,args:structuredClone(args)});
  const child=spawn(process.execPath,['-e',childCode,root,id],{stdio:['ignore','pipe','pipe']});trace.push({kind:'spawn',id,pid:child.pid});let stdout='',stderr='';child.stdout.on('data',b=>{stdout+=b;update?.({content:[{type:'text',text:b.toString()}],details:{id}})});child.stderr.on('data',b=>stderr+=b);
  const abort=()=>child.kill('SIGTERM');signal?.addEventListener('abort',abort,{once:true});if(signal?.aborted)abort();
  const [code,killed]=await new Promise((resolve,reject)=>{child.once('error',reject);child.once('close',(code,killed)=>resolve([code,killed]))});signal?.removeEventListener('abort',abort);trace.push({kind:'close',id,pid:child.pid,code,signal:killed,stdout,stderr});
  if(code!==0)throw new Error('actual child exit '+code+' signal '+killed);
  const product=await fs.readFile(join(root,'product-'+id),'utf8');assert(product.endsWith(':'+child.pid));
  return {content:[{type:'text',text:stdout}],details:{id,pid:child.pid,product}};
 }};
}
export function recorder(events){return async event=>{events.push(structuredClone(event))}}
export const users=request=>request.context.messages.filter(m=>m.role==='user').map(m=>m.content);
export async function artifacts(root){const out={};for(const name of (await fs.readdir(root)).sort())out[name]=await fs.readFile(join(root,name),'utf8');return out}
