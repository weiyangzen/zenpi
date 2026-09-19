#!/usr/bin/env bash
# Detect and (with explicit permission) install the external monitors the
# compact resource pane prefers: `htop` for process detail and `nvidia-smi`
# for discrete GPU telemetry.
#
# This script never installs anything unless ZENPI_INSTALL_MONITORS=1 is set,
# so a normal TUI start only probes. Missing tools are a typed "unavailable"
# state, not an error.
set -euo pipefail

install="${ZENPI_INSTALL_MONITORS:-0}"

have() { command -v "$1" >/dev/null 2>&1; }

report() {
  local tool="$1" state="$2"
  printf '%s=%s\n' "$tool" "$state"
}

probe() {
  if have htop; then report htop present; else report htop missing; fi
  if have nvidia-smi; then report nvidia-smi present; else report nvidia-smi missing; fi
  if have rocminfo; then report rocminfo present; else report rocminfo missing; fi
}

if [[ "$install" != "1" ]]; then
  probe
  printf 'install=skipped (set ZENPI_INSTALL_MONITORS=1 to install)\n'
  exit 0
fi

os="$(uname -s)"
case "$os" in
  Darwin)
    if ! have brew; then
      printf 'install=failed reason=homebrew-missing\n' >&2
      exit 1
    fi
    have htop || brew install htop
    # nvidia-smi has no macOS package; report it as unavailable rather than fail.
    have nvidia-smi || printf 'nvidia-smi=unavailable-on-macos\n'
    ;;
  Linux)
    if have apt-get; then
      sudo apt-get update
      have htop || sudo apt-get install -y htop
      have nvidia-smi || sudo apt-get install -y nvidia-utils-$(uname -r | cut -d- -f1) || \
        printf 'nvidia-smi=unavailable reason=no-matching-nvidia-utils-package\n'
    elif have dnf; then
      have htop || sudo dnf install -y htop
      have nvidia-smi || sudo dnf install -y nvidia-smi || \
        printf 'nvidia-smi=unavailable reason=no-matching-package\n'
    else
      printf 'install=failed reason=no-supported-package-manager\n' >&2
      exit 1
    fi
    ;;
  *)
    printf 'install=failed reason=unsupported-os:%s\n' "$os" >&2
    exit 1
    ;;
esac

probe
printf 'install=done\n'
