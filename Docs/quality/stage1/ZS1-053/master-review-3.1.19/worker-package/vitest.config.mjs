import {defineConfig} from 'vitest/config';
import {fileURLToPath} from 'node:url';
const base=fileURLToPath(new URL('.',import.meta.url));
export default defineConfig({resolve:{alias:['bash','grep','find'].map(name=>({find:'./renderers/'+name+'.ts',replacement:base+'renderer-bridge.mjs'}))},test:{include:['directory.test.ts'],testTimeout:20000,maxWorkers:1,reporters:['verbose','json'],outputFile:{json:process.env.DIR_RESULT||base+'test-results.json'}}});

