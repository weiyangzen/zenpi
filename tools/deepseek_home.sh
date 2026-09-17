#!/usr/bin/env bash
# Build a throwaway ZENPI_HOME with both DeepSeek official profiles
# (OpenAI chat wire + Anthropic Messages wire) from .secrets/deepseek.env.
# Prints the home path. Never prints the key.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
ENVF="${ZENPI_SECRETS_FILE:-$ROOT/.secrets/deepseek.env}"
if [ ! -f "$ENVF" ]; then echo "missing $ENVF" >&2; exit 2; fi
set -a; . "$ENVF"; set +a
: "${DEEPSEEK_API_KEY:?fill DEEPSEEK_API_KEY in $ENVF}"
OUT="${1:-/tmp/zenpi-deepseek-home}"
rm -rf "$OUT"; mkdir -p "$OUT"; chmod 700 "$OUT"
cat > "$OUT/config.toml" <<TOML
default_profile = "deepseek-openai"

[profiles.deepseek-openai]
backend = "openai"
provider = "openai"
model = "${DEEPSEEK_MODEL:-deepseek-chat}"
base_url = "${DEEPSEEK_OPENAI_BASE_URL:-https://api.deepseek.com/v1}"
wire_api = "chat"
timeout_seconds = 120
max_retries = 0

[profiles.deepseek-anthropic]
backend = "openai"
provider = "anthropic"
model = "${DEEPSEEK_MODEL:-deepseek-chat}"
base_url = "${DEEPSEEK_ANTHROPIC_BASE_URL:-https://api.deepseek.com/anthropic/v1}"
wire_api = "anthropic"
timeout_seconds = 120
max_retries = 0
TOML
umask 077
cat > "$OUT/auth.json" <<JSON
{"profiles":{"deepseek-openai":{"api_key":"${DEEPSEEK_API_KEY}"},"deepseek-anthropic":{"api_key":"${DEEPSEEK_API_KEY}"}}}
JSON
chmod 600 "$OUT/auth.json"
echo "$OUT"
