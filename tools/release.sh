#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="${TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
OUT="${OUT:-$ROOT/dist}"
VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
NAME="zenpi-v${VERSION}-${TARGET}"
STAGE="$OUT/$NAME"
ARCHIVE="$OUT/$NAME.tar.gz"

rm -rf "$STAGE" "$ARCHIVE"
mkdir -p "$STAGE"
cargo build --manifest-path "$ROOT/Cargo.toml" --release --locked --target "$TARGET"
BIN="zenpi"
if [[ "$TARGET" == *windows* ]]; then BIN="zenpi.exe"; fi
install -m 0755 "$ROOT/target/$TARGET/release/$BIN" "$STAGE/$BIN"
install -m 0644 "$ROOT/README.md" "$ROOT/LICENSE" "$STAGE/"

# A source-package inventory is deterministic and does not require a network
# service. It intentionally lists exact Cargo packages rather than build paths.
cargo metadata --manifest-path "$ROOT/Cargo.toml" --locked --format-version 1 \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(json.dumps({"bomFormat":"CycloneDX","specVersion":"1.5","version":1,"components":[{"type":"library","name":p["name"],"version":p["version"],"purl":"pkg:cargo/%s@%s"%(p["name"],p["version"])} for p in sorted(d["packages"],key=lambda p:(p["name"],p["version"]))]},sort_keys=True,separators=(",",":")))' \
  > "$STAGE/SBOM.cdx.json"

tar -C "$OUT" -czf "$ARCHIVE" "$NAME"
(cd "$OUT" && shasum -a 256 "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256")
printf '%s\n' "$ARCHIVE"
