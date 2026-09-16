import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
const inventory=JSON.parse(readFileSync(new URL('./source-inventory.json',import.meta.url),'utf8'));
for(const item of inventory){const bytes=readFileSync(new URL('./source/'+item.path,import.meta.url));const hash=createHash('sha256').update(bytes).digest('hex');if(hash!==item.sha256||bytes.length!==item.bytes)throw Error('Source mismatch: '+item.path);}
console.log(JSON.stringify({verified:inventory.length,source:'frozen byte-for-byte source copies'}));
