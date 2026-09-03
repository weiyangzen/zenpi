# Transform Note: `pi-mono/packages/coding-agent/src/main.ts`

source_path: `pi-mono/packages/coding-agent/src/main.ts`
source_hash: `bf990fbe55858919b59ca51562058bfda773e5ddff9937f54743c7dae486a60b`

The source resolves configuration, creates one session, and dispatches a
selected mode. zenpi keeps that ordering: parse and validate CLI arguments,
then construct one backend, session store, and core agent, then enter exactly
one transport. Invalid modes therefore cannot create a session file. The
mapping is exercised by the executable mode-boundary test and headless smoke.
