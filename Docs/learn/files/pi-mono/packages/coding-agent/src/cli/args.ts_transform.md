# Transform Note: `pi-mono/packages/coding-agent/src/cli/args.ts`

source_path: `pi-mono/packages/coding-agent/src/cli/args.ts`
source_hash: `5bd31a9b22943d4479539f2c90891ce83641bdea96da6e7a3b8f2c1ecd0b3593`

The source accepts three output labels (`text`, `json`, and `rpc`) plus many
feature flags. zenpi intentionally maps the mode label to the closed Rust
enum `RunMode::{Tui, Headless}` and rejects every other value before opening
state. Prompt payloads move into a typed `TurnInputRequest`; optional provider
flags remain outside the mode contract. Validation is `cargo test` plus the
forbidden-mode subprocess probe.
