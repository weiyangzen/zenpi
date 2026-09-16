#!/bin/sh
set -eu
cd "$(dirname "$0")"
: "${NODE_BIN:=node}"
"$NODE_BIN" --input-type=module -e 'const [a,b]=process.versions.node.split(".").map(Number);if(a<22||(a===22&&b<19))throw Error("Node >=22.19.0 required")'
if [ ! -e node_modules ]; then ln -s runtime/node_modules node_modules; fi
"$NODE_BIN" verify-source.mjs
REPLAY_DIR=$(mktemp -d "$PWD/replay.XXXXXX")
mkdir -p "$REPLAY_DIR/tmp" "$REPLAY_DIR/agent-home"
export TMPDIR="$REPLAY_DIR/tmp"
export PI_CODING_AGENT_DIR="$REPLAY_DIR/agent-home"
export PI_PACKAGE_DIR="$PWD/source/packages/coding-agent"
export SOURCE025_ARTIFACTS="$REPLAY_DIR/artifacts"
export SOURCE025_RESULT="$REPLAY_DIR/test-results.json"
"$NODE_BIN" runtime/node_modules/vitest/vitest.mjs run --config vitest.config.mjs > "$REPLAY_DIR/stdout.log" 2> "$REPLAY_DIR/stderr.log"
"$NODE_BIN" verify-source.mjs
printf '%s\n' "$REPLAY_DIR"
