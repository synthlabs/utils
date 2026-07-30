#!/usr/bin/env bash

tauri_packaging_detect_platform() {
  local kernel
  kernel="${TAURI_PACKAGING_UNAME_S:-$(uname -s)}"

  case "${kernel}" in
    Darwin)
      printf '%s\n' macos
      ;;
    Linux)
      tauri_packaging_detect_linux_family
      ;;
    CYGWIN* | MINGW* | MSYS*)
      printf '%s\n' windows
      ;;
    *)
      printf '%s\n' unsupported
      ;;
  esac
}

tauri_packaging_detect_linux_family() (
  set +u

  local family os_release
  os_release="${TAURI_PACKAGING_OS_RELEASE_FILE:-/etc/os-release}"
  if [[ ! -r "${os_release}" ]]; then
    echo "cannot read OS metadata: ${os_release}" >&2
    return 1
  fi

  ID=
  ID_LIKE=
  # shellcheck source=/dev/null
  source "${os_release}"

  for family in "${ID:-}" ${ID_LIKE:-}; do
    case "${family}" in
      debian | ubuntu)
        printf '%s\n' debian
        return
        ;;
      arch | cachyos | manjaro)
        printf '%s\n' arch
        return
        ;;
    esac
  done

  printf '%s\n' linux
)

tauri_packaging_machine() {
  printf '%s\n' "${TAURI_PACKAGING_MACHINE:-$(uname -m)}"
}

tauri_packaging_require_arch_x86_64() {
  local machine
  machine="$(tauri_packaging_machine)"
  if [[ "${machine}" != x86_64 ]]; then
    echo "Arch-family Tauri packaging currently supports x86_64 only, not ${machine}" >&2
    return 1
  fi
}
