#!/usr/bin/env bash
set -euo pipefail

pkgbuild=
pkgver=
pkgrel=

usage() {
  echo "usage: $0 --pkgbuild PATH --pkgver VERSION --pkgrel NUMBER" >&2
}

while (( $# > 0 )); do
  case "$1" in
    --pkgbuild)
      pkgbuild="${2:-}"
      shift 2
      ;;
    --pkgver)
      pkgver="${2:-}"
      shift 2
      ;;
    --pkgrel)
      pkgrel="${2:-}"
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "${pkgbuild}" || -z "${pkgver}" || -z "${pkgrel}" ]]; then
  usage
  exit 2
fi

export TAURI_PACKAGING_PKGVER="${pkgver}"
export TAURI_PACKAGING_PKGREL="${pkgrel}"
# shellcheck source=/dev/null
source "${pkgbuild}"

binary_name="${_tauri_binary:-${pkgname}}"
smoke_enabled="${_tauri_smoke_enabled:-1}"
smoke_timeout="${_tauri_smoke_timeout:-15}"
smoke_env=()
if declare -p _tauri_smoke_env >/dev/null 2>&1; then
  smoke_env=("${_tauri_smoke_env[@]}")
fi

if [[ "${smoke_enabled}" != 1 ]]; then
  exit 0
fi

launch_log="$(mktemp "${TMPDIR:-/tmp}/tauri-packaging-launch.XXXXXXXX.log")"
cleanup() {
  rm -f -- "${launch_log}"
}
trap cleanup EXIT

set +e
env "${smoke_env[@]}" \
  timeout --signal=TERM "${smoke_timeout}s" \
  dbus-run-session -- xvfb-run -a "/usr/bin/${binary_name}" \
  >"${launch_log}" 2>&1
status=$?
set -e

if [[ "${status}" -ne 124 ]]; then
  cat "${launch_log}" >&2
  echo "${binary_name} exited before the headless launch timeout (status ${status})" >&2
  exit 1
fi
