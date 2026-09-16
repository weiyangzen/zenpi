import { writeFileSync, readFileSync, existsSync, writeSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
const [mode,dir,arg]=process.argv.slice(2);
const self=fileURLToPath(import.meta.url);
const safety=setTimeout(()=>process.exit(91),7000);safety.unref();
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
if(mode==='mixed'){
  writeSync(1,Buffer.from('OUT\n'));await pause(25);writeSync(2,Buffer.from('ERR\n'));writeSync(1,Buffer.from([0,27,91,51,49,109]));
} else if(mode==='lines'){
  writeSync(1,Buffer.from(Array.from({length:Number(arg)},(_,i)=>`line-${i}\n`).join('')));
} else if(mode==='utf8'){
  const data=Buffer.from('界🌍'.repeat(Number(arg)));writeSync(1,data.subarray(0,1));await pause(30);writeSync(1,data.subarray(1,5));await pause(30);writeSync(1,data.subarray(5));
} else if(mode==='interleaved-utf8'){
  writeSync(1,Buffer.from([0xe7]));await pause(40);writeSync(2,Buffer.from('E'));await pause(40);writeSync(1,Buffer.from([0x95,0x8c]));
} else if(mode==='invalid'){
  writeSync(1,Buffer.concat([Buffer.alloc(55000,120),Buffer.from([255,254,226,130])]));
} else if(mode==='stream'){
  for(let i=0;i<120;i++){writeSync(1,Buffer.from(`${i}:`+'x'.repeat(1000)+'\n'));await pause(3);}
  writeFileSync(join(dir,'stream-ready'),'ready');writeSync(1,Buffer.from('WAITING_RELEASE\n'));
  while(!existsSync(join(dir,'release')))await pause(10);
  writeSync(1,Buffer.from('FINAL_AFTER_RELEASE\n'));
} else if(mode==='heartbeat'){
  writeFileSync(join(dir,'grand-ready'),String(process.pid));let n=0;setInterval(()=>writeFileSync(join(dir,'heartbeat'),String(++n)),20);
} else if(mode==='tree'){
  const child=spawn(process.execPath,[self,'heartbeat',dir],{stdio:'ignore'});
  writeFileSync(join(dir,'pids.json'),JSON.stringify({parent:process.pid,grandchild:child.pid}));
  while(!existsSync(join(dir,'grand-ready')))await pause(5);
  writeSync(1,Buffer.from('x'.repeat(60000)+'\nTREE_READY\n'));
  setInterval(()=>{},1000);
} else if(mode==='signal'){
  writeSync(1,Buffer.from('BEFORE_SIGNAL\n'));process.kill(process.pid,'SIGTERM');
} else if(mode==='descendant'){
  writeSync(1,Buffer.from('DESC_READY\n'));process.send?.('ready');
  if(arg==='active'){
    for(let i=0;i<8;i++){writeSync(1,Buffer.from(`TICK${i}\n`));await pause(30);}
    writeSync(1,Buffer.from('DESC_FINAL\n'));process.exit(0);
  }
  setInterval(()=>{},1000);
} else if(mode==='inherit'){
  const child=spawn(process.execPath,[self,'descendant',dir,arg],{stdio:['ignore',1,2,'ipc'],detached:true});
  writeFileSync(join(dir,'pids.json'),JSON.stringify({parent:process.pid,grandchild:child.pid}));
  child.once('message',()=>{child.disconnect();child.unref();writeSync(1,Buffer.from('PARENT_EXIT\n'));process.exit(0);});
} else if(mode==='env'){
  const keys=['PI_SESSION_ID','PI_SESSION_FILE','PI_PROVIDER','PI_MODEL','PI_REASONING_LEVEL','PROBE_PREFIX','PROBE_HOOK'];
  const env=Object.fromEntries(keys.map(k=>[k,process.env[k]??null]));
  console.log(JSON.stringify({pid:process.pid,cwd:process.cwd(),env}));
} else if(mode==='mark'){
  writeFileSync(join(dir,'effect'),'spawned');
} else throw Error('unknown fixture mode '+mode);
