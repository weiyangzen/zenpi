# Transform Note: `pi-mono/packages/coding-agent/src/modes/rpc/rpc-mode.ts`

source_path: `pi-mono/packages/coding-agent/src/modes/rpc/rpc-mode.ts`
source_hash: `3e7c2374eefd3e2d74720ea5b32f78040ac3252e1c21fd5f609353e50d27d222`

The source keeps a process alive, correlates commands, streams events, and
shuts down on EOF. zenpi keeps only that useful stdio behavior in
`headless::run_headless`; extension UI requests, RPC selectors, and server
transport are omitted. Every accepted request gets one terminal response,
events remain typed JSON lines, and EOF leaves a flushed durable session.
