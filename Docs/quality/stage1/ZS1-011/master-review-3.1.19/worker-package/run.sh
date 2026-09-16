#!/bin/sh
set -eu
cd "$(dirname "$0")"
: "${NODE_BIN:=node}"
"$NODE_BIN" --input-type=module -e 'const [major,minor]=process.versions.node.split(".").map(Number);if(major<22||(major===22&&minor<19))throw Error("Node >=22.19.0 required")'
"$NODE_BIN" verify-source.mjs
"$NODE_BIN" --experimental-strip-types --experimental-loader ./loader.mjs probe.mjs > replay.json 2> replay.stderr.log
"$NODE_BIN" verify-source.mjs
