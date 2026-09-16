import {readFileSync,writeFileSync,mkdirSync,copyFileSync,chmodSync} from 'node:fs';
import {join} from 'node:path';
import {gunzipSync} from 'node:zlib';
import {createHash} from 'node:crypto';
const metadata=JSON.parse(readFileSync(new URL('./native-tools.json',import.meta.url),'utf8'));
const hash=b=>createHash('sha256').update(b).digest('hex');
if(process.platform!=='darwin'||process.arch!=='arm64')throw Error('Recorded native binaries require Darwin arm64');
const bin=join(process.env.PI_CODING_AGENT_DIR,'bin');mkdirSync(bin,{recursive:true});
const fd=gunzipSync(readFileSync(new URL('./runtime/native/fd-darwin-arm64.gz',import.meta.url)));
if(hash(fd)!==metadata.fd.sha256)throw Error('fd hash mismatch');
writeFileSync(join(bin,'fd'),fd);chmodSync(join(bin,'fd'),0o755);
const rgPath=process.env.RG_BIN||metadata.rg.host_path;const rg=readFileSync(rgPath);
if(hash(rg)!==metadata.rg.sha256)throw Error('rg must match recorded host binary hash');
copyFileSync(rgPath,join(bin,'rg'));chmodSync(join(bin,'rg'),0o755);
console.log(JSON.stringify({fd:metadata.fd,rg:metadata.rg,bin}));

