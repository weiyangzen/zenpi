#!/usr/bin/env bash
# LAN headless cluster dispatch helper (ZS1-160).
#
# This is the thin operator-facing wrapper around the Rust control plane's
# contract: read-only probing of explicitly authorized LAN hosts, dispatch of a
# headless worker to one of them by capacity, and reclamation of that worker.
#
# Safety contract:
#   * Only hosts listed in ZENPI_CLUSTER_AUTHORIZED_HOSTS (comma/space
#     separated, private LAN addresses only) are ever touched.
#   * Probing is read-only (`nproc`, `free`, `nvidia-smi`).
#   * Dispatch relies on the local SSH agent/key (BatchMode=yes). A password is
#     never placed on the command line.
#   * Every remote command is built from quoted, validated arguments and the
#     script refuses public/loopback/CGNAT addresses.
#
# Usage:
#   tools/cluster_dispatch.sh [--allow-skip] probe  [HOST ...]
#   tools/cluster_dispatch.sh [--allow-skip] dispatch HOST WORKER_NAME [PROMPT]
#   tools/cluster_dispatch.sh [--allow-skip] reclaim  HOST PID
#   tools/cluster_dispatch.sh [--allow-skip] list
#
# HOST may be omitted from probe (the authorization list is used) or must be an
# explicitly authorized address. The remote binary and state directory default
# to `zenpi` and `~/.zenpi/cluster`; override with ZENPI_CLUSTER_BIN and
# ZENPI_CLUSTER_DIR.
set -euo pipefail

ALLOW_SKIP=0
SSH_BIN="${ZENPI_CLUSTER_SSH:-ssh}"
REMOTE_BIN="${ZENPI_CLUSTER_BIN:-zenpi}"
REMOTE_DIR="${ZENPI_CLUSTER_DIR:-.zenpi/cluster}"
LOCAL_STATE="${ZENPI_CLUSTER_STATE:-${TMPDIR:-/tmp}/zenpi-cluster}"

usage() {
  sed -n '2,28p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

skip() {
  echo "cluster dispatch: skipped ($*)" >&2
  exit 0
}

fail() {
  echo "cluster dispatch: ERROR: $*" >&2
  exit 1
}

while (($# > 0)); do
  case "$1" in
    --allow-skip) ALLOW_SKIP=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) break ;;
  esac
done

COMMAND="${1:-}"
shift || true

if [[ "${ZENPI_CLUSTER_ALLOW_SKIP:-0}" == "1" ]]; then
  ALLOW_SKIP=1
fi

authorized_hosts() {
  local raw="${ZENPI_CLUSTER_AUTHORIZED_HOSTS:-}"
  [[ -n "$raw" ]] || fail "ZENPI_CLUSTER_AUTHORIZED_HOSTS is empty; refusing to touch any host"
  printf '%s\n' "$raw" | tr ', ' '\n\n' | awk 'NF'
}

# Reject anything that is not a private LAN IPv4 address. This mirrors the
# Rust `net_probe::is_lan_address` contract and keeps a typo from reaching the
# public internet or a shared-carrier address.
is_lan_ipv4() {
  local ip="$1" a b c d
  IFS=. read -r a b c d <<<"$ip"
  [[ "$a" =~ ^[0-9]+$ && "$b" =~ ^[0-9]+$ && "$c" =~ ^[0-9]+$ && "$d" =~ ^[0-9]+$ ]] || return 1
  ((a <= 255 && b <= 255 && c <= 255 && d <= 255)) || return 1
  ((a == 10)) && return 0
  ((a == 192 && b == 168)) && return 0
  ((a == 172 && b >= 16 && b <= 31)) && return 0
  return 1
}

require_authorized() {
  local host="$1" entry
  is_lan_ipv4 "$host" || fail "$host is not a private LAN address"
  while IFS= read -r entry; do
    [[ "$entry" == "$host" ]] && return 0
  done < <(authorized_hosts)
  fail "$host is not in ZENPI_CLUSTER_AUTHORIZED_HOSTS"
}

ssh_run() {
  local host="$1" command="$2"
  "$SSH_BIN" -o BatchMode=yes -o ConnectTimeout=5 "$host" "$command"
}

probe_host() {
  local host="$1"
  require_authorized "$host"
  if ! command -v "$SSH_BIN" >/dev/null 2>&1; then
    ((ALLOW_SKIP)) && skip "ssh is not installed"
    fail "ssh is not installed"
  fi
  local report
  report="$(ssh_run "$host" \
    'printf "cpus=%s\n" "$(nproc 2>/dev/null || echo 0)"; \
     printf "mem_kb=%s\n" "$(awk "/MemTotal/{print \$2}" /proc/meminfo 2>/dev/null || echo 0)"; \
     printf "gpus=%s\n" "$(command -v nvidia-smi >/dev/null 2>&1 && nvidia-smi -L 2>/dev/null | wc -l || echo 0)"' 2>/dev/null)" \
    || { ((ALLOW_SKIP)) && skip "probe of $host failed"; fail "probe of $host failed"; }
  printf '%s\t%s\n' "$host" "$(printf '%s' "$report" | tr '\n' ' ')"
}

dispatch_worker() {
  local host="$1" name="$2" prompt="${3:-}"
  require_authorized "$host"
  [[ "$name" =~ ^[A-Za-z0-9_-]+$ ]] || fail "worker name must be [A-Za-z0-9_-]+"
  local remote_id="${name}-$$"
  local quoted_dir quoted_session quoted_log quoted_bin
  quoted_dir="$(printf '%q' "$REMOTE_DIR")"
  quoted_session="$(printf '%q' "$REMOTE_DIR/$remote_id.jsonl")"
  quoted_log="$(printf '%q' "$REMOTE_DIR/$remote_id.log")"
  quoted_bin="$(printf '%q' "$REMOTE_BIN")"
  local stdin
  if [[ -n "$prompt" ]]; then
    stdin="printf '%s\\n' $(printf '%q' "$prompt") |"
  else
    stdin="printf '' |"
  fi
  local command
  command="mkdir -p $quoted_dir && { $stdin nohup $quoted_bin --mode headless --session $quoted_session >$quoted_log 2>&1 & echo \$!; }"
  local pid
  pid="$(ssh_run "$host" "$command" | awk 'NF{last=$0} END{print last}')" \
    || { ((ALLOW_SKIP)) && skip "dispatch to $host failed"; fail "dispatch to $host failed"; }
  [[ "$pid" =~ ^[0-9]+$ ]] || fail "dispatch to $host did not return a pid"
  mkdir -p "$LOCAL_STATE"
  printf '%s %s %s %s\n' "$host" "$remote_id" "$pid" "$(date +%s)" >>"$LOCAL_STATE/workers"
  printf '%s\n' "$host $remote_id pid=$pid"
}

reclaim_worker() {
  local host="$1" pid="$2"
  require_authorized "$host"
  [[ "$pid" =~ ^[0-9]+$ ]] || fail "pid must be numeric"
  if ssh_run "$host" "kill $pid" >/dev/null 2>&1; then
    printf '%s\n' "reclaimed $host pid=$pid"
  elif ! ssh_run "$host" "kill -0 $pid" >/dev/null 2>&1; then
    printf '%s\n' "already exited $host pid=$pid"
  else
    fail "could not reclaim $host pid=$pid"
  fi
}

list_workers() {
  [[ -f "$LOCAL_STATE/workers" ]] || return 0
  cat "$LOCAL_STATE/workers"
}

case "$COMMAND" in
  probe)
    if (($# > 0)); then
      for host in "$@"; do probe_host "$host"; done
    else
      while IFS= read -r host; do probe_host "$host"; done < <(authorized_hosts)
    fi
    ;;
  dispatch)
    (($# >= 2)) || fail "dispatch needs HOST and WORKER_NAME"
    dispatch_worker "$1" "$2" "${3:-}"
    ;;
  reclaim)
    (($# == 2)) || fail "reclaim needs HOST and PID"
    reclaim_worker "$1" "$2"
    ;;
  list)
    list_workers
    ;;
  ""|-h|--help)
    usage
    ;;
  *)
    fail "unknown command: $COMMAND"
    ;;
esac
