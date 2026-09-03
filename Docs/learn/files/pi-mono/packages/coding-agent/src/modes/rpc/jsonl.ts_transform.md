# Transform Note: `pi-mono/packages/coding-agent/src/modes/rpc/jsonl.ts`

source_path: `pi-mono/packages/coding-agent/src/modes/rpc/jsonl.ts`
source_hash: `95723d349fcebad1f1da7ce103d02ba7d5e2c876b7d178d41d8b56beedbd93e0`

The source uses LF-only framing and does not split on U+2028 or U+2029. zenpi
implements the same rule in `protocol::parse_line` and `encode_line`, accepts a
single optional CR for interoperability, bounds each frame, and returns typed
errors without mutating the agent. Protocol unit tests and the shell smoke
test cover split, malformed, Unicode, and multiple-frame inputs.
