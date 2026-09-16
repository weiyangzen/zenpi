#!/bin/sh
set -eu
cd "$(dirname "$0")"
: "${NODE_BIN:=node}"
if [ ! -e node_modules ]; then ln -s runtime/node_modules node_modules; fi
"$NODE_BIN" verify-source.mjs
REPLAY_DIR=$(mktemp -d "$PWD/replay.XXXXXX")
"$NODE_BIN" runtime/node_modules/vitest/vitest.mjs list --config vitest.config.mjs --json > "$REPLAY_DIR/collection.json" 2> "$REPLAY_DIR/stderr.log"
printf '%s\n' "$REPLAY_DIR"

