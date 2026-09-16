import assert from 'node:assert/strict';
import {readFileSync,writeFileSync,mkdirSync,mkdtempSync,rmSync,symlinkSync,existsSync} from 'node:fs';
import {join,dirname} from 'node:path';
import {tmpdir} from 'node:os';
import {createHash} from 'node:crypto';
const source='/Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/core/prompt-templates.ts';
const digest=()=>createHash('sha256').update(readFileSync(source)).digest('hex');
const expected='e94b8504b97fe668b04577891b7029abc7d11ac795e728982d2615a13ec1528a';
assert.equal(digest(),expected);
const {parseCommandArgs,substituteArgs,loadPromptTemplates,expandPromptTemplate}=await import(source);
const {CONFIG_DIR_NAME}=await import('/Users/wangweiyang/GitHub/pi-mono/packages/coding-agent/src/config.ts');
const records:any[]=[];
function check(name:string,fn:()=>void){fn();records.push({name,status:'pass'});}
const root=mkdtempSync(join(tmpdir(),'zenpi-source020-'));
function put(p:string,text:string|Uint8Array){mkdirSync(dirname(p),{recursive:true});writeFileSync(p,text);return p;}
function fixture(name:string){const cwd=join(root,name,'project'),agentDir=join(root,name,'agent');mkdirSync(cwd,{recursive:true});mkdirSync(agentDir,{recursive:true});return {cwd,agentDir,promptPaths:[] as string[],includeDefaults:true};}
try {
check('quote concatenation and mixed quote literals',()=>assert.deepEqual(parseCommandArgs(`pre"two words"post 'other "quote"'`),['pretwo wordspost','other "quote"']));
check('empty quotes disappear and unmatched quote consumes EOF',()=>assert.deepEqual(parseCommandArgs(`"" '' one 'two words`),['one','two words']));
check('backslash does not escape quote and unicode whitespace splits',()=>assert.deepEqual(parseCommandArgs('a\\"b c"\u3000日本\n語'),['a\\b c','日本','語']));
check('one-pass replacement keeps shell and placeholder strings literal',()=>{const s='$(touch '+join(root,'NEVER')+')';assert.equal(substituteArgs('$1|$2|$ARGUMENTS',['$2',s]),'$2|'+s+'|$2 '+s);assert(!existsSync(join(root,'NEVER')));});
check('multi-digit and missing position plus all arguments',()=>assert.equal(substituteArgs('$1/$10/$11/$0/$@/$ARGUMENTS',Array.from({length:10},(_,i)=>String(i+1))),'1/10///1 2 3 4 5 6 7 8 9 10/1 2 3 4 5 6 7 8 9 10'));
check('defaults preserve inserted placeholders and replace empty position',()=>assert.equal(substituteArgs('${1:-$2}|${2:-fallback}|${@:-all}|${ARGUMENTS:-none}',['','x']),'$2|x| x| x'));
check('empty all defaults and huge position',()=>assert.equal(substituteArgs('${@:-$1}|${ARGUMENTS:-none}|$999999999999999999999999',[]),'$1|none|'));
check('one-based slices clamp zero and beyond end; zero length',()=>assert.equal(substituteArgs('${@:0}|${@:2}|${@:2:1}|${@:2:0}|${@:9}',['a','b','c']),'a b c|b c|b||'));
check('unknown syntax and literal backslash dollar prefix',()=>assert.equal(substituteArgs('$NAME|${1}|${@:x}|\\$1|$1.5',['v']),'$NAME|${1}|${@:x}|\\v|v.5'));
check('source has no 64KiB substitution input limit',()=>assert.equal(substituteArgs('$1',['x'.repeat(65537)]).length,65537));
check('default discovery order duplicates and source scope',()=>{const o=fixture('order');put(join(o.agentDir,'prompts/same.md'),'global $1');put(join(o.cwd,CONFIG_DIR_NAME,'prompts/same.md'),'project $1');const explicit=put(join(root,'order/extra/same.md'),'extra $1');o.promptPaths=[explicit];const t=loadPromptTemplates(o);assert.deepEqual(t.map(x=>x.content),['global $1','project $1','extra $1']);assert.deepEqual(t.map(x=>x.sourceInfo.scope),['user','project','temporary']);assert(t.every(x=>x.sourceInfo.origin==='top-level'&&x.sourceInfo.source==='local'));assert.equal(expandPromptTemplate('/same value',t),'global value');});
check('includeDefaults=false and trimmed relative explicit path',()=>{const o=fixture('explicit');put(join(o.agentDir,'prompts/ignored.md'),'ignored');put(join(o.cwd,'nested/one.md'),'body');o.includeDefaults=false;o.promptPaths=['  nested/one.md  '];const t=loadPromptTemplates(o);assert.equal(t.length,1);assert.equal(t[0].filePath,join(o.cwd,'nested/one.md'));assert.equal(t[0].sourceInfo.baseDir,join(o.cwd,'nested'));});
check('nonrecursive directory skips extension mismatch and nested files',()=>{const o=fixture('direct');const d=join(o.cwd,'extras');put(join(d,'yes.md'),'yes');put(join(d,'no.MD'),'no');put(join(d,'nested/no.md'),'no');o.includeDefaults=false;o.promptPaths=[d];assert.deepEqual(loadPromptTemplates(o).map(t=>t.name),['yes']);});
check('frontmatter BOM CRLF body trim description and argument hint',()=>{const o=fixture('fm');o.promptPaths=[put(join(o.cwd,'p.md'),'\uFEFF---\r\ndescription: explicit\r\nargument-hint: "<file>"\r\n---\r\n  Body $1  \r\n')];o.includeDefaults=false;const t=loadPromptTemplates(o)[0];assert.equal(t.description,'explicit');assert.equal(t.argumentHint,'<file>');assert.equal(t.content,'Body $1');});
check('no-frontmatter body whitespace stays and description uses first nonempty line',()=>{const o=fixture('body');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'p.md'),'\n  description\n rest  \n')];const t=loadPromptTemplates(o)[0];assert.equal(t.content,'\n  description\n rest  \n');assert.equal(t.description,'  description');});
check('description truncation uses UTF16 and can split surrogate pair',()=>{const o=fixture('utf16');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'p.md'),'a'.repeat(59)+'😀tail')];const t=loadPromptTemplates(o)[0];assert.equal(t.description.length,63);assert.equal(t.description.charCodeAt(59),0xd83d);assert.equal(t.description.slice(60),'...');});
check('malformed YAML silently skips file',()=>{const o=fixture('bad-yaml');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'p.md'),'---\ndescription: [\n---\nbody')];assert.equal(loadPromptTemplates(o).length,0);});
check('unterminated frontmatter remains ordinary body',()=>{const o=fixture('unclosed');o.includeDefaults=false;const body='---\ndescription: unclosed\nbody';o.promptPaths=[put(join(o.cwd,'p.md'),body)];assert.equal(loadPromptTemplates(o)[0].content,body);});
check('runtime frontmatter types are not enforced',()=>{const o=fixture('types');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'p.md'),'---\ndescription: 42\nargument-hint: true\n---\nbody')];const t=loadPromptTemplates(o)[0];assert.equal(t.description as any,42);assert.equal(t.argumentHint as any,true);});
check('empty file is still a template',()=>{const o=fixture('empty');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'empty.md'),'')];assert.equal(loadPromptTemplates(o)[0].content,'');});
check('invalid UTF8 is replaced by the filesystem decoder',()=>{const o=fixture('invalid-utf8');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'p.md'),new Uint8Array([0xff,0x61]))];assert.equal(loadPromptTemplates(o)[0].content,'\ufffda');});
check('missing and unsupported explicit files silently skipped',()=>{const o=fixture('missing');o.includeDefaults=false;o.promptPaths=[join(o.cwd,'missing.md'),put(join(o.cwd,'p.txt'),'text')];assert.equal(loadPromptTemplates(o).length,0);});
check('symlink file followed, broken and directory links skipped during scan',()=>{const o=fixture('links');const d=join(o.cwd,'scan');mkdirSync(d);const target=put(join(o.cwd,'target.md'),'linked');symlinkSync(target,join(d,'alias.md'));symlinkSync(join(o.cwd,'absent'),join(d,'broken.md'));symlinkSync(o.agentDir,join(d,'directory.md'));o.includeDefaults=false;o.promptPaths=[d];const t=loadPromptTemplates(o);assert.deepEqual(t.map(x=>x.name),['alias']);assert.equal(t[0].content,'linked');assert.equal(t[0].filePath,join(d,'alias.md'));});
check('repeated explicit file remains duplicated',()=>{const o=fixture('duplicate');o.includeDefaults=false;const p=put(join(o.cwd,'p.md'),'body');o.promptPaths=[p,p];assert.equal(loadPromptTemplates(o).length,2);});
check('source ancestry requires path separator',()=>{const o=fixture('prefix');o.includeDefaults=false;o.promptPaths=[put(join(o.agentDir,'prompts-neighbor/p.md'),'body')];assert.equal(loadPromptTemplates(o)[0].sourceInfo.scope,'temporary');});
check('reload rereads body while old template snapshot remains eager',()=>{const o=fixture('reload');o.includeDefaults=false;const p=put(join(o.cwd,'p.md'),'old');o.promptPaths=[p];const old=loadPromptTemplates(o);put(p,'new');assert.equal(old[0].content,'old');assert.equal(loadPromptTemplates(o)[0].content,'new');});
check('newline command args expand and unknown/case/leading-space remain unchanged',()=>{const o=fixture('expand');o.includeDefaults=false;o.promptPaths=[put(join(o.cwd,'p.md'),'$1|$2|$@')];const t=loadPromptTemplates(o);assert.equal(expandPromptTemplate('/p\n"first word" second',t),'first word|second|first word second');for(const s of ['/P x','/unknown',' /p x','plain','/'])assert.equal(expandPromptTemplate(s,t),s);});
assert.equal(digest(),expected);
console.log(JSON.stringify({source,sha256:expected,cases:records.length,mocks:[],runtime:'actual Bun TS imports; actual yaml parser and filesystem; owned temporary fixture only',records},null,2));
} finally {rmSync(root,{recursive:true,force:true});}
