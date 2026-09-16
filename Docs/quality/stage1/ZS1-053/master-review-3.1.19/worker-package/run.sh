#!/bin/sh
set -eu
cd "$(dirname "$0")"
: "${NODE_BIN:=node}"
if [ ! -e node_modules ]; then ln -s runtime/node_modules node_modules; fi
"$NODE_BIN" verify-source.mjs
REPLAY_DIR=$(mktemp -d "$PWD/replay.XXXXXX")
export PI_CODING_AGENT_DIR="$REPLAY_DIR/agent-home"
export PI_PACKAGE_DIR="$PWD/source/packages/coding-agent"
export PI_OFFLINE=1
export TMPDIR=$(mktemp -d /tmp/zenpi-directory053.XXXXXX)
export DIR_RESULT="$REPLAY_DIR/test-results.json"
export DIR_OBSERVATIONS="$REPLAY_DIR/observations.json"
"$NODE_BIN" setup-native.mjs > "$REPLAY_DIR/native.json"
"$NODE_BIN" runtime/node_modules/vitest/vitest.mjs run --config vitest.config.mjs > "$REPLAY_DIR/stdout.log" 2> "$REPLAY_DIR/stderr.log"
"$NODE_BIN" verify-source.mjs
printf '%s\n' "$REPLAY_DIR"

