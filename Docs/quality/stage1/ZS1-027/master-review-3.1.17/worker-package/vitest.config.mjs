import { defineConfig } from 'vitest/config';
import { fileURLToPath } from 'node:url';
const base=fileURLToPath(new URL('.',import.meta.url));
export default defineConfig({resolve:{alias:[{find:'./renderers/find.ts',replacement:base+'renderer-bridge.mjs'}]},test:{include:['source/packages/coding-agent/test/suite/regressions/*.test.ts','probe.test.ts'],testTimeout:20000,maxWorkers:1,reporters:['verbose','json'],outputFile:{json:process.env.SOURCE027_RESULT||base+'test-results.json'}}});
