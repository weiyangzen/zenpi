import fs from 'node:fs';import {createHash} from 'node:crypto';import assert from 'node:assert/strict';
for(const file of JSON.parse(fs.readFileSync(new URL('./source-inventory.json',import.meta.url)))){const raw=fs.readFileSync(new URL('./source/'+file.path,import.meta.url));assert.equal(raw.length,file.bytes);assert.equal(createHash('sha256').update(raw).digest('hex'),file.sha256)}
