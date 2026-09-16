#!/bin/sh
set -eu
cd "$(dirname "$0")"
: "${NODE_BIN:=node}"
"$NODE_BIN" --input-type=module -e 'const [a,b]=process.versions.node.split(".").map(Number);if(a<22||(a===22&&b<19))throw Error("Node >=22.19.0 required")'
if [ ! -e node_modules ]; then ln -s runtime/node_modules node_modules; fi
"$NODE_BIN" verify-source.mjs
ZENPI_SOURCE_REPLAY_OUTPUT=./replay-results.json "$NODE_BIN" runtime/node_modules/vitest/vitest.mjs run --config vitest.config.mjs > replay.stdout.log 2> replay.stderr.log
"$NODE_BIN" verify-source.mjs
