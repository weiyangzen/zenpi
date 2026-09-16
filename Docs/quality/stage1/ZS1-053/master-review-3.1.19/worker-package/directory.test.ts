import {it,expect,afterAll} from 'vitest';
import {mkdirSync,mkdtempSync,writeFileSync,readFileSync,existsSync} from 'node:fs';
import {stat,readFile} from 'node:fs/promises';
import {join,basename,dirname} from 'node:path';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {createHash} from 'node:crypto';
import {createBashTool} from './source/packages/coding-agent/src/core/tools/bash.ts';
import {createGrepTool} from './source/packages/coding-agent/src/core/tools/grep.ts';
import {createFindTool} from './source/packages/coding-agent/src/core/tools/find.ts';
import {getToolPath} from './source/packages/coding-agent/src/utils/tools-manager.ts';
const base=fileURLToPath(new URL('.',import.meta.url));const root=mkdtempSync(join(tmpdir(),'fixture-'));
const records:any[]=[];const area=(name:string)=>{const p=join(root,name);mkdirSync(p);return p;};
const text=(r:any)=>r.content.map((c:any)=>c.text||'').join('\n');
const q=(s:string)=>"'"+s.replaceAll("'","'\\''")+"'";
afterAll(()=>writeFileSync(process.env.DIR_OBSERVATIONS!,JSON.stringify({root,native:{fd:getToolPath('fd'),rg:getToolPath('rg')},records},null,2)+'\n'));
it('D053-01 real bash spill is found by fd and searched by rg in this and a fresh process',async()=>{
 const dir=area('pipeline');const b:any=await createBashTool(dir).execute('spill',{command:'exec '+q(process.execPath)+' '+q(join(base,'producer.mjs'))});
 expect(b.details.fullOutputPath).toBeTruthy();expect(b.details.truncation.truncated).toBe(true);
 const path=b.details.fullOutputPath;const expected=Buffer.from('DIRECTORY_NEEDLE\n'+Array.from({length:2500},(_,i)=>'line-'+i+' '+'x'.repeat(40)+'\n').join(''));
 const raw=readFileSync(path);expect(raw.equals(expected)).toBe(true);expect(text(b)).not.toContain('DIRECTORY_NEEDLE');
 const f=await createFindTool(dir).execute('locate',{pattern:basename(path),path:dirname(path)});expect(text(f)).toBe(basename(path));
 const g=await createGrepTool(dir).execute('search',{pattern:'DIRECTORY_NEEDLE',literal:true,path});expect(text(g)).toBe(basename(path)+':1: DIRECTORY_NEEDLE');
 const args=['--experimental-strip-types','--experimental-loader',join(base,'loader.mjs'),join(base,'artifact-reader.mjs'),path];
 const child=await promisify(execFile)(process.execPath,args,{cwd:base,timeout:8000});const restarted=JSON.parse(child.stdout);expect(restarted.pid).not.toBe(process.pid);expect(restarted.result).toEqual(g);expect(readFileSync(path).equals(raw)).toBe(true);
 records.push({case:'D053-01',bash:b,find:f,grep:g,raw:{path,bytes:raw.length,sha256:createHash('sha256').update(raw).digest('hex')},child:{args,stdout:child.stdout,stderr:child.stderr},explicitPathOutsideCwdAllowed:true});
});
it('D053-02 real rg regex literal ignore hidden and context behavior stays distinct from file discovery',async()=>{
 const dir=area('search');mkdirSync(join(dir,'.git'));writeFileSync(join(dir,'.gitignore'),'ignored.txt\n');
 writeFileSync(join(dir,'kept.txt'),'before\r\nalpha1\r\na.1\r\nafter\r\n');writeFileSync(join(dir,'.hidden.txt'),'alpha2\n');writeFileSync(join(dir,'ignored.txt'),'alpha3\n');
 const g=createGrepTool(dir),f=createFindTool(dir);
 const regex=await g.execute('regex',{pattern:'alpha[0-9]',glob:'*.txt'});expect(text(regex)).toContain('kept.txt:2: alpha1');expect(text(regex)).toContain('.hidden.txt:1: alpha2');expect(text(regex)).toContain('ignored.txt');
 const withoutGlob=await g.execute('default-ignore',{pattern:'alpha[0-9]'});expect(text(withoutGlob)).not.toContain('ignored.txt');expect(text(withoutGlob)).toContain('.hidden.txt:1: alpha2');
 const literal=await g.execute('literal',{pattern:'alpha[0-9]',literal:true});expect(text(literal)).toBe('No matches found');
 const context=await g.execute('context',{pattern:'a.1',literal:true,path:'kept.txt',context:1});expect(text(context)).toBe('kept.txt-2- alpha1\nkept.txt:3: a.1\nkept.txt-4- after');
 const found=await f.execute('find',{pattern:'*.txt'});expect(text(found).split('\n').sort()).toEqual(['.hidden.txt','kept.txt']);
 records.push({case:'D053-02',regex,withoutGlob,literal,context,found,explicitPositiveGlobOverridesGitignore:true});
});
it('D053-03 zero limit and truncation metadata differ between grep and find',async()=>{
 const dir=area('limits');writeFileSync(join(dir,'a.txt'),'NEEDLE '+ 'x'.repeat(3000)+'\nNEEDLE second\n');writeFileSync(join(dir,'b.txt'),'other\n');
 const g:any=await createGrepTool(dir).execute('grep',{pattern:'NEEDLE',limit:0});expect(g.details.matchLimitReached).toBe(1);expect(g.details.linesTruncated).toBe(true);expect(g.details.fullOutputPath).toBeUndefined();
 const f:any=await createFindTool(dir).execute('find',{pattern:'*.txt',limit:0});expect(text(f)).toContain('a.txt');expect(text(f)).toContain('b.txt');expect(f.details.resultLimitReached).toBe(0);expect(f.details.fullOutputPath).toBeUndefined();
 records.push({case:'D053-03',grep:g,find:f,noSharedPositiveLimitContract:true});
});
it('D053-04 all wrappers reject pre-abort; real rg syntax and missing-path errors remain errors',async()=>{
 const dir=area('negative');const c=new AbortController();c.abort();
 await expect(createBashTool(dir).execute('bash',{command:'touch '+q(join(dir,'effect'))},c.signal)).rejects.toThrow('aborted');
 await expect(createGrepTool(dir).execute('grep',{pattern:'x'},c.signal)).rejects.toThrow('aborted');
 await expect(createFindTool(dir).execute('find',{pattern:'*'},c.signal)).rejects.toThrow('aborted');expect(existsSync(join(dir,'effect'))).toBe(false);
 writeFileSync(join(dir,'input.txt'),'input\n');
 await expect(createGrepTool(dir).execute('regex',{pattern:'['})).rejects.toThrow();
 await expect(createGrepTool(dir).execute('missing',{pattern:'x',path:'absent'})).rejects.toThrow('Path not found');
 records.push({case:'D053-04',preAbortAllRejected:true,bashSideEffectAbsent:true,actualRgInvalidRegexRejected:true,missingPathRejected:true});
});
it('D053-05 rg context formatting after child close ignores late abort and rereads changed file',async()=>{
 const dir=area('late');const path=join(dir,'input.txt');writeFileSync(path,'NEEDLE\nold context\n');let entered!:()=>void;const started=new Promise<void>(r=>entered=r);let release!:()=>void;const gate=new Promise<void>(r=>release=r);
 const c=new AbortController();const g=createGrepTool(dir,{operations:{isDirectory:async p=>(await stat(p)).isDirectory(),readFile:async p=>{entered();await gate;return readFile(p,'utf8');}}});
 const running=g.execute('late',{pattern:'NEEDLE',context:1},c.signal);await started;c.abort();writeFileSync(path,'CHANGED\nnew context\n');release();const result=await running;
 expect(c.signal.aborted).toBe(true);expect(text(result)).toBe('input.txt:1: CHANGED\ninput.txt-2- new context');
 records.push({case:'D053-05',result,lateAbortIgnored:true,contextReadAfterRgResultChanged:true,operationsBoundary:'actual stat/readFile with an explicit asynchronous gate; native rg search'});
});

