import {createGrepTool} from './source/packages/coding-agent/src/core/tools/grep.ts';
import {dirname} from 'node:path';
const result=await createGrepTool(dirname(process.argv[2])).execute('restart-read',{pattern:'DIRECTORY_NEEDLE',path:process.argv[2],literal:true});
console.log(JSON.stringify({pid:process.pid,result}));

