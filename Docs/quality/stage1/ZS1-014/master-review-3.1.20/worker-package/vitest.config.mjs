import {fileURLToPath} from 'node:url';
const at=path=>fileURLToPath(new URL(path,import.meta.url));
export default {
 root:at('./'),
 test:{include:['source/packages/agent/test/agent-loop.test.ts'],environment:'node',testTimeout:30000,reporters:['verbose','json'],outputFile:{json:at(process.env.ZENPI_SOURCE_REPLAY_OUTPUT || './results.json')},fileParallelism:false,maxWorkers:1},
 resolve:{alias:[{find:/^@earendil-works\/pi-ai$/,replacement:at('./pi-ai-runtime.mjs')},{find:'../src/index.ts',replacement:at('./agent-index-runtime.mjs')}]}
};
