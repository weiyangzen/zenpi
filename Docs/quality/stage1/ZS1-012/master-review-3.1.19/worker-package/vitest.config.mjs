import { defineConfig } from 'vitest/config';
import { fileURLToPath } from 'node:url';
const base=fileURLToPath(new URL('.',import.meta.url));
export default defineConfig({resolve:{alias:{'@earendil-works/pi-ai':base+'pi-ai-runtime.mjs','@earendil-works/chord/context':base+'source/packages/chord/src/context/index.ts','@earendil-works/pi-telemetry':base+'source/packages/telemetry/src/index.ts'}},test:{include:['source/packages/agent/test/harness/*.test.ts','probe.test.ts'],testTimeout:30000,maxWorkers:1,reporters:['verbose','json'],outputFile:{json:process.env.SOURCE012_RESULT||base+'test-results.json'}}});
