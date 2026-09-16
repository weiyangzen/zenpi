import { appendFileSync, writeFileSync } from 'node:fs';
const mode=process.env.SOURCE027_FAULT;
appendFileSync(process.env.SOURCE027_FAULT_LOG,JSON.stringify({pid:process.pid,mode,args:process.argv.slice(2)})+'\n');
if(mode==='partial'){process.stdout.write('partial.txt\n');process.stderr.write('injected fd failure\n');process.exitCode=7;}
else if(mode==='empty-error'){process.stderr.write('injected empty failure\n');process.exitCode=7;}
else if(mode==='args'){process.stdout.write('fixture.txt\n');}
else if(mode==='blank-error'){process.stdout.write('\n\n');process.stderr.write('blank failure\n');process.exitCode=7;}
else if(mode==='cancel'){
  process.on('SIGTERM',()=>{writeFileSync(process.env.SOURCE027_TERM_SEEN,String(process.pid));setTimeout(()=>process.exit(0),350);});
  writeFileSync(process.env.SOURCE027_READY,String(process.pid));
  setInterval(()=>{},1000);setTimeout(()=>process.exit(91),6000).unref();
}else throw Error('unknown fd fault fixture mode');
