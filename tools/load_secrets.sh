#!/usr/bin/env bash
# Export the local secrets for tests/tools/workers. Secrets stay in the
# environment only; they are never written into task workspaces or commits.
set -a
. "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/.secrets/deepseek.env"
set +a
export ZENPI_TEST_BASE_URL="${DEEPSEEK_OPENAI_BASE_URL:-https://api.deepseek.com/v1}"
export ZENPI_TEST_ANTHROPIC_BASE_URL="${DEEPSEEK_ANTHROPIC_BASE_URL:-https://api.deepseek.com/anthropic/v1}"
export ZENPI_TEST_API_KEY="${DEEPSEEK_API_KEY:-}"
export ZENPI_TEST_MODEL="${DEEPSEEK_MODEL:-deepseek-chat}"
